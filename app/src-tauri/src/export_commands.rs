//! Postman export commands (D58): a whole project into a folder, or one
//! collection into a file. Only registered projects can be exported.

use std::fs;
use std::path::PathBuf;

use ilixpatch_core::project;
use ilixpatch_importer::{ExportOptions, ExportReport, export_collection_file, export_postman};
use tauri::State;

use crate::registry::Registry;

type CmdResult<T> = Result<T, String>;

fn registered_roots(registry: &Registry) -> CmdResult<Vec<PathBuf>> {
    Ok(registry
        .paths()?
        .iter()
        .filter_map(|p| fs::canonicalize(p).ok())
        .collect())
}

/// Every collection and the project environments into `out_dir`
/// (`<name>.postman_collection.json`, `<env>.postman_environment.json`).
#[tauri::command(async)]
pub fn export_postman_project(
    registry: State<'_, Registry>,
    project_path: String,
    out_dir: String,
    include_secrets: bool,
) -> CmdResult<ExportReport> {
    let root = fs::canonicalize(&project_path).map_err(|e| format!("{project_path}: {e}"))?;
    if !registered_roots(&registry)?.contains(&root) {
        return Err(format!("{}: not a registered project", root.display()));
    }
    export_postman(
        &root,
        &PathBuf::from(out_dir),
        &ExportOptions { include_secrets },
    )
    .map_err(|e| e.to_string())
}

/// One collection into `file` (Postman Collection v2.1).
#[tauri::command(async)]
pub fn export_postman_collection(
    registry: State<'_, Registry>,
    collection_path: String,
    file: String,
    include_secrets: bool,
) -> CmdResult<ExportReport> {
    let dir =
        project::resolve_inside(&registry.paths()?, &collection_path).map_err(|e| e.to_string())?;
    export_collection_file(
        &dir,
        &PathBuf::from(file),
        &ExportOptions { include_secrets },
    )
    .map_err(|e| e.to_string())
}
