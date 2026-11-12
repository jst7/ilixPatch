//! `ResponseStore`: bodies by id with lazily built line indexes and a lazily
//! built pretty-JSON view, an LRU cap, and temp-file cleanup. All methods
//! are blocking (file I/O): call them off the async runtime's main threads.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::body::{Body, TEMP_PREFIX, utf8_floor};
use crate::lines::{SearchOptions, SearchResult, build_line_index, read_lines, search};
use crate::pretty::pretty_json;

/// `BodyHandle` in api.ts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BodyHandle {
    pub id: String,
    pub size: u64,
    /// "memory" or "file".
    pub stored: String,
    pub complete: bool,
    pub content_type: Option<String>,
    pub is_text: bool,
    pub is_json: bool,
    pub preview_truncated: bool,
}

/// `BodyChunk` in api.ts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BodyChunk {
    pub data: String,
    /// "utf8" or "base64".
    pub encoding: String,
    pub offset: u64,
    pub length: u64,
    pub eof: bool,
}

/// `BodyLines` in api.ts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BodyLines {
    pub start: u64,
    /// Each cut to at most `max_line_bytes` bytes (UTF-8 boundary).
    pub lines: Vec<String>,
    /// Byte offset of each line in the selected view.
    pub offsets: Vec<u64>,
    /// Full byte length of each line (without the line break).
    pub byte_lengths: Vec<u64>,
    pub total_lines: u64,
}

/// Largest chunk `read` returns.
pub const MAX_READ: usize = 8 * 1024 * 1024;
/// Most lines `lines` returns.
pub const MAX_LINES: usize = 10_000;

struct View {
    body: Body,
    index: OnceLock<Arc<Vec<u64>>>,
}

impl View {
    fn new(body: Body) -> Self {
        Self {
            body,
            index: OnceLock::new(),
        }
    }

    fn index(&self) -> Result<Arc<Vec<u64>>, String> {
        if let Some(i) = self.index.get() {
            return Ok(i.clone());
        }
        let idx = Arc::new(build_line_index(self.body.reader().map_err(err)?).map_err(err)?);
        Ok(self.index.get_or_init(|| idx).clone())
    }
}

