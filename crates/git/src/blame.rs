//! Annotate / blame (D25): who last changed each line of a working-tree file,
//! and the same mapped onto the fields of a request `.toml`.
//!
//! Uses `git blame --incremental --contents -` fed with the file as it is on
//! disk, so uncommitted lines come back as "not committed" (all fields `None`).
//! Results are cached per (path, size, mtime, HEAD).

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use toml_edit::{Item, TableLike};

use crate::{
    Error, Result, canonicalize_lenient, cli, enclosing_repo, in_repo, io_err, pathspec, read,
};

/// Who last changed a line. Every field is `None` for uncommitted lines.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlameInfo {
    pub commit: Option<String>,
    pub short_id: Option<String>,
    pub author: Option<String>,
    pub author_email: Option<String>,
    /// Author time, Unix seconds.
    pub time: Option<i64>,
    pub summary: Option<String>,
}

impl BlameInfo {
    fn is_uncommitted(&self) -> bool {
        self.commit.is_none()
    }

    /// Ordering by recency: uncommitted is newest, then by author time.
    fn recency(&self) -> (bool, i64) {
        (self.is_uncommitted(), self.time.unwrap_or(i64::MIN))
    }
}

/// One entry per line of the file (line N = index N-1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileBlame {
    pub path: PathBuf,
    pub lines: Vec<BlameInfo>,
}

/// Blame mapped onto the fields of a request file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestBlame {
    pub path: PathBuf,
    pub name: Option<BlameInfo>,
    pub method: Option<BlameInfo>,
    pub url: Option<BlameInfo>,
    pub headers: BTreeMap<String, BlameInfo>,
    pub query: BTreeMap<String, BlameInfo>,
    /// One per line of the body content (as split on `\n`).
    pub body_lines: Vec<BlameInfo>,
    pub form: BTreeMap<String, BlameInfo>,
    pub pre_request_lines: Vec<BlameInfo>,
    pub tests_lines: Vec<BlameInfo>,
}

type CacheKey = (u64, Option<SystemTime>, Option<String>);
type Cache = Mutex<HashMap<PathBuf, (CacheKey, Vec<BlameInfo>)>>;

fn cache() -> &'static Cache {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

const CACHE_MAX: usize = 128;

/// Blame of a file in its nearest enclosing repo, including uncommitted lines.
pub fn blame_file(path: impl AsRef<Path>) -> Result<FileBlame> {
    let path = canonicalize_lenient(path)?;
    let content = fs::read(&path).map_err(io_err(&path))?;
    let lines = blame_lines(&path, &content)?;
    Ok(FileBlame { path, lines })
}

fn blame_lines(path: &Path, content: &[u8]) -> Result<Vec<BlameInfo>> {
    let root = path
        .parent()
        .and_then(enclosing_repo)
        .ok_or_else(|| crate::invalid(path, "not inside a git repository"))?;
    let (_, rel) = in_repo(&root, path)?;
    let repo = read::open(&root)?;
    let head = repo
        .head()
        .ok()
        .and_then(|h| h.id())
        .map(|id| id.to_string());
    let meta = fs::metadata(path).map_err(io_err(path))?;
    let key: CacheKey = (meta.len(), meta.modified().ok(), head.clone());
    if let Some((k, lines)) = cache().lock().unwrap_or_else(|e| e.into_inner()).get(path)
        && *k == key
    {
        return Ok(lines.clone());
    }

    let n_lines = count_lines(content);
    let in_head = head.is_some() && read::blob_id_at_head(&repo, &rel)?.is_some();
    let lines = if in_head {
        let out = cli::git_stdin(
            &root,
            [
                "blame",
                "--incremental",
                "--contents",
                "-",
                "--",
                &pathspec(&rel),
            ],
            content.to_vec(),
        )?;
        parse_incremental(&out, n_lines)
    } else {
        // Untracked, newly added or empty repo: nothing is committed yet.
        vec![BlameInfo::default(); n_lines]
    };

    let mut c = cache().lock().unwrap_or_else(|e| e.into_inner());
    if c.len() >= CACHE_MAX {
        c.clear();
    }
    c.insert(path.to_path_buf(), (key, lines.clone()));
    Ok(lines)
}

/// Lines as git counts them (a final newline doesn't start a new line).
fn count_lines(content: &[u8]) -> usize {
    if content.is_empty() {
        return 0;
    }
    content.iter().filter(|b| **b == b'\n').count() + usize::from(!content.ends_with(b"\n"))
}

