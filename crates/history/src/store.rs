//! SQLite-backed history store.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use ilixpatch_core::{HttpMethod, Request};
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

use crate::{
    HistoryEntry, HistoryEntrySummary, HistoryResponse, HistorySettings, HistoryStats, NewEntry,
    Result, now_ms, truncate_utf8,
};

/// Bump when the schema changes and add a migration in [`migrate`].
const SCHEMA_VERSION: i64 = 1;

/// Bodies larger than this are deflated (if that makes them smaller).
const COMPRESS_THRESHOLD: usize = 4 * 1024;

/// Run retention after this many inserts...
const PRUNE_EVERY_INSERTS: u64 = 50;
/// ...or after roughly this many payload bytes were inserted, whichever comes first.
const PRUNE_EVERY_BYTES: u64 = 4 * 1024 * 1024;

const DAY_MS: i64 = 86_400_000;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS entries (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp    INTEGER NOT NULL,          -- unix ms
    name         TEXT    NOT NULL,
    method       TEXT    NOT NULL,
    url          TEXT    NOT NULL,          -- resolved URL
    status       INTEGER,                   -- NULL on network error
    elapsed_ms   REAL    NOT NULL,
    size_bytes   INTEGER NOT NULL,
    error        TEXT,
    project_path TEXT,
    request_path TEXT,
    environment  TEXT
);
CREATE INDEX IF NOT EXISTS entries_timestamp ON entries(timestamp);

CREATE TABLE IF NOT EXISTS payloads (
    entry_id         INTEGER PRIMARY KEY REFERENCES entries(id) ON DELETE CASCADE,
    request          TEXT    NOT NULL,      -- JSON Request (as edited)
    resolved_request TEXT,                  -- JSON Request (resolved)
    response_meta    TEXT,                  -- JSON {status, status_text, headers}; NULL if no response
    body             BLOB,                  -- UTF-8 text, deflated when body_compressed = 1
    body_compressed  INTEGER NOT NULL DEFAULT 0,
    body_truncated   INTEGER NOT NULL DEFAULT 0,
    tests            TEXT    NOT NULL,      -- JSON TestResult[]
    logs             TEXT    NOT NULL       -- JSON string[]
);
";

const SUMMARY_COLUMNS: &str = "id, timestamp, name, method, url, status, elapsed_ms, size_bytes, \
     error, project_path, request_path, environment";

#[derive(Serialize, Deserialize)]
struct ResponseMeta {
    status: u16,
    status_text: String,
    headers: Vec<(String, String)>,
}

/// Thread-safe handle to the history database. Cheap to clone (shared `Arc`).
#[derive(Clone)]
pub struct HistoryStore {
    inner: Arc<Inner>,
}

struct Inner {
    conn: Mutex<Connection>,
    settings: Mutex<HistorySettings>,
    /// `None` for in-memory stores: settings are not persisted.
    settings_path: Option<PathBuf>,
    inserts_since_prune: AtomicU64,
    bytes_since_prune: AtomicU64,
}

impl HistoryStore {
    /// Open (or create) the database at `db_path` and load settings from
    /// `settings_path` (`history.toml`; missing or unreadable → defaults).
    /// Does not prune; call [`prune`](Self::prune) (e.g. in a background thread).
    pub fn open(db_path: &Path, settings_path: &Path) -> Result<Self> {
        if let Some(dir) = db_path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(db_path)?;
        let settings = HistorySettings::load(settings_path).unwrap_or_default();
        Self::init(conn, settings, Some(settings_path.to_path_buf()))
    }

    /// A throwaway store (tests, or a fallback when the database can't be opened).
    pub fn open_in_memory(settings: HistorySettings) -> Result<Self> {
        Self::init(Connection::open_in_memory()?, settings, None)
    }

