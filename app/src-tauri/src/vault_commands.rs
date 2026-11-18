//! Master password (D60): unlock, change and reset the secrets vault.
//! At startup the vault is opened with the default password; if the user
//! changed it, the app starts locked and the UI asks for it.

use std::path::PathBuf;

use ilixpatch_core::vault::{self, State, Vault};
use serde::Serialize;
use tauri::{App, Manager};

type CmdResult<T> = Result<T, String>;

pub struct VaultDir(PathBuf);

/// `VaultStatus` in api.ts.
#[derive(Debug, Serialize)]
pub struct VaultStatus {
    pub unlocked: bool,
    /// Unlocked with the default password ("pass"): the UI recommends changing it.
    pub default_password: bool,
}

pub fn init(app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    let dir = app.path().app_config_dir()?;
    // A wrong default password just means "locked"; other errors also leave it locked.
    if let Err(e) = vault::install(&dir, None) {
        eprintln!("vault: {e}");
    }
    app.manage(VaultDir(dir));
    Ok(())
}

fn status(dir: &VaultDir) -> VaultStatus {
    let unlocked = matches!(vault::state(), State::Unlocked(_));
    VaultStatus {
        unlocked,
        default_password: unlocked && vault::uses_default_password(&dir.0),
    }
}

#[tauri::command(async)]
pub fn vault_status(dir: tauri::State<'_, VaultDir>) -> VaultStatus {
    status(&dir)
}

#[tauri::command(async)]
pub fn vault_unlock(dir: tauri::State<'_, VaultDir>, password: String) -> CmdResult<VaultStatus> {
    match vault::install(&dir.0, Some(&password)) {
        Ok(true) => Ok(status(&dir)),
        Ok(false) => Err(vault::VaultError::WrongPassword.to_string()),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command(async)]
pub fn vault_change_password(
    dir: tauri::State<'_, VaultDir>,
    current: String,
    new: String,
) -> CmdResult<VaultStatus> {
    Vault::change_password(&dir.0, &current, &new).map_err(|e| e.to_string())?;
    vault::install(&dir.0, Some(&new)).map_err(|e| e.to_string())?;
    Ok(status(&dir))
}

/// Forgotten password: a new key with the default password. Secrets saved with
/// the old key read as empty from now on.
#[tauri::command(async)]
pub fn vault_reset(dir: tauri::State<'_, VaultDir>) -> CmdResult<VaultStatus> {
    let v = Vault::reset(&dir.0).map_err(|e| e.to_string())?;
    vault::set_state(State::Unlocked(std::sync::Arc::new(v)));
    Ok(status(&dir))
}
