//! Streaming JSON pretty-printer: re-indents any amount of JSON with constant
//! memory (the nesting stack only), without building a value tree. The output
//! matches `serde_json::to_string_pretty` (2 spaces, `": "`, empty `{}`/`[]`)
//! except that strings and numbers are copied verbatim.

use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};

#[derive(Debug)]
pub enum PrettyError {
    Io(io::Error),
    /// Not valid JSON: message and byte offset.
    Invalid {
        offset: u64,
        message: &'static str,
    },
}

impl std::fmt::Display for PrettyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PrettyError::Io(e) => write!(f, "{e}"),
            PrettyError::Invalid { offset, message } => {
                write!(f, "not valid JSON at byte {offset}: {message}")
            }
        }
    }
}

impl std::error::Error for PrettyError {}

impl From<io::Error> for PrettyError {
    fn from(e: io::Error) -> Self {
        PrettyError::Io(e)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Expect {
    /// A value (top level, after `:`, after `,` in an array).
    Value,
    /// A value or `]` (just after `[`).
    ValueOrEnd,
    /// A key or `}` (just after `{`).
    KeyOrEnd,
    /// A key (after `,` in an object).
    Key,
    Colon,
    /// `,` or the closing bracket of the current container.
    CommaOrEnd,
    /// After the top-level value: only whitespace.
    Done,
}

struct Printer<W: Write> {
    out: W,
    /// `true` = object, `false` = array.
    stack: Vec<bool>,
    expect: Expect,
    /// A container was just opened and nothing written inside yet.
    pending_open: bool,
    offset: u64,
}

impl<W: Write> Printer<W> {
    fn err(&self, message: &'static str) -> PrettyError {
        PrettyError::Invalid {
            offset: self.offset,
            message,
        }
    }

    fn newline_indent(&mut self, depth: usize) -> io::Result<()> {
        self.out.write_all(b"\n")?;
        for _ in 0..depth {
            self.out.write_all(b"  ")?;
        }
        Ok(())
    }

    /// Before an element (value in array, key in object).
    fn begin_element(&mut self) -> io::Result<()> {
        if self.pending_open {
            self.pending_open = false;
            self.newline_indent(self.stack.len())?;
        }
        Ok(())
    }

    fn after_value(&mut self) {
        self.expect = if self.stack.is_empty() {
            Expect::Done
        } else {
            Expect::CommaOrEnd
        };
    }

    /// Start of a value or key token; validates and writes the separator.
    fn start_value(&mut self, is_string: bool) -> Result<bool, PrettyError> {
        match self.expect {
            Expect::Value | Expect::ValueOrEnd => {
                self.begin_element()?;
                Ok(false)
            }
            Expect::KeyOrEnd | Expect::Key if is_string => {
                self.begin_element()?;
                Ok(true)
            }
            Expect::KeyOrEnd | Expect::Key => Err(self.err("expected a string key")),
            Expect::Colon => Err(self.err("expected ':'")),
            Expect::CommaOrEnd => Err(self.err("expected ',' or a closing bracket")),
            Expect::Done => Err(self.err("trailing characters after the JSON value")),
        }
    }

    fn open(&mut self, object: bool) -> Result<(), PrettyError> {
        self.start_value(false)?;
        self.out.write_all(if object { b"{" } else { b"[" })?;
        self.stack.push(object);
        self.pending_open = true;
        self.expect = if object {
            Expect::KeyOrEnd
        } else {
            Expect::ValueOrEnd
        };
        Ok(())
    }

    fn close(&mut self, object: bool) -> Result<(), PrettyError> {
        let ok = match self.expect {
            Expect::KeyOrEnd => object,
            Expect::ValueOrEnd => !object,
            Expect::CommaOrEnd => self.stack.last() == Some(&object),
            _ => false,
        };
        if !ok || self.stack.last() != Some(&object) {
            return Err(self.err("unexpected closing bracket"));
        }
        self.stack.pop();
        if self.pending_open {
            self.pending_open = false;
        } else {
            self.newline_indent(self.stack.len())?;
        }
        self.out.write_all(if object { b"}" } else { b"]" })?;
        self.after_value();
        Ok(())
    }

    fn comma(&mut self) -> Result<(), PrettyError> {
        if self.expect != Expect::CommaOrEnd {
            return Err(self.err("unexpected ','"));
        }
        self.out.write_all(b",")?;
        self.newline_indent(self.stack.len())?;
        self.expect = if self.stack.last() == Some(&true) {
            Expect::Key
        } else {
            Expect::Value
        };
        Ok(())
    }

    fn colon(&mut self) -> Result<(), PrettyError> {
        if self.expect != Expect::Colon {
            return Err(self.err("unexpected ':'"));
        }
        self.out.write_all(b": ")?;
        self.expect = Expect::Value;
        Ok(())
    }
}

fn is_literal_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.')
}

fn valid_literal(lit: &[u8]) -> bool {
    match lit {
        b"true" | b"false" | b"null" => true,
        _ => {
            let s = lit.strip_prefix(b"-").unwrap_or(lit);
            !s.is_empty()
                && s[0].is_ascii_digit()
                && s.iter()
                    .all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'E' | b'+' | b'-'))
        }
    }
}