    fn init(conn: Connection, settings: HistorySettings, path: Option<PathBuf>) -> Result<Self> {
        // auto_vacuum must be set before the first table is created.
        conn.execute_batch("PRAGMA auto_vacuum = INCREMENTAL;")?;
        let _mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
        conn.execute_batch(
            "PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;",
        )?;
        migrate(&conn)?;
        Ok(Self {
            inner: Arc::new(Inner {
                conn: Mutex::new(conn),
                settings: Mutex::new(settings.normalized()),
                settings_path: path,
                inserts_since_prune: AtomicU64::new(0),
                bytes_since_prune: AtomicU64::new(0),
            }),
        })
    }

    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.inner.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn settings(&self) -> HistorySettings {
        self.inner
            .settings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Persist new settings and apply retention immediately.
    pub fn set_settings(&self, settings: HistorySettings) -> Result<HistoryStats> {
        let settings = settings.normalized();
        if let Some(path) = &self.inner.settings_path {
            settings.save(path)?;
        }
        *self
            .inner
            .settings
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = settings;
        self.prune()
    }

    /// Record one sent request. Respects `enabled`, `save_response_body` and
    /// `max_body_kb` (truncating on a UTF-8 boundary). Returns the new id, or
    /// `None` when history is disabled. Retention runs every few inserts.
    ///
    /// Blocking (SQLite I/O); from async code run it off the executor:
    ///
    /// ```no_run
    /// # use ilixpatch_history::*;
    /// # use ilixpatch_core::{HttpMethod, Request};
    /// # fn demo(store: &HistoryStore, request: Request) {
    /// let entry = NewEntry {
    ///     timestamp_ms: now_ms(),
    ///     name: request.name.clone(),
    ///     method: request.method,
    ///     url: "https://api.example.com/users".into(),
    ///     status: Some(200),
    ///     status_text: "OK".into(),
    ///     headers: vec![("content-type".into(), "application/json".into())],
    ///     body: Some("[]".into()),
    ///     elapsed_ms: 42.0,
    ///     size_bytes: 2,
    ///     error: None,
    ///     request: request.clone(),
    ///     resolved_request: Some(request),
    ///     tests: serde_json::Value::Null, // or serde_json::to_value(&test_results)?
    ///     logs: vec![],
    ///     context: HistoryContext::default(),
    /// };
    /// let store = store.clone();
    /// // In Tauri: tauri::async_runtime::spawn_blocking(move || store.record(entry));
    /// std::thread::spawn(move || store.record(entry));
    /// # }
    /// ```
    pub fn record(&self, entry: NewEntry) -> Result<Option<i64>> {
        let settings = self.settings();
        if !settings.enabled {
            return Ok(None);
        }

        // Prepare the payload before taking the lock.
        let (body, body_compressed, body_truncated) = match entry.body {
            Some(mut text) if settings.save_response_body => {
                let truncated = truncate_utf8(&mut text, settings.max_body_bytes());
                let (bytes, compressed) = compress(text.into_bytes());
                (Some(bytes), compressed, truncated)
            }
            _ => (None, false, false),
        };
        let request = serde_json::to_string(&entry.request)?;
        let resolved = entry
            .resolved_request
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let meta = entry
            .status
            .map(|status| {
                serde_json::to_string(&ResponseMeta {
                    status,
                    status_text: entry.status_text,
                    headers: entry.headers,
                })
            })
            .transpose()?;
        let tests = match entry.tests {
            serde_json::Value::Null => "[]".to_string(),
            v => serde_json::to_string(&v)?,
        };
        let logs = serde_json::to_string(&entry.logs)?;
        let approx_bytes = (request.len()
            + resolved.as_ref().map_or(0, String::len)
            + meta.as_ref().map_or(0, String::len)
            + body.as_ref().map_or(0, Vec::len)
            + tests.len()
            + logs.len()
            + entry.url.len()
            + 256) as u64;

        let id = {
            let mut conn = self.conn();
            let tx = conn.transaction()?;
            tx.execute(
                "INSERT INTO entries (timestamp, name, method, url, status, elapsed_ms, size_bytes,
                                      error, project_path, request_path, environment)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    entry.timestamp_ms,
                    entry.name,
                    entry.method.as_str(),
                    entry.url,
                    entry.status,
                    entry.elapsed_ms,
                    entry.size_bytes as i64,
                    entry.error,
                    entry.context.project_path,
                    entry.context.request_path,
                    entry.context.environment,
                ],
            )?;
            let id = tx.last_insert_rowid();
            tx.execute(
                "INSERT INTO payloads (entry_id, request, resolved_request, response_meta, body,
                                       body_compressed, body_truncated, tests, logs)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    id,
                    request,
                    resolved,
                    meta,
                    body,
                    body_compressed,
                    body_truncated,
                    tests,
                    logs
                ],
            )?;
            tx.commit()?;
            id
        };

        let inserts = self
            .inner
            .inserts_since_prune
            .fetch_add(1, Ordering::Relaxed)
            + 1;
        let bytes = self
            .inner
            .bytes_since_prune
            .fetch_add(approx_bytes, Ordering::Relaxed)
            + approx_bytes;
        if inserts >= PRUNE_EVERY_INSERTS || bytes >= PRUNE_EVERY_BYTES {
            self.prune()?;
        }
        Ok(Some(id))
    }

    /// Newest first. `query` filters case-insensitively on url, name and method;
    /// `before_id` continues a previous page (keyset pagination).
    pub fn list(
        &self,
        query: Option<&str>,
        before_id: Option<i64>,
        limit: u32,
    ) -> Result<Vec<HistoryEntrySummary>> {
        let pattern = query
            .map(str::trim)
            .filter(|q| !q.is_empty())
            .map(|q| format!("%{}%", escape_like(q)));
        let limit = limit.clamp(1, 1000);
        let conn = self.conn();
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT {SUMMARY_COLUMNS} FROM entries
             WHERE (?1 IS NULL OR id < ?1)
               AND (?2 IS NULL OR url LIKE ?2 ESCAPE '\\'
                               OR name LIKE ?2 ESCAPE '\\'
                               OR method LIKE ?2 ESCAPE '\\')
             ORDER BY id DESC
             LIMIT ?3"
        ))?;
        let rows = stmt.query_map(params![before_id, pattern, limit], summary_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn get(&self, id: i64) -> Result<HistoryEntry> {
        let conn = self.conn();
        let row = conn
            .query_row(
                &format!(
                    "SELECT {SUMMARY_COLUMNS}, p.request, p.resolved_request, p.response_meta,
                            p.body, p.body_compressed, p.body_truncated, p.tests, p.logs
                     FROM entries e JOIN payloads p ON p.entry_id = e.id
                     WHERE e.id = ?1"
                ),
                [id],
                |r| {
                    Ok((
                        summary_from_row(r)?,
                        r.get::<_, String>(12)?,
                        r.get::<_, Option<String>>(13)?,
                        r.get::<_, Option<String>>(14)?,
                        r.get::<_, Option<Vec<u8>>>(15)?,
                        r.get::<_, bool>(16)?,
                        r.get::<_, bool>(17)?,
                        r.get::<_, String>(18)?,
                        r.get::<_, String>(19)?,
                    ))
                },
            )
            .optional()?
            .ok_or(crate::Error::NotFound(id))?;
        drop(conn);
        let (summary, request, resolved, meta, body, compressed, truncated, tests, logs) = row;

        let response = meta
            .map(|m| -> Result<HistoryResponse> {
                let m: ResponseMeta = serde_json::from_str(&m)?;
                let body = body
                    .map(|b| decompress(b, compressed))
                    .map(|b| String::from_utf8_lossy(&b).into_owned())
                    .unwrap_or_default();
                Ok(HistoryResponse {
                    status: m.status,
                    status_text: m.status_text,
                    headers: m.headers,
                    body,
                    body_truncated: truncated,
                })
            })
            .transpose()?;
        Ok(HistoryEntry {
            summary,
            request: serde_json::from_str::<Request>(&request)?,
            resolved_request: resolved
                .map(|r| serde_json::from_str::<Request>(&r))
                .transpose()?,
            response,
            tests: serde_json::from_str(&tests)?,
            logs: serde_json::from_str(&logs)?,
        })
    }

    pub fn delete(&self, ids: &[i64]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached("DELETE FROM entries WHERE id = ?1")?;
            for id in ids {
                stmt.execute([id])?;
            }
        }
        tx.commit()?;
        reclaim(&conn)
    }

    pub fn clear(&self) -> Result<()> {
        let conn = self.conn();
        conn.execute_batch("BEGIN; DELETE FROM payloads; DELETE FROM entries; COMMIT;")?;
        reclaim(&conn)
    }

    pub fn stats(&self) -> Result<HistoryStats> {
        stats(&self.conn())
    }

    /// Apply retention: drop entries older than `max_days`, then the oldest
    /// entries while the database is larger than `max_size_mb`, then return
    /// freed pages to the OS (`incremental_vacuum`).
    pub fn prune(&self) -> Result<HistoryStats> {
        self.prune_at(now_ms())
    }

    /// [`prune`](Self::prune) with an explicit "now" (for tests).
    pub fn prune_at(&self, now_ms: i64) -> Result<HistoryStats> {
        let settings = self.settings();
        self.inner.inserts_since_prune.store(0, Ordering::Relaxed);
        self.inner.bytes_since_prune.store(0, Ordering::Relaxed);
        let conn = self.conn();
        let mut deleted = 0usize;

        if let Some(days) = settings.max_days {
            let cutoff = now_ms.saturating_sub(i64::from(days).saturating_mul(DAY_MS));
            deleted += conn.execute("DELETE FROM entries WHERE timestamp < ?1", [cutoff])?;
        }

        if let Some(max) = settings.max_size_bytes() {
            loop {
                let used = used_bytes(&conn)?;
                if used <= max {
                    break;
                }
                let count = int(&conn, "SELECT COUNT(*) FROM entries")?;
                if count == 0 {
                    break;
                }
                // Estimate how many of the oldest entries to drop; refine on the next pass.
                let avg = (used / count).max(1);
                let batch = ((used - max).div_ceil(avg)).clamp(1, 500);
                let n = conn.execute(
                    "DELETE FROM entries WHERE id IN
                       (SELECT id FROM entries ORDER BY timestamp, id LIMIT ?1)",
                    [batch as i64],
                )?;
                if n == 0 {
                    break;
                }
                deleted += n;
            }
        }

        if deleted > 0 {
            reclaim(&conn)?;
        }
        stats(&conn)
    }
}

