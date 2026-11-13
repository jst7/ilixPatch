//! Importer commands (Postman D31, OpenAPI / Swagger D53): preview parsed
//! sources (files, folders, downloaded specs), then apply a plan.

use std::fs;
use std::path::PathBuf;

use ilixpatch_importer::{
    self as importer, ApplyContext, ImportPlan, ImportPreview, ImportReport, MAX_FILE,
    ParseOptions, Parsed, Sessions,
};
use tauri::{AppHandle, Manager, State};

use crate::network_commands::SharedEngine;
use crate::registry::Registry;

type CmdResult<T> = Result<T, String>;

/// Parsed sets between preview and apply (expire after 30 min, removed on apply).
static SESSIONS: Sessions = Sessions::new();

/// Parse what the user chose: Postman exports (files, `.zip` dumps, folders)
/// and OpenAPI / Swagger specs (files, folders, or `urls` downloaded with the
/// app's network settings).
#[tauri::command]
pub async fn import_preview(
    shared: State<'_, SharedEngine>,
    paths: Vec<String>,
    urls: Vec<String>,
    options: ParseOptions,
) -> CmdResult<ImportPreview> {
    if paths.is_empty() && urls.is_empty() {
        return Err("nothing to import".into());
    }
    let engine = shared.get();
    let mut downloads = Vec::new();
    for url in urls {
        let result = engine.download(&url, MAX_FILE).await;
        downloads.push((url, result));
    }
    // Large specs take a moment to parse: keep it off the async runtime.
    let parsed = tauri::async_runtime::spawn_blocking(move || {
        let mut parsed = Parsed::new(options);
        for p in &paths {
            parsed.add_path(&PathBuf::from(p));
        }
        for (url, result) in downloads {
            match result {
                Ok(bytes) => parsed.add_download(&url, &bytes),
                Err(e) => parsed.add_error(&url, e),
            }
        }
        parsed
    })
    .await
    .map_err(|e| e.to_string())?;
    let session = SESSIONS.insert(parsed);
    let parsed = SESSIONS.get(&session).ok_or("import session lost")?;
    Ok(parsed.preview(session))
}

/// Write the chosen items into a registered project.
#[tauri::command(async)]
pub fn import_apply(
    app: AppHandle,
    registry: State<'_, Registry>,
    plan: ImportPlan,
) -> CmdResult<ImportReport> {
    let parsed = SESSIONS
        .get(&plan.session)
        .ok_or("this import session has expired; choose the files again")?;
    let root = fs::canonicalize(&plan.project_path)
        .map_err(|e| format!("{}: {e}", plan.project_path.display()))?;
    let registered = registry
        .paths()?
        .iter()
        .filter_map(|p| fs::canonicalize(p).ok())
        .any(|p| p == root);
    if !registered {
        return Err(format!("{}: not a registered project", root.display()));
    }
    let ctx = ApplyContext {
        global_dir: app.path().app_config_dir().ok(),
    };
    let report = importer::apply(&parsed, &plan, &ctx).map_err(|e| e.to_string())?;
    SESSIONS.remove(&plan.session);
    Ok(report)
}
