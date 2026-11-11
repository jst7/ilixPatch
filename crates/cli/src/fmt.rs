//! Terminal formatting helpers: colors, sizes, durations and box tables.

use std::io::IsTerminal;
use std::sync::OnceLock;
use std::time::Duration;

fn color_enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal())
}

fn paint(code: &str, s: &str) -> String {
    if color_enabled() {
        format!("\x1b[{code}m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

pub fn green(s: &str) -> String {
    paint("32", s)
}
pub fn red(s: &str) -> String {
    paint("31", s)
}
pub fn yellow(s: &str) -> String {
    paint("33", s)
}
pub fn dim(s: &str) -> String {
    paint("2", s)
}
pub fn bold(s: &str) -> String {
    paint("1", s)
}

pub fn size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes}B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1}kB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

pub fn ms(v: f64) -> String {
    if v >= 10_000.0 {
        format!("{:.1}s", v / 1000.0)
    } else if v >= 10.0 {
        format!("{v:.0}ms")
    } else {
        format!("{v:.1}ms")
    }
}

/// Parse `30s`, `500ms`, `2m`, `1h`, `1.5s` or a bare number of seconds.
pub fn parse_duration(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    let (num, mult) = if let Some(n) = s.strip_suffix("ms") {
        (n, 0.001)
    } else if let Some(n) = s.strip_suffix('s') {
        (n, 1.0)
    } else if let Some(n) = s.strip_suffix('m') {
        (n, 60.0)
    } else if let Some(n) = s.strip_suffix('h') {
        (n, 3600.0)
    } else {
        (s, 1.0)
    };
    let v: f64 = num
        .trim()
        .parse()
        .map_err(|_| format!("invalid duration '{s}' (e.g. 30s, 500ms, 2m)"))?;
    if !v.is_finite() || v < 0.0 {
        return Err(format!("invalid duration '{s}'"));
    }
    Ok(Duration::from_secs_f64(v * mult))
}

/// `key=value` for `--global-var` / `--env-var`.
pub fn parse_kv(s: &str) -> Result<(String, String), String> {
    match s.split_once('=') {
        Some((k, v)) if !k.trim().is_empty() => Ok((k.trim().to_string(), v.to_string())),
        _ => Err(format!("expected key=value, got '{s}'")),
    }
}

/// Visible width (ignores ANSI escapes).
fn width(s: &str) -> usize {
    let mut w = 0;
    let mut esc = false;
    for c in s.chars() {
        if esc {
            if c == 'm' {
                esc = false;
            }
        } else if c == '\x1b' {
            esc = true;
        } else {
            w += 1;
        }
    }
    w
}

/// Render a box table. The first column is right-aligned like Newman's
/// summary when `right_first` is set; other columns are right-aligned.
pub fn table(
    header: &[&str],
    rows: &[Vec<String>],
    footer: &[String],
    right_first: bool,
) -> String {
    let cols = header.len();
    let mut w: Vec<usize> = header.iter().map(|h| width(h)).collect();
    for r in rows {
        for (i, c) in r.iter().enumerate().take(cols) {
            w[i] = w[i].max(width(c));
        }
    }
    let inner: usize = w.iter().map(|x| x + 2).sum::<usize>() + cols - 1;
    let footer_w = footer.iter().map(|f| width(f) + 2).max().unwrap_or(0);
    if footer_w > inner {
        w[0] += footer_w - inner;
    }
    let inner = inner.max(footer_w);
    let line = |l: &str, m: &str, r: &str| {
        let segs: Vec<String> = w.iter().map(|x| "─".repeat(x + 2)).collect();
        format!("{l}{}{r}\n", segs.join(m))
    };
    let cell = |s: &str, w: usize, right: bool| {
        let pad = w.saturating_sub(width(s));
        if right {
            format!(" {}{s} ", " ".repeat(pad))
        } else {
            format!(" {s}{} ", " ".repeat(pad))
        }
    };
    let row = |r: &[String]| {
        let cells: Vec<String> = r
            .iter()
            .enumerate()
            .map(|(i, c)| cell(c, w[i], i > 0 || right_first))
            .collect();
        format!("│{}│\n", cells.join("│"))
    };
    let mut out = line("┌", "┬", "┐");
    out += &row(&header.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    out += &line("├", "┼", "┤");
    for r in rows {
        out += &row(r);
    }
    if footer.is_empty() {
        out += &line("└", "┴", "┘");
    } else {
        out += &line("├", "┴", "┤");
        for (i, f) in footer.iter().enumerate() {
            if i > 0 {
                out += &format!("├{}┤\n", "─".repeat(inner));
            }
            out += &format!("│{}│\n", cell(f, inner - 2, false));
        }
        out += &format!("└{}┘\n", "─".repeat(inner));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        assert_eq!(parse_duration("30s").unwrap(), Duration::from_secs(30));
        assert_eq!(parse_duration("250ms").unwrap(), Duration::from_millis(250));
        assert_eq!(parse_duration("2m").unwrap(), Duration::from_secs(120));
        assert_eq!(parse_duration("5").unwrap(), Duration::from_secs(5));
        assert!(parse_duration("abc").is_err());
    }

    #[test]
    fn kv() {
        assert_eq!(parse_kv("a=b=c").unwrap(), ("a".into(), "b=c".into()));
        assert!(parse_kv("nope").is_err());
    }

    #[test]
    fn table_is_rectangular() {
        let t = table(
            &["", "executed", "failed"],
            &[vec!["requests".into(), "3".into(), "0".into()]],
            &["total run duration: 1.2s".into()],
            true,
        );
        let widths: Vec<usize> = t.lines().map(|l| l.chars().count()).collect();
        assert!(widths.iter().all(|&x| x == widths[0]), "{t}");
    }
}
