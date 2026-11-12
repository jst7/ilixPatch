//! Authorization (D39) save commands: collection `[auth]` / `[variables]` in
//! `collection.toml` and folder `[auth]` in `folder.toml`. Paths must be
//! inside a registered project; files are edited in place (comments kept).

use ilixpatch_core::auth::Auth;
use ilixpatch_core::storage;
use indexmap::IndexMap;
use tauri::State;

use crate::registry::Registry;
use crate::{CmdResult, err, inside_project};

/// Set or clear (`null` = no auth) a collection's auth.
#[tauri::command(async)]
pub fn save_collection_auth(
    registry: State<'_, Registry>,
    collection_path: String,
    auth: Option<Auth>,
) -> CmdResult<()> {
    let dir = inside_project(&registry, &collection_path)?;
    storage::save_collection_auth(dir, auth.as_ref()).map_err(err)
}

/// Set or clear (`null` = inherit; `folder.toml` removed when empty) a folder's auth.
#[tauri::command(async)]
pub fn save_folder_auth(
    registry: State<'_, Registry>,
    folder_path: String,
    auth: Option<Auth>,
) -> CmdResult<()> {
    let dir = inside_project(&registry, &folder_path)?;
    storage::save_folder_auth(dir, auth.as_ref()).map_err(err)
}

/// Replace a collection's variables.
#[tauri::command(async)]
pub fn save_collection_variables(
    registry: State<'_, Registry>,
    collection_path: String,
    variables: IndexMap<String, String>,
) -> CmdResult<()> {
    let dir = inside_project(&registry, &collection_path)?;
    storage::save_collection_variables(dir, &variables).map_err(err)
}
