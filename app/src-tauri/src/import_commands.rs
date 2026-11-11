//! Postman importer commands (D31): preview parsed exports, then apply a plan.

use std::fs;
use std::path::PathBuf;

use ilixpatch_importer::{
    self as importer, ApplyContext, ImportPlan, ImportPreview, ImportReport, Sessions,
};
use tauri::{AppHandle, Manager, State};

use crate::registry::Registry;

type CmdResult<T> = Result<T, String>;

/// Parsed sets between preview and apply (expire after 30 min, removed on apply).
static SESSIONS: Sessions = Sessions::new();

/// Parse Postman exports chosen by the user (files, `.zip` dumps or folders).
#[tauri::command(async)]
pub fn import_postman_preview(paths: Vec<String>) -> CmdResult<ImportPreview> {
    if paths.is_empty() {
        return Err("no files to import".into());
    }
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    let session = SESSIONS.insert(importer::parse_paths(&paths));
    let parsed = SESSIONS.get(&session).ok_or("import session lost")?;
    Ok(parsed.preview(session))
}

/// Write the chosen items into a registered project.
#[tauri::command(async)]
pub fn import_postman_apply(
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