struct Entry {
    raw: View,
    pretty: Mutex<Option<Result<Arc<View>, String>>>,
    content_type: Option<String>,
    is_text: bool,
    is_json: bool,
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Content type or byte sniffing says "text".
pub fn looks_like_text(content_type: Option<&str>, head: &[u8]) -> bool {
    if let Some(ct) = content_type.map(str::to_ascii_lowercase) {
        if ct.starts_with("text/")
            || [
                "json",
                "xml",
                "javascript",
                "x-www-form-urlencoded",
                "yaml",
                "csv",
                "html",
            ]
            .iter()
            .any(|t| ct.contains(t))
        {
            return true;
        }
        if ["image/", "audio/", "video/", "font/"]
            .iter()
            .any(|t| ct.starts_with(t))
            || ct.contains("octet-stream")
            || ct.contains("zip")
            || ct.contains("pdf")
            || ct.contains("protobuf")
        {
            return false;
        }
    }
    !head.contains(&0) && std::str::from_utf8(&head[..utf8_floor(head)]).is_ok()
}

/// Content type or the first non-blank byte says "JSON".
pub fn looks_like_json(content_type: Option<&str>, head: &[u8]) -> bool {
    if content_type.is_some_and(|ct| ct.to_ascii_lowercase().contains("json")) {
        return true;
    }
    matches!(
        head.iter().find(|b| !b.is_ascii_whitespace()),
        Some(b'{') | Some(b'[')
    )
}

struct Inner {
    entries: IndexMap<String, Arc<Entry>>,
    next_id: u64,
}

/// Bodies kept for the UI. Least recently used entries are dropped when the
/// total size of file bodies or the number of entries exceeds the caps.
pub struct ResponseStore {
    dir: PathBuf,
    max_file_bytes: u64,
    max_entries: usize,
    inner: Mutex<Inner>,
}

impl ResponseStore {
    /// `dir`: where pretty views are written (and bodies are spilled by the engine).
    pub fn new(dir: impl Into<PathBuf>, max_file_bytes: u64, max_entries: usize) -> Self {
        Self {
            dir: dir.into(),
            max_file_bytes,
            max_entries: max_entries.max(1),
            inner: Mutex::new(Inner {
                entries: IndexMap::new(),
                next_id: 1,
            }),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Delete temp files left by a previous run (crash, kill).
    pub fn clean_orphans(dir: &Path) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            if e.file_name().to_string_lossy().starts_with(TEMP_PREFIX) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Keep `body`; `inline_limit` = bytes sent whole in `SendResult.body`.
    pub fn insert(
        &self,
        body: Body,
        content_type: Option<String>,
        complete: bool,
        inline_limit: u64,
    ) -> BodyHandle {
        let head = body.preview(8192).unwrap_or_default();
        let is_text = looks_like_text(content_type.as_deref(), &head);
        let is_json = is_text && looks_like_json(content_type.as_deref(), &head);
        let size = body.len();
        let stored = if body.is_file() { "file" } else { "memory" };
        let entry = Arc::new(Entry {
            raw: View::new(body),
            pretty: Mutex::new(None),
            content_type: content_type.clone(),
            is_text,
            is_json,
        });
        let mut inner = self.lock();
        let id = format!("b{}", inner.next_id);
        inner.next_id += 1;
        inner.entries.insert(id.clone(), entry);
        // LRU caps (never evict the entry just added).
        loop {
            let files: u64 = inner
                .entries
                .values()
                .filter(|e| e.raw.body.is_file())
                .map(|e| e.raw.body.len())
                .sum();
            if inner.entries.len() <= 1
                || (files <= self.max_file_bytes && inner.entries.len() <= self.max_entries)
            {
                break;
            }
            inner.entries.shift_remove_index(0);
        }
        BodyHandle {
            id,
            size,
            stored: stored.into(),
            complete,
            content_type,
            is_text,
            is_json,
            preview_truncated: size > inline_limit,
        }
    }

    fn get(&self, id: &str) -> Result<Arc<Entry>, String> {
        let mut inner = self.lock();
        let i = inner
            .entries
            .get_index_of(id)
            .ok_or_else(|| format!("response body {id} is no longer available"))?;
        // Most recently used goes last.
        let last = inner.entries.len() - 1;
        inner.entries.move_index(i, last);
        Ok(inner.entries[last].clone())
    }

    pub fn contains(&self, id: &str) -> bool {
        self.lock().entries.contains_key(id)
    }

    pub fn len(&self) -> usize {
        self.lock().entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Free a body now (its temp files are deleted when no reader holds them).
    pub fn release(&self, id: &str) {
        self.lock().entries.shift_remove(id);
    }

    /// Free everything (app exit).
    pub fn clear(&self) {
        self.lock().entries.clear();
    }

    fn pretty(&self, entry: &Entry) -> Result<Arc<View>, String> {
        let mut slot = entry.pretty.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(r) = slot.as_ref() {
            return r.clone();
        }
        let result = if !entry.is_json {
            Err("the pretty view is only available for JSON bodies".to_string())
        } else {
            (|| {
                std::fs::create_dir_all(&self.dir).map_err(err)?;
                let tmp = tempfile::Builder::new()
                    .prefix(TEMP_PREFIX)
                    .tempfile_in(&self.dir)
                    .map_err(err)?;
                let (file, path) = tmp.into_parts();
                pretty_json(entry.raw.body.reader().map_err(err)?, file).map_err(err)?;
                let len = std::fs::metadata(&path).map_err(err)?.len();
                Ok(Arc::new(View::new(Body::File {
                    path: Arc::new(path),
                    len,
                })))
            })()
        };
        *slot = Some(result.clone());
        result
    }

    fn view(&self, entry: &Entry, pretty: bool) -> Result<Arc<View>, String> {
        if pretty {
            self.pretty(entry)
        } else {
            Ok(Arc::new(View {
                body: entry.raw.body.clone(),
                index: entry.raw.index.clone(),
            }))
        }
    }

    /// Bytes `[offset, offset + length)` of the raw or pretty view: text
    /// adjusted to UTF-8 character boundaries, base64 for binary bodies.
    pub fn read(
        &self,
        id: &str,
        offset: u64,
        length: u64,
        pretty: bool,
    ) -> Result<BodyChunk, String> {
        let e = self.get(id)?;
        let view = self.view(&e, pretty)?;
        let body = &view.body;
        let size = body.len();
        let length = length.min(MAX_READ as u64) as usize;
        let mut offset = offset.min(size);
        if !e.is_text {
            let bytes = body.read_range(offset, length).map_err(err)?;
            let n = bytes.len() as u64;
            return Ok(BodyChunk {
                data: B64.encode(&bytes),
                encoding: "base64".into(),
                offset,
                length: n,
                eof: offset + n >= size,
            });
        }
        // Start on a character boundary: skip continuation bytes (≤ 3).
        let lead = body.read_range(offset, 3).map_err(err)?;
        let skip = lead
            .iter()
            .take_while(|b| *b & 0b1100_0000 == 0b1000_0000)
            .count();
        offset += skip as u64;
        let mut bytes = body.read_range(offset, length).map_err(err)?;
        let at_eof = offset + bytes.len() as u64 >= size;
        if !at_eof {
            bytes.truncate(utf8_floor(&bytes));
        }
        let n = bytes.len() as u64;
        Ok(BodyChunk {
            data: String::from_utf8_lossy(&bytes).into_owned(),
            encoding: "utf8".into(),
            offset,
            length: n,
            eof: offset + n >= size,
        })
    }

    /// Lines of the raw or pretty view (index built on first use).
    pub fn lines(
        &self,
        id: &str,
        start: u64,
        count: u64,
        pretty: bool,
        max_line_bytes: usize,
    ) -> Result<BodyLines, String> {
        let e = self.get(id)?;
        let view = self.view(&e, pretty)?;
        let index = view.index()?;
        if !pretty {
            // Cache the raw index on the entry for next time.
            let _ = e.raw.index.set(index.clone());
        }
        let lines = read_lines(
            &view.body,
            &index,
            start.min(usize::MAX as u64) as usize,
            count.min(MAX_LINES as u64) as usize,
            max_line_bytes.max(4),
        )
        .map_err(err)?;
        Ok(BodyLines {
            start,
            offsets: lines.iter().map(|l| l.offset).collect(),
            byte_lengths: lines.iter().map(|l| l.byte_length).collect(),
            lines: lines.into_iter().map(|l| l.text).collect(),
            total_lines: index.len() as u64,
        })
    }

    pub fn search(
        &self,
        id: &str,
        query: &str,
        opts: &SearchOptions,
    ) -> Result<SearchResult, String> {
        let e = self.get(id)?;
        let view = self.view(&e, opts.pretty)?;
        search(view.body.reader().map_err(err)?, query, opts)
    }

    /// Copy the raw or pretty body to `path`.
    pub fn save(&self, id: &str, path: &Path, pretty: bool) -> Result<u64, String> {
        let e = self.get(id)?;
        let view = self.view(&e, pretty)?;
        view.body
            .copy_to(path)
            .map_err(|x| format!("{}: {x}", path.display()))
    }

    pub fn content_type(&self, id: &str) -> Result<Option<String>, String> {
        Ok(self.get(id)?.content_type.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body::{BodySink, SinkConfig};

    fn file_body(dir: &Path, data: &[u8]) -> Body {
        let mut s = BodySink::new(SinkConfig {
            inline_limit: 0,
            dir: Some(dir.to_path_buf()),
            ..Default::default()
        });
        s.push(data).unwrap();
        s.finish().unwrap().0
    }

    #[test]
    fn read_lines_search_save_release() {
        let dir = tempfile::tempdir().unwrap();
        let store = ResponseStore::new(dir.path(), u64::MAX, 100);
        let text = "{\"a\":\"é€\",\r\n\"b\":[1,2]}";
        let h = store.insert(
            file_body(dir.path(), text.as_bytes()),
            Some("application/json".into()),
            true,
            4,
        );
        assert!(h.is_text && h.is_json && h.preview_truncated);
        assert_eq!(h.stored, "file");

        // Offset 7 is inside "é" (bytes 6..8): moves to 8; the end is cut
        // before an incomplete "€" (bytes 8..11).
        let c = store.read(&h.id, 7, 2, false).unwrap();
        assert_eq!((c.offset, c.data.as_str(), c.length), (8, "", 0));
        let c = store.read(&h.id, 7, 3, false).unwrap();
        assert_eq!((c.offset, c.data.as_str()), (8, "€"));
        let c = store.read(&h.id, 6, 5, false).unwrap();
        assert_eq!((c.data.as_str(), c.length), ("é€", 5));
        let c = store.read(&h.id, 0, 1000, false).unwrap();
        assert!(c.eof && c.data == text);

        let l = store.lines(&h.id, 0, 10, false, 1024).unwrap();
        assert_eq!(l.lines, ["{\"a\":\"é€\",", "\"b\":[1,2]}"]);
        assert_eq!(l.offsets, [0, 15]);
        assert_eq!(l.byte_lengths, [13, 10]);
        assert_eq!(l.total_lines, 2);
        // Cut lines: 8 bytes ends inside "é€" → cut before it.
        let cut = store.lines(&h.id, 0, 1, false, 8).unwrap();
        assert_eq!(
            (cut.lines[0].as_str(), cut.byte_lengths[0]),
            ("{\"a\":\"é", 13)
        );
        // Reading the pretty view by bytes.
        let pc = store.read(&h.id, 0, 4, true).unwrap();
        assert_eq!(pc.data, "{\n  ");
        let p = store.lines(&h.id, 0, 100, true, 1024).unwrap();
        let v: serde_json::Value = serde_json::from_str(text).unwrap();
        assert_eq!(
            p.lines.join("\n"),
            serde_json::to_string_pretty(&v).unwrap()
        );

        let r = store
            .search(
                &h.id,
                "\\d",
                &SearchOptions {
                    regex: true,
                    pretty: true,
                    max_results: 10,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(r.matches.len(), 2);
        assert_eq!((r.matches[0].line, r.matches[0].column), (3, 4));

        let out = dir.path().join("saved.json");
        store.save(&h.id, &out, true).unwrap();
        assert_eq!(
            std::fs::read_to_string(&out).unwrap(),
            serde_json::to_string_pretty(&v).unwrap()
        );

        let temp_files = || {
            std::fs::read_dir(dir.path())
                .unwrap()
                .filter(|e| {
                    e.as_ref()
                        .unwrap()
                        .file_name()
                        .to_string_lossy()
                        .starts_with(TEMP_PREFIX)
                })
                .count()
        };
        assert_eq!(temp_files(), 2, "raw + pretty");
        store.release(&h.id);
        assert_eq!(temp_files(), 0);
        assert!(store.read(&h.id, 0, 1, false).is_err());
    }

    #[test]
    fn binary_pretty_errors_and_lru() {
        let dir = tempfile::tempdir().unwrap();
        let store = ResponseStore::new(dir.path(), 10, 3);
        let bin = store.insert(Body::from(vec![0u8, 159, 146, 150]), None, true, 1024);
        assert!(!bin.is_text);
        let c = store.read(&bin.id, 0, 10, false).unwrap();
        assert_eq!(
            (c.encoding.as_str(), c.data.as_str()),
            ("base64", "AJ+Slg==")
        );
        assert!(store.lines(&bin.id, 0, 1, true, 1024).is_err());
        let bad = store.insert(
            Body::from(b"{\"a\":".to_vec()),
            Some("application/json".into()),
            true,
            1024,
        );
        assert!(
            store
                .lines(&bad.id, 0, 1, true, 1024)
                .unwrap_err()
                .contains("not valid JSON")
        );

        // Count cap: 3 entries.
        let a = store.insert(Body::from(b"x".to_vec()), None, true, 1024);
        assert_eq!(store.len(), 3);
        assert!(store.contains(&bin.id));
        // Touching `bad` makes it recent; the next file body evicts `a` and more.
        store.read(&bad.id, 0, 1, false).unwrap();
        let big = store.insert(file_body(dir.path(), &[b'y'; 20]), None, true, 1024);
        assert!(store.contains(&big.id), "the newest entry is never evicted");
        assert_eq!(
            store.len(),
            1,
            "file bytes over the cap evict older entries"
        );
        let _ = a;
        store.clear();
        assert!(store.is_empty());

        // Orphans from a previous run are removed.
        let orphan = dir.path().join(format!("{TEMP_PREFIX}old"));
        std::fs::write(&orphan, b"x").unwrap();
        std::fs::write(dir.path().join("keep.txt"), b"x").unwrap();
        ResponseStore::clean_orphans(dir.path());
        assert!(!orphan.exists() && dir.path().join("keep.txt").exists());
    }
}
