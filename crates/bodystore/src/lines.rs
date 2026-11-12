//! Line index (one streaming pass), line reads with a per-line byte cap, and
//! a search that scans in bounded windows (a minified 50 MB JSON is a
//! single line and is never materialised whole).

use std::io::{self, BufRead, BufReader, Read};

use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

use crate::body::{Body, utf8_floor};

/// Default `max_line_bytes` for [`read_lines`].
pub const DEFAULT_MAX_LINE_BYTES: usize = 64 * 1024;

/// Start offset of every line. Lines end with `\n`; a trailing `\r` is part
/// of the separator. `"a\nb"` → `[0, 2]`; `"a\n"` → `[0, 2]` (last line empty).
pub fn build_line_index(input: impl Read) -> io::Result<Vec<u64>> {
    let mut r = BufReader::with_capacity(256 * 1024, input);
    let mut index = vec![0u64];
    let mut pos = 0u64;
    loop {
        let buf = r.fill_buf()?;
        if buf.is_empty() {
            break;
        }
        let n = buf.len();
        for (i, &b) in buf.iter().enumerate() {
            if b == b'\n' {
                index.push(pos + i as u64 + 1);
            }
        }
        pos += n as u64;
        r.consume(n);
    }
    Ok(index)
}

/// One line as returned to the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// At most `max_line_bytes` bytes of the line, cut on a UTF-8 boundary.
    pub text: String,
    /// Byte offset of the line start in the view.
    pub offset: u64,
    /// Full byte length of the line, without `\n` / `\r\n`.
    pub byte_length: u64,
}

