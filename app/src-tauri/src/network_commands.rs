//! Network, HTTPS & proxy settings (D29): `<app_config_dir>/network.toml`
//! (mode 0600) and the shared [`Engine`] built from them.

use std::path::PathBuf;
use std::sync::RwLock;

use ilixpatch_engine::{
    Engine, NetworkSettings, load_settings, save_settings, settings::SETTINGS_FILE,
};
use tauri::{Manager, State};

/// The engine used by `send_request`, `run_collection` and `start_load_test`.
/// Rebuilt when the settings are saved; in-flight requests keep the engine
/// (and connection pool) they started with.
pub struct SharedEngine {
    engine: RwLock<Engine>,
    file: PathBuf,
    /// D41: where large bodies are spilled.
    spill_dir: PathBuf,
    /// D46: network console.
    observer: ilixpatch_engine::NetObserver,
}

impl SharedEngine {
    /// A cheap clone of the current engine (never hold the lock across `.await`).
    pub fn get(&self) -> Engine {
        self.engine
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

pub fn init(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let file = app.path().app_config_dir()?.join(SETTINGS_FILE);
    let spill_dir = crate::response_commands::spill_dir(app)?;
    let observer = crate::netlog_commands::observer(app.handle());
    let engine = match load_settings(&file)
        .and_then(|s| Engine::with_settings(s).map_err(|e| e.to_string()))
    {
        Ok(e) => e,
        Err(e) => {
            // E.g. a certificate file was moved: keep working with defaults;
            // the settings screen shows the saved values and the error on save.
            eprintln!("network settings: {e}; using defaults");
            Engine::new()
        }
    };
    app.manage(SharedEngine {
        engine: RwLock::new(
            engine
                .with_spill_dir(&spill_dir)
                .with_observer(observer.clone()),
        ),
        file,
        spill_dir,
        observer,
    });
    Ok(())
}

#[tauri::command(async)]
pub fn network_get_settings(shared: State<'_, SharedEngine>) -> Result<NetworkSettings, String> {
    load_settings(&shared.file)
}

/// Validate (files exist and parse, proxy URL, host patterns), save and
/// reconfigure the engine. Errors name the offending field.
#[tauri::command(async)]
pub fn network_set_settings(
    shared: State<'_, SharedEngine>,
    settings: NetworkSettings,
) -> Result<(), String> {
    let engine = Engine::with_settings(settings.clone())
        .map_err(|e| e.to_string())?
        .with_spill_dir(&shared.spill_dir)
        .with_observer(shared.observer.clone());
    save_settings(&shared.file, &settings)?;
    *shared.engine.write().unwrap_or_else(|e| e.into_inner()) = engine;
    Ok(())
}
