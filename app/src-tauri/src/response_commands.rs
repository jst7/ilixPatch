//! Large responses (D41): the bodies kept for the UI and the commands that
//! read them in chunks. Bodies above `inline_body_kb` live in temp files under
//! `<app_cache_dir>/bodies`, which is wiped at startup and on exit.

use std::path::{Path, PathBuf};

use ilixpatch_engine::bodystore::{
    BodyChunk, BodyHandle, BodyLines, ResponseStore, SearchOptions, SearchResult, utf8_floor,
};
use ilixpatch_engine::{Engine, Response};
use tauri::{Manager, State};

/// Total size of spilled bodies kept before the least recently used go.
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
/// Most bodies kept (one per open response, plus some slack).
const MAX_ENTRIES: usize = 64;
/// Bytes of a large body sent in `SendResult.body` as a preview.
pub const PREVIEW_BYTES: usize = 64 * 1024;

/// Where the engine spills bodies.
pub fn spill_dir(app: &tauri::App) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let dir = app.path().app_cache_dir()?.join("bodies");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn init(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let dir = spill_dir(app)?;
    ResponseStore::clean_orphans(&dir);
    app.manage(ResponseStore::new(dir, MAX_FILE_BYTES, MAX_ENTRIES));
    Ok(())
}

/// Keep the body of `resp` and return its handle plus the text for
/// `SendResult.body`: the whole body when small, else a preview.
pub fn keep(store: &ResponseStore, engine: &Engine, resp: &Response) -> (BodyHandle, String) {
    let inline = engine.settings().inline_body_kb.saturating_mul(1024);
    let handle = store.insert(
        resp.body.clone(),
        resp.content_type().map(str::to_owned),
        resp.body_complete,
        inline,
    );
    let limit = if handle.preview_truncated {
        PREVIEW_BYTES
    } else {
        usize::try_from(inline).unwrap_or(usize::MAX)
    };
    let text = match resp.body.to_bytes_limited(limit) {
        Ok((mut bytes, cut)) => {
            if cut {
                bytes.truncate(utf8_floor(&bytes));
            }
            String::from_utf8_lossy(&bytes).into_owned()
        }
        Err(e) => {
            eprintln!("response body: {e}");
            String::new()
        }
    };
    (handle, text)
}

#[tauri::command(async)]
pub fn response_read(
    store: State<'_, ResponseStore>,
    id: String,
    offset: u64,
    length: u64,
    pretty: bool,
) -> Result<BodyChunk, String> {
    store.read(&id, offset, length, pretty)
}

#[tauri::command(async)]
pub fn response_lines(
    store: State<'_, ResponseStore>,
    id: String,
    start: u64,
    count: u64,
    pretty: bool,
    max_line_bytes: Option<usize>,
) -> Result<BodyLines, String> {
    store.lines(
        &id,
        start,
        count,
        pretty,
        max_line_bytes.unwrap_or(64 * 1024),
    )
}

#[tauri::command(async)]
pub fn response_search(
    store: State<'_, ResponseStore>,
    id: String,
    query: String,
    options: SearchOptions,
) -> Result<SearchResult, String> {
    store.search(&id, &query, &options)
}

#[tauri::command(async)]
pub fn response_save(
    store: State<'_, ResponseStore>,
    id: String,
    path: String,
    pretty: bool,
) -> Result<(), String> {
    store.save(&id, Path::new(&path), pretty).map(|_| ())
}

#[tauri::command(async)]
pub fn response_release(store: State<'_, ResponseStore>, id: String) {
    store.release(&id);
}