/// Lines `[start, start + count)` of `body` using its `index`.
pub fn read_lines(
    body: &Body,
    index: &[u64],
    start: usize,
    count: usize,
    max_line_bytes: usize,
) -> io::Result<Vec<Line>> {
    let total = index.len();
    let end = start.saturating_add(count).min(total);
    let mut out = Vec::with_capacity(end.saturating_sub(start));
    for i in start..end {
        let from = index[i];
        let next = index.get(i + 1).copied();
        // Content end: before "\n" (and "\r") of this line.
        let mut to = next.map_or(body.len(), |n| n - 1);
        if to > from && body.read_range(to - 1, 1)? == b"\r" {
            to -= 1;
        }
        let byte_length = to - from;
        let take = (byte_length as usize).min(max_line_bytes);
        let mut bytes = body.read_range(from, take)?;
        if (take as u64) < byte_length {
            bytes.truncate(utf8_floor(&bytes));
        }
        out.push(Line {
            text: String::from_utf8_lossy(&bytes).into_owned(),
            offset: from,
            byte_length,
        });
    }
    Ok(out)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchOptions {
    #[serde(default)]
    pub regex: bool,
    #[serde(default)]
    pub case_sensitive: bool,
    #[serde(default)]
    pub pretty: bool,
    #[serde(default = "default_max_results")]
    pub max_results: usize,
}

fn default_max_results() -> usize {
    1000
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchMatch {
    pub line: u64,
    /// Character column and length within the line.
    pub column: usize,
    pub length: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchResult {
    pub matches: Vec<SearchMatch>,
    pub truncated: bool,
}

pub fn build_regex(query: &str, opts: &SearchOptions) -> Result<Regex, String> {
    let pattern = if opts.regex {
        query.to_string()
    } else {
        regex::escape(query)
    };
    RegexBuilder::new(&pattern)
        .case_insensitive(!opts.case_sensitive)
        .size_limit(10 * 1024 * 1024)
        .build()
        .map_err(|e| format!("invalid regular expression: {e}"))
}

/// Window size for scanning long lines.
const WINDOW: usize = 1024 * 1024;
/// Overlap between windows: matches up to this long are always found whole.
const OVERLAP: usize = 64 * 1024;

/// Scan `input` line by line for `query`. Long lines are scanned in windows
/// of 1 MiB overlapping by 64 KiB, so memory stays bounded; a match longer
/// than 64 KiB that crosses a window boundary may be reported shorter, and
/// `^`/`\b` at a window start see the window start, not the line start.
pub fn search(input: impl Read, query: &str, opts: &SearchOptions) -> Result<SearchResult, String> {
    search_windowed(input, query, opts, WINDOW, OVERLAP)
}

fn search_windowed(
    input: impl Read,
    query: &str,
    opts: &SearchOptions,
    window: usize,
    overlap: usize,
) -> Result<SearchResult, String> {
    let mut result = SearchResult::default();
    if query.is_empty() {
        return Ok(result);
    }
    let re = build_regex(query, opts)?;
    let max = opts.max_results.max(1);
    let mut r = BufReader::with_capacity(256 * 1024, input);
    let mut seg: Vec<u8> = Vec::new();
    let mut line_no = 0u64;
    // Characters of the current line before `seg`.
    let mut chars_before = 0usize;
    loop {
        let mut line_end = false;
        let mut eof = false;
        while seg.len() < window {
            let buf = r.fill_buf().map_err(|e| e.to_string())?;
            if buf.is_empty() {
                eof = true;
                break;
            }
            let take = &buf[..buf.len().min(window - seg.len())];
            if let Some(pos) = take.iter().position(|&b| b == b'\n') {
                seg.extend_from_slice(&take[..pos]);
                r.consume(pos + 1);
                line_end = true;
                break;
            }
            let n = take.len();
            seg.extend_from_slice(take);
            r.consume(n);
        }
        if eof && !line_end && seg.is_empty() {
            break;
        }
        let last_window = line_end || eof;
        let usable = if last_window {
            seg.len()
        } else {
            utf8_floor(&seg)
        };
        let mut bytes = &seg[..usable];
        if last_window && bytes.last() == Some(&b'\r') {
            bytes = &bytes[..bytes.len() - 1];
        }
        let text = String::from_utf8_lossy(bytes).into_owned();
        // Matches starting at or after `limit` are left to the next window.
        let limit = if last_window {
            text.len()
        } else {
            let mut l = text.len().saturating_sub(overlap);
            while !text.is_char_boundary(l) {
                l -= 1;
            }
            l
        };
        let mut col_chars = chars_before;
        let mut col_bytes = 0usize;
        for m in re.find_iter(&text) {
            if m.start() >= limit && !last_window {
                break;
            }
            if m.start() == m.end() {
                continue;
            }
            col_chars += text[col_bytes..m.start()].chars().count();
            col_bytes = m.start();
            if result.matches.len() >= max {
                result.truncated = true;
                return Ok(result);
            }
            result.matches.push(SearchMatch {
                line: line_no,
                column: col_chars,
                length: m.as_str().chars().count(),
            });
        }
        if last_window {
            line_no += 1;
            chars_before = 0;
            seg.clear();
            if eof {
                break;
            }
        } else {
            chars_before += text[..limit].chars().count();
            let mut next = text.as_bytes()[limit..].to_vec();
            next.extend_from_slice(&seg[usable..]);
            seg = next;
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_and_lines_with_crlf_unicode_and_cut() {
        let text = "héllo\r\nwörld €\n\nlast";
        let body = Body::from(text.as_bytes().to_vec());
        let idx = build_line_index(text.as_bytes()).unwrap();
        assert_eq!(idx.len(), 4);
        let lines = read_lines(&body, &idx, 0, 10, 1000).unwrap();
        let texts: Vec<_> = lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["héllo", "wörld €", "", "last"]);
        let meta: Vec<_> = lines.iter().map(|l| (l.offset, l.byte_length)).collect();
        assert_eq!(meta, [(0, 6), (8, 10), (19, 0), (20, 4)]);
        // Cut to 9 bytes: "wörld " is 7 bytes, "€" (3) does not fit.
        let cut = read_lines(&body, &idx, 1, 1, 9).unwrap();
        assert_eq!(cut[0].text, "wörld ");
        assert_eq!(cut[0].byte_length, 10);
        assert!(read_lines(&body, &idx, 9, 1, 10).unwrap().is_empty());
        assert_eq!(build_line_index(&b"a\n"[..]).unwrap(), [0, 2]);
    }

    #[test]
    fn search_columns_are_characters() {
        let text = "ab€Foo foo\r\nno\nFOO";
        let opts = SearchOptions {
            max_results: 10,
            ..Default::default()
        };
        let r = search(text.as_bytes(), "foo", &opts).unwrap();
        let got: Vec<_> = r
            .matches
            .iter()
            .map(|m| (m.line, m.column, m.length))
            .collect();
        assert_eq!(got, [(0, 3, 3), (0, 7, 3), (2, 0, 3)]);

        let cs = SearchOptions {
            case_sensitive: true,
            ..opts.clone()
        };
        assert_eq!(
            search(text.as_bytes(), "foo", &cs).unwrap().matches.len(),
            1
        );

        let re = SearchOptions {
            regex: true,
            max_results: 2,
            ..Default::default()
        };
        let r = search(text.as_bytes(), "f.o|no", &re).unwrap();
        assert_eq!(r.matches.len(), 2);
        assert!(r.truncated);
        assert!(search(text.as_bytes(), "(", &re).is_err());
        assert_eq!(
            search("a.b".as_bytes(), ".", &opts).unwrap().matches.len(),
            1
        );
    }

    #[test]
    fn windowed_search_matches_whole_line_search() {
        // A long single line with multi-byte characters; tiny windows force
        // many window boundaries (and boundaries inside characters).
        let mut line = String::new();
        for i in 0..400 {
            line.push_str(&format!("é{i}€key{}😀 ", i % 7));
        }
        let text = format!("{line}\r\nsecond key3 line\n{line}");
        let opts = SearchOptions {
            regex: true,
            max_results: 100_000,
            ..Default::default()
        };
        let whole = search_windowed(text.as_bytes(), "key[0-3]", &opts, 1 << 30, 0).unwrap();
        for (window, overlap) in [(64, 16), (100, 30), (1000, 64)] {
            let w = search_windowed(text.as_bytes(), "key[0-3]", &opts, window, overlap).unwrap();
            assert_eq!(w, whole, "window {window}");
        }
        assert!(whole.matches.iter().any(|m| m.line == 1 && m.column == 7));
    }
}