fn is_hex_id(s: &str) -> bool {
    (s.len() == 40 || s.len() == 64) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Parse `git blame --incremental` output into one entry per line.
pub(crate) fn parse_incremental(out: &[u8], n_lines: usize) -> Vec<BlameInfo> {
    let text = String::from_utf8_lossy(out);
    let mut commits: HashMap<String, BlameInfo> = HashMap::new();
    let mut lines = vec![BlameInfo::default(); n_lines];
    // (sha, first final line (1-based), count)
    let mut group: Option<(String, usize, usize)> = None;
    for line in text.lines() {
        if group.is_none() {
            let mut f = line.split(' ');
            if let (Some(sha), Some(_orig), Some(fin), Some(num)) =
                (f.next(), f.next(), f.next(), f.next())
                && is_hex_id(sha)
                && let (Ok(fin), Ok(num)) = (fin.parse(), num.parse())
            {
                let uncommitted = sha.bytes().all(|b| b == b'0');
                commits.entry(sha.to_string()).or_insert_with(|| {
                    if uncommitted {
                        BlameInfo::default()
                    } else {
                        BlameInfo {
                            commit: Some(sha.to_string()),
                            short_id: Some(sha[..7].to_string()),
                            ..Default::default()
                        }
                    }
                });
                group = Some((sha.to_string(), fin, num));
            }
            continue;
        }
        let (sha, fin, num) = group.clone().expect("in group");
        let (k, v) = line.split_once(' ').unwrap_or((line, ""));
        let info = commits.get_mut(&sha).expect("commit entry");
        if !info.is_uncommitted() {
            match k {
                "author" => info.author = Some(v.to_string()),
                "author-mail" => {
                    info.author_email =
                        Some(v.trim_start_matches('<').trim_end_matches('>').to_string())
                }
                "author-time" => info.time = v.parse().ok(),
                "summary" => info.summary = Some(v.to_string()),
                _ => {}
            }
        }
        if k == "filename" {
            let info = info.clone();
            for l in fin..fin + num {
                if let Some(slot) = lines.get_mut(l.wrapping_sub(1)) {
                    *slot = info.clone();
                }
            }
            group = None;
        }
    }
    lines
}

// ---------- Request mapping ----------

struct Mapper<'a> {
    src: &'a str,
    line_starts: Vec<usize>,
    blame: &'a [BlameInfo],
}

impl<'a> Mapper<'a> {
    fn new(src: &'a str, blame: &'a [BlameInfo]) -> Self {
        let mut line_starts = vec![0];
        line_starts.extend(src.match_indices('\n').map(|(i, _)| i + 1));
        Self {
            src,
            line_starts,
            blame,
        }
    }

    /// 0-based line of a byte offset.
    fn line_of(&self, offset: usize) -> usize {
        self.line_starts.partition_point(|s| *s <= offset) - 1
    }

    /// Most recent blame among 0-based lines `first..=last`.
    fn newest(&self, first: usize, last: usize) -> Option<BlameInfo> {
        (first..=last)
            .filter_map(|l| self.blame.get(l))
            .max_by_key(|b| b.recency())
            .cloned()
    }

    /// Blame of a `key = value` entry: newest line from key to end of value.
    fn entry(&self, table: &dyn TableLike, key: &str) -> Option<BlameInfo> {
        let (k, item) = table.get_key_value(key)?;
        let vspan = item.span()?;
        let start = k.span().map_or(vspan.start, |s| s.start.min(vspan.start));
        let end = vspan.end.max(start + 1);
        self.newest(self.line_of(start), self.line_of(end - 1))
    }

    fn entries(&self, item: Option<&Item>) -> BTreeMap<String, BlameInfo> {
        let Some(table) = item.and_then(Item::as_table_like) else {
            return BTreeMap::new();
        };
        table
            .iter()
            .filter_map(|(k, _)| Some((k.to_string(), self.entry(table, k)?)))
            .collect()
    }

    /// One blame per content line of a string value.
    fn content_lines(&self, item: Option<&Item>) -> Vec<BlameInfo> {
        let Some(span) = item.filter(|i| i.as_str().is_some()).and_then(Item::span) else {
            return Vec::new();
        };
        string_line_sources(&self.src[span.clone()])
            .into_iter()
            .map(|r| {
                let first = self.line_of(span.start + r.start);
                let last = self.line_of(span.start + r.end.max(r.start + 1) - 1);
                self.newest(first, last).unwrap_or_default()
            })
            .collect()
    }
}

