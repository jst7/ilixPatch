//! "Is there a newer version?" (D54): compares this build with the latest
//! published GitHub release. Nothing is downloaded or installed; the UI links
//! to the release page. Whether and when to check is decided by the UI
//! (once a day at startup unless turned off, or on demand).

use ilixpatch_engine::UpdateCheck;
use tauri::{AppHandle, State};

use crate::network_commands::SharedEngine;

/// Uses the app's network settings (proxy, extra CAs).
#[tauri::command]
pub async fn update_check(
    app: AppHandle,
    shared: State<'_, SharedEngine>,
) -> Result<UpdateCheck, String> {
    let engine = shared.get();
    let current = app.package_info().version.to_string();
    engine.check_for_update(&current).await
}
