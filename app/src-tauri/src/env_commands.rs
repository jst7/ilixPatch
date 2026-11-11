//! Environments per project and global (D28), plus user-level globals.
//! Format and IO live in `ilixpatch_core::environments`; this module only picks
//! the directories and enforces that project paths are registered projects.

use std::fs;
use std::path::PathBuf;

use ilixpatch_core::environments::{
    EnvStore, EnvVariable, EnvironmentDoc, copy_environment, load_globals, save_globals,
};
use ilixpatch_runner::EnvScope;
use serde::Deserialize;
use tauri::{AppHandle, Manager, State};

use crate::registry::Registry;

type CmdResult<T> = Result<T, String>;

fn err(e: impl ToString) -> String {
    e.to_string()
}

/// `EnvRef` in api.ts.
#[derive(Deserialize)]
#[serde(tag = "scope", rename_all = "lowercase")]
pub enum EnvRef {
    Project { project_path: String, name: String },
    Global { name: String },
}

/// The app config dir: global environments and `globals.toml` live here.
pub fn config_dir(app: &AppHandle) -> CmdResult<PathBuf> {
    app.path().app_config_dir().map_err(err)
}

/// The canonical root of a registered project.
fn registered_project(registry: &Registry, project_path: Option<&str>) -> CmdResult<PathBuf> {
    let path = project_path.ok_or("project_path is required for project environments")?;
    let root = fs::canonicalize(path).map_err(|e| format!("{path}: {e}"))?;
    if !registry.paths()?.contains(&root) {
        return Err(format!("{path}: not a registered project"));
    }
    Ok(root)
}

fn store(
    app: &AppHandle,
    registry: &Registry,
    scope: EnvScope,
    project_path: Option<&str>,
) -> CmdResult<EnvStore> {
    Ok(match scope {
        EnvScope::Project => EnvStore::project(registered_project(registry, project_path)?),
        EnvScope::Global => EnvStore::global(config_dir(app)?),
    })
}

#[tauri::command(async)]
pub fn env_list(
    app: AppHandle,
    registry: State<'_, Registry>,
    scope: EnvScope,
    project_path: Option<String>,
) -> CmdResult<Vec<EnvironmentDoc>> {
    store(&app, &registry, scope, project_path.as_deref())?
        .list()
        .map_err(err)
}

/// Create or update; `original_name` renames (moves the files).
#[tauri::command(async)]
pub fn env_save(
    app: AppHandle,
    registry: State<'_, Registry>,
    scope: EnvScope,
    project_path: Option<String>,
    env: EnvironmentDoc,
    original_name: Option<String>,
) -> CmdResult<EnvironmentDoc> {
    store(&app, &registry, scope, project_path.as_deref())?
        .save(&env, original_name.as_deref())
        .map_err(err)
}

#[tauri::command(async)]
pub fn env_delete(
    app: AppHandle,
    registry: State<'_, Registry>,
    scope: EnvScope,
    project_path: Option<String>,
    name: String,
) -> CmdResult<()> {
    store(&app, &registry, scope, project_path.as_deref())?
        .delete(&name)
        .map_err(err)
}

/// Copy between scopes or projects; the new name is made unique ("dev (2)").
#[tauri::command(async)]
pub fn env_copy(
    app: AppHandle,
    registry: State<'_, Registry>,
    from: EnvRef,
    to: EnvScope,
    to_project_path: Option<String>,
    new_name: Option<String>,
) -> CmdResult<EnvironmentDoc> {
    let (source, name) = match &from {
        EnvRef::Project { project_path, name } => (
            store(&app, &registry, EnvScope::Project, Some(project_path))?,
            name,
        ),
        EnvRef::Global { name } => (store(&app, &registry, EnvScope::Global, None)?, name),
    };
    let target = store(&app, &registry, to, to_project_path.as_deref())?;
    copy_environment(&source, name, &target, new_name.as_deref()).map_err(err)
}

#[tauri::command(async)]
pub fn globals_get(app: AppHandle) -> CmdResult<Vec<EnvVariable>> {
    load_globals(config_dir(&app)?).map_err(err)
}

#[tauri::command(async)]
pub fn globals_save(app: AppHandle, variables: Vec<EnvVariable>) -> CmdResult<()> {
    save_globals(config_dir(&app)?, &variables).map_err(err)
}