/// For a raw TOML string literal (quotes included), the byte range in the
/// literal that produced each content line (content split on `\n`).
pub(crate) fn string_line_sources(raw: &str) -> Vec<Range<usize>> {
    let b = raw.as_bytes();
    let (basic, delim) = if raw.starts_with("\"\"\"") {
        (true, 3)
    } else if raw.starts_with("'''") {
        (false, 3)
    } else {
        (raw.starts_with('"'), 1)
    };
    let end = b.len().saturating_sub(delim).max(delim);
    let mut i = delim;
    // A newline right after an opening `'''`/`"""` is not part of the content.
    if delim == 3 {
        if b[i..].starts_with(b"\r\n") {
            i += 2;
        } else if b.get(i) == Some(&b'\n') {
            i += 1;
        }
    }
    let mut out = Vec::new();
    let mut cur = i..i;
    while i < end {
        let c = b[i];
        if c == b'\n' {
            cur.end = i;
            out.push(cur);
            i += 1;
            cur = i..i;
            continue;
        }
        if basic && c == b'\\' {
            let next = b.get(i + 1).copied();
            match next {
                Some(b'n') => {
                    // Escaped newline: next content line starts on this source line.
                    cur.end = i + 2;
                    out.push(cur);
                    i += 2;
                    cur = i..i;
                    continue;
                }
                Some(b' ' | b'\t' | b'\r' | b'\n') if delim == 3 => {
                    // Line-ending backslash: swallow whitespace and newlines.
                    i += 1;
                    while i < end && matches!(b[i], b' ' | b'\t' | b'\r' | b'\n') {
                        i += 1;
                    }
                    cur.end = i;
                    continue;
                }
                Some(b'u') => i += 6,
                Some(b'U') => i += 10,
                _ => i += 2,
            }
            cur.end = i.min(end);
            continue;
        }
        i += 1;
        cur.end = i;
    }
    cur.end = cur.end.min(end).max(cur.start);
    out.push(cur);
    out
}

/// Blame of a request `.toml`, mapped onto its fields.
pub fn blame_request(path: impl AsRef<Path>) -> Result<RequestBlame> {
    let path = canonicalize_lenient(path)?;
    let content = fs::read(&path).map_err(io_err(&path))?;
    let lines = blame_lines(&path, &content)?;
    let src = String::from_utf8(content)
        .map_err(|_| crate::invalid(&path, "request file is not valid UTF-8"))?;
    let doc = toml_edit::Document::parse(src.as_str()).map_err(|e| Error::InvalidPath {
        path: path.clone(),
        reason: format!("invalid TOML: {e}"),
    })?;
    let m = Mapper::new(&src, &lines);
    let root: &dyn TableLike = doc.as_table();
    let body = root.get("body").and_then(Item::as_table_like);
    let scripts = root.get("scripts").and_then(Item::as_table_like);
    Ok(RequestBlame {
        name: m.entry(root, "name"),
        method: m.entry(root, "method"),
        url: m.entry(root, "url"),
        headers: m.entries(root.get("headers")),
        query: m.entries(root.get("query")),
        body_lines: m.content_lines(body.and_then(|b| b.get("content"))),
        form: m.entries(body.and_then(|b| b.get("fields"))),
        pre_request_lines: m.content_lines(scripts.and_then(|s| s.get("pre_request"))),
        tests_lines: m.content_lines(scripts.and_then(|s| s.get("tests"))),
        path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contents(raw: &str) -> Vec<&str> {
        string_line_sources(raw)
            .into_iter()
            .map(|r| &raw[r])
            .collect()
    }

    #[test]
    fn maps_string_literals_to_content_lines() {
        assert_eq!(contents("\"a\\nb\""), ["a\\n", "b"]);
        assert_eq!(contents("'''\nx\ny\n'''"), ["x", "y", ""]);
        assert_eq!(contents("'''x\ny'''"), ["x", "y"]);
        assert_eq!(contents("\"\"\"\r\nx\\\n   y\"\"\""), ["x\\\n   y"]);
        assert_eq!(contents("''"), [""]);
        assert_eq!(contents("\"\\u00e9\\n\""), ["\\u00e9\\n", ""]);
    }

    #[test]
    fn parses_incremental_output() {
        let a = "a".repeat(40);
        let out = format!(
            "{a} 1 1 2\nauthor Ann\nauthor-mail <ann@x>\nauthor-time 100\nsummary first\nfilename f\n\
{z} 3 3 1\nauthor Not Committed Yet\nfilename f\n{a} 4 4 1\nfilename f\n",
            z = "0".repeat(40)
        );
        let l = parse_incremental(out.as_bytes(), 4);
        assert_eq!(l[0].author.as_deref(), Some("Ann"));
        assert_eq!(l[1].author_email.as_deref(), Some("ann@x"));
        assert_eq!(l[1].time, Some(100));
        assert_eq!(l[2], BlameInfo::default());
        assert_eq!(l[3].summary.as_deref(), Some("first"));
        assert_eq!(l[3].short_id.as_deref(), Some("aaaaaaa"));
    }
}
