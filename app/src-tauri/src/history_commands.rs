//! Request history commands (D22). Contract: History section of `app/src/lib/api.ts`.

use ilixpatch_history::{
    HistoryEntry, HistoryEntrySummary, HistorySettings, HistoryStats, HistoryStore, NewEntry,
};
use tauri::{App, Manager, State};

type CmdResult<T> = Result<T, String>;

fn err(e: impl ToString) -> String {
    e.to_string()
}

/// Open `<app_data_dir>/history.sqlite3` (settings: `<app_config_dir>/history.toml`),
/// register it as managed state and prune old entries in a background thread.
/// Falls back to an in-memory store if the database can't be opened.
pub fn init(app: &App) -> Result<(), Box<dyn std::error::Error>> {
    let db = app.path().app_data_dir()?.join("history.sqlite3");
    let settings = app.path().app_config_dir()?.join("history.toml");
    let store = HistoryStore::open(&db, &settings).or_else(|e| {
        eprintln!(
            "history: cannot open {}: {e}; using in-memory history",
            db.display()
        );
        HistoryStore::open_in_memory(HistorySettings::load(&settings).unwrap_or_default())
    })?;
    app.manage(store.clone());
    std::thread::spawn(move || {
        if let Err(e) = store.prune() {
            eprintln!("history: prune failed: {e}");
        }
    });
    Ok(())
}

/// Record a sent request without blocking the caller (for `send_request`).
/// Errors are logged, never surfaced to the UI.
#[allow(dead_code)] // wired into send_request by the lead
pub fn record_in_background(store: &HistoryStore, entry: NewEntry) {
    let store = store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(e) = store.record(entry) {
            eprintln!("history: record failed: {e}");
        }
    });
}

#[tauri::command(async)]
pub fn history_list(
    store: State<'_, HistoryStore>,
    query: Option<String>,
    before_id: Option<i64>,
    limit: u32,
) -> CmdResult<Vec<HistoryEntrySummary>> {
    store.list(query.as_deref(), before_id, limit).map_err(err)
}

#[tauri::command(async)]
pub fn history_get(store: State<'_, HistoryStore>, id: i64) -> CmdResult<HistoryEntry> {
    store.get(id).map_err(err)
}

#[tauri::command(async)]
pub fn history_delete(store: State<'_, HistoryStore>, ids: Vec<i64>) -> CmdResult<()> {
    store.delete(&ids).map_err(err)
}

#[tauri::command(async)]
pub fn history_clear(store: State<'_, HistoryStore>) -> CmdResult<()> {
    store.clear().map_err(err)
}

#[tauri::command(async)]
pub fn history_stats(store: State<'_, HistoryStore>) -> CmdResult<HistoryStats> {
    store.stats().map_err(err)
}

#[tauri::command(async)]
pub fn history_get_settings(store: State<'_, HistoryStore>) -> CmdResult<HistorySettings> {
    Ok(store.settings())
}

/// Save settings and prune immediately; returns the resulting stats.
#[tauri::command(async)]
pub fn history_set_settings(
    store: State<'_, HistoryStore>,
    settings: HistorySettings,
) -> CmdResult<HistoryStats> {
    store.set_settings(settings).map_err(err)
}
