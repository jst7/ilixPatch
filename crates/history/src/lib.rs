//! Local request/response history for ilixPatch (decision D22).
//!
//! Every request sent from the app is recorded in a SQLite database that lives
//! in the app data directory — never in a project repo. Listing only reads a
//! small summary table; the heavy payload (request, response body, tests, logs)
//! lives in a separate table and is loaded only by [`HistoryStore::get`].
//!
//! Retention ([`HistorySettings`]) bounds the database by age and by size; see
//! [`HistoryStore::prune`].

mod settings;
mod store;

use ilixpatch_core::{HttpMethod, Request};
use serde::{Deserialize, Serialize};

pub use settings::HistorySettings;
pub use store::HistoryStore;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("history database: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("history data: {0}")]
    Json(#[from] serde_json::Error),
    #[error("history settings: {0}")]
    Io(#[from] std::io::Error),
    #[error("history settings: {0}")]
    TomlRead(#[from] toml::de::Error),
    #[error("history settings: {0}")]
    TomlWrite(#[from] toml::ser::Error),
    #[error("history entry {0} not found")]
    NotFound(i64),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Where a history entry came from (optional; ad-hoc requests have none).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryContext {
    #[serde(default)]
    pub project_path: Option<String>,
    #[serde(default)]
    pub request_path: Option<String>,
    #[serde(default)]
    pub environment: Option<String>,
}

/// One row of the history list (no payload).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryEntrySummary {
    pub id: i64,
    /// Unix milliseconds.
    pub timestamp: i64,
    pub name: String,
    pub method: HttpMethod,
    /// Resolved URL that was actually sent.
    pub url: String,
    pub status: Option<u16>,
    pub elapsed_ms: f64,
    pub size_bytes: u64,
    /// Network error, if the request failed.
    pub error: Option<String>,
    pub project_path: Option<String>,
    pub request_path: Option<String>,
    pub environment: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryResponse {
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    /// Empty when `save_response_body` is false.
    pub body: String,
    pub body_truncated: bool,
}

/// A full history entry, as returned by `history_get`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryEntry {
    #[serde(flatten)]
    pub summary: HistoryEntrySummary,
    /// The request as edited (unresolved `{{vars}}`) — restored into a new tab.
    pub request: Request,
    /// The request after variable resolution and pre-request script.
    pub resolved_request: Option<Request>,
    pub response: Option<HistoryResponse>,
    /// `TestResult[]` as produced by `ilixpatch-scripting` (kept as raw JSON so
    /// this crate does not depend on the scripting engine).
    pub tests: serde_json::Value,
    pub logs: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryStats {
    pub entries: u64,
    pub size_bytes: u64,
    /// Unix ms of the oldest entry.
    pub oldest: Option<i64>,
}

/// Everything needed to record one sent request. See [`HistoryStore::record`].
#[derive(Debug, Clone)]
pub struct NewEntry {
    /// Unix milliseconds; use [`now_ms`].
    pub timestamp_ms: i64,
    pub name: String,
    pub method: HttpMethod,
    /// Resolved URL that was actually sent.
    pub url: String,
    /// `None` when the request failed before a response arrived.
    pub status: Option<u16>,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    /// Response body as text; truncated to `max_body_kb` on record.
    pub body: Option<String>,
    pub elapsed_ms: f64,
    pub size_bytes: u64,
    pub error: Option<String>,
    /// The request as edited (unresolved).
    pub request: Request,
    pub resolved_request: Option<Request>,
    /// Serialized `Vec<TestResult>` (`serde_json::to_value(&tests)`), or `Value::Null` for none.
    pub tests: serde_json::Value,
    pub logs: Vec<String>,
    pub context: HistoryContext,
}

/// Current time in Unix milliseconds.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Truncate `s` to at most `max` bytes on a UTF-8 char boundary.
/// Returns `true` if anything was cut.
pub(crate) fn truncate_utf8(s: &mut String, max: usize) -> bool {
    if s.len() <= max {
        return false;
    }
    let mut cut = max;
    while !s.is_char_boundary(cut) {
        cut -= 1;
    }
    s.truncate(cut);
    true
}