/// Pretty-print the JSON read from `input` into `output`.
pub fn pretty_json(input: impl Read, output: impl Write) -> Result<(), PrettyError> {
    let mut r = BufReader::with_capacity(256 * 1024, input);
    let mut p = Printer {
        out: BufWriter::with_capacity(256 * 1024, output),
        stack: Vec::new(),
        expect: Expect::Value,
        pending_open: false,
        offset: 0,
    };
    let mut literal: Vec<u8> = Vec::new();
    // Inside a string: Some(escaped_next).
    let mut in_string: Option<bool> = None;
    let mut string_is_key = false;

    loop {
        let buf = r.fill_buf()?;
        if buf.is_empty() {
            break;
        }
        let n = buf.len();
        let mut i = 0;
        while i < n {
            let b = buf[i];
            if let Some(escaped) = in_string {
                // Copy runs of plain string bytes at once.
                if !escaped {
                    let run = buf[i..]
                        .iter()
                        .position(|&c| c == b'"' || c == b'\\' || c < 0x20)
                        .unwrap_or(n - i);
                    if run > 0 {
                        p.out.write_all(&buf[i..i + run])?;
                        i += run;
                        p.offset += run as u64;
                        continue;
                    }
                }
                if b < 0x20 {
                    return Err(p.err("control character in string"));
                }
                p.out.write_all(&[b])?;
                in_string = if escaped {
                    Some(false)
                } else if b == b'\\' {
                    Some(true)
                } else if b == b'"' {
                    if string_is_key {
                        p.expect = Expect::Colon;
                    } else {
                        p.after_value();
                    }
                    None
                } else {
                    Some(false)
                };
                i += 1;
                p.offset += 1;
                continue;
            }
            if is_literal_byte(b) {
                if literal.is_empty() {
                    p.start_value(false)?;
                }
                literal.push(b);
                i += 1;
                p.offset += 1;
                continue;
            }
            if !literal.is_empty() {
                if !valid_literal(&literal) {
                    return Err(p.err("invalid literal"));
                }
                p.out.write_all(&literal)?;
                literal.clear();
                p.after_value();
            }
            match b {
                b' ' | b'\t' | b'\n' | b'\r' => {}
                b'{' => p.open(true)?,
                b'[' => p.open(false)?,
                b'}' => p.close(true)?,
                b']' => p.close(false)?,
                b',' => p.comma()?,
                b':' => p.colon()?,
                b'"' => {
                    string_is_key = p.start_value(true)?;
                    p.out.write_all(b"\"")?;
                    in_string = Some(false);
                }
                _ => return Err(p.err("unexpected character")),
            }
            i += 1;
            p.offset += 1;
        }
        r.consume(n);
    }
    if in_string.is_some() {
        return Err(p.err("unterminated string"));
    }
    if !literal.is_empty() {
        if !valid_literal(&literal) {
            return Err(p.err("invalid literal"));
        }
        p.out.write_all(&literal)?;
        p.after_value();
    }
    if p.expect != Expect::Done {
        return Err(p.err("unexpected end of input"));
    }
    p.out.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn pretty(s: &str) -> Result<String, PrettyError> {
        let mut out = Vec::new();
        pretty_json(s.as_bytes(), &mut out)?;
        Ok(String::from_utf8(out).unwrap())
    }

    /// Deterministic pseudo-random JSON values (xorshift).
    fn random_value(seed: &mut u64, depth: u32) -> Value {
        let mut next = || {
            *seed ^= *seed << 13;
            *seed ^= *seed >> 7;
            *seed ^= *seed << 17;
            *seed
        };
        let strings = [
            "",
            "plain",
            "quote \" and \\ backslash",
            "tab\tnew\nline",
            "unicode é € 😀",
            "\u{0001}ctl",
            "</script>",
            "{not: json}",
        ];
        match next() % if depth > 3 { 5 } else { 7 } {
            0 => Value::Null,
            1 => Value::Bool(next() % 2 == 0),
            2 => json!((next() % 100_000) as i64 - 50_000),
            3 => json!((next() % 10_000) as f64 / 7.0),
            4 => Value::String(strings[(next() % strings.len() as u64) as usize].to_string()),
            5 => {
                let n = next() % 5;
                Value::Array((0..n).map(|_| random_value(seed, depth + 1)).collect())
            }
            _ => {
                let n = next() % 5;
                let mut m = serde_json::Map::new();
                for i in 0..n {
                    m.insert(format!("k{i} \"é\""), random_value(seed, depth + 1));
                }
                Value::Object(m)
            }
        }
    }

    #[test]
    fn matches_serde_json_on_random_values() {
        let mut seed = 0x9E37_79B9_7F4A_7C15;
        for _ in 0..500 {
            let v = random_value(&mut seed, 0);
            let compact = serde_json::to_string(&v).unwrap();
            let expected = serde_json::to_string_pretty(&v).unwrap();
            assert_eq!(pretty(&compact).unwrap(), expected, "input: {compact}");
            // Already-indented input gives the same result.
            assert_eq!(pretty(&expected).unwrap(), expected);
        }
    }

    #[test]
    fn small_buffers_and_edge_cases() {
        // Read one byte at a time to cross every token boundary.
        struct OneByte<'a>(&'a [u8]);
        impl Read for OneByte<'_> {
            fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
                if self.0.is_empty() || buf.is_empty() {
                    return Ok(0);
                }
                buf[0] = self.0[0];
                self.0 = &self.0[1..];
                Ok(1)
            }
        }
        // Numbers are copied verbatim, so use serde_json's own spelling here.
        let src = r#"{"a\"b":[1,-2500.0,true,null,{},[],"x\\"],"c":{"d":"€"}}"#;
        let mut out = Vec::new();
        pretty_json(OneByte(src.as_bytes()), &mut out).unwrap();
        let v: Value = serde_json::from_str(src).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            serde_json::to_string_pretty(&v).unwrap()
        );
        assert_eq!(pretty("  42 \n").unwrap(), "42");
        assert_eq!(pretty("\"s\"").unwrap(), "\"s\"");
    }

    #[test]
    fn rejects_invalid_json() {
        for bad in [
            "",
            "{",
            "[1,]",
            "{\"a\" 1}",
            "{\"a\":1,}",
            "[1 2]",
            "{1:2}",
            "\"open",
            "tru",
            "[1]]",
            "{}{}",
            "nul",
            "[\"a\nb\"]",
            "]",
        ] {
            assert!(pretty(bad).is_err(), "should reject {bad:?}");
        }
    }
}