fn migrate(conn: &Connection) -> Result<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version < SCHEMA_VERSION {
        conn.execute_batch(SCHEMA)?;
        conn.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION};"))?;
    }
    Ok(())
}

fn summary_from_row(r: &Row<'_>) -> rusqlite::Result<HistoryEntrySummary> {
    let method: String = r.get(3)?;
    Ok(HistoryEntrySummary {
        id: r.get(0)?,
        timestamp: r.get(1)?,
        name: r.get(2)?,
        method: parse_method(&method),
        url: r.get(4)?,
        status: r.get(5)?,
        elapsed_ms: r.get(6)?,
        size_bytes: r.get::<_, i64>(7)?.max(0) as u64,
        error: r.get(8)?,
        project_path: r.get(9)?,
        request_path: r.get(10)?,
        environment: r.get(11)?,
    })
}

fn parse_method(s: &str) -> HttpMethod {
    serde_json::from_value(serde_json::Value::String(s.to_string())).unwrap_or_default()
}

fn escape_like(q: &str) -> String {
    let mut out = String::with_capacity(q.len());
    for c in q.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Bytes actually used by live data (excludes free pages).
fn used_bytes(conn: &Connection) -> Result<u64> {
    let page_count = int(conn, "PRAGMA page_count")?;
    let freelist = int(conn, "PRAGMA freelist_count")?;
    let page_size = int(conn, "PRAGMA page_size")?;
    Ok(page_count.saturating_sub(freelist) * page_size)
}

/// A single non-negative integer result.
fn int(conn: &Connection, sql: &str) -> Result<u64> {
    let v: i64 = conn.query_row(sql, [], |r| r.get(0))?;
    Ok(v.max(0) as u64)
}

fn stats(conn: &Connection) -> Result<HistoryStats> {
    let (entries, oldest): (i64, Option<i64>) =
        conn.query_row("SELECT COUNT(*), MIN(timestamp) FROM entries", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?;
    Ok(HistoryStats {
        entries: entries.max(0) as u64,
        size_bytes: used_bytes(conn)?,
        oldest,
    })
}

/// Give free pages back to the OS and shrink the WAL file.
fn reclaim(conn: &Connection) -> Result<()> {
    // Frees one page per step, so drain it fully.
    let mut stmt = conn.prepare("PRAGMA incremental_vacuum")?;
    let mut rows = stmt.query([])?;
    while rows.next()?.is_some() {}
    drop(rows);
    drop(stmt);
    // Returns a status row; failures (e.g. a concurrent reader) are harmless.
    let _ = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
    Ok(())
}

fn compress(bytes: Vec<u8>) -> (Vec<u8>, bool) {
    if bytes.len() < COMPRESS_THRESHOLD {
        return (bytes, false);
    }
    let packed = miniz_oxide::deflate::compress_to_vec(&bytes, 1);
    if packed.len() < bytes.len() {
        (packed, true)
    } else {
        (bytes, false)
    }
}

fn decompress(bytes: Vec<u8>, compressed: bool) -> Vec<u8> {
    if !compressed {
        return bytes;
    }
    miniz_oxide::inflate::decompress_to_vec(&bytes).unwrap_or_default()
}

#[cfg(test)]
mod tests;
