mod auth_commands;
mod env_commands;
mod git_commands;
mod history_commands;
mod import_commands;
mod netlog_commands;
mod network_commands;
mod oauth2_commands;
mod registry;
mod response_commands;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ilixpatch_core::{Project, Request, RequestFile, project, storage};
use ilixpatch_runner::{
    CancellationToken, ExecContext, LoadConfig, LoadOptions, LoadSnapshot, RequestError,
    RequestErrorKind, RunConfig, RunEvent, RunOptions, RunSummary, VarStore, execute_request,
    resolve_target, run_load_test,
};
use ilixpatch_scripting::{TestResult, VarUpdate};
use indexmap::IndexMap;
use network_commands::SharedEngine;
use registry::Registry;
use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::{Manager, State};

type CmdResult<T> = Result<T, String>;

fn err(e: impl ToString) -> String {
    e.to_string()
}

/// Result of `send_request` (`SendResult` in api.ts).
#[derive(Serialize)]
struct SendResult {
    status: u16,
    status_text: String,
    headers: Vec<(String, String)>,
    elapsed_ms: f64,
    size_bytes: usize,
    /// Whole body when small (≤ `inline_body_kb`), else a preview (D41).
    body: String,
    /// D41: handle to the full body in the `ResponseStore`.
    body_handle: Option<ilixpatch_engine::bodystore::BodyHandle>,
    tests: Vec<TestResult>,
    logs: Vec<String>,
    script_error: Option<String>,
    variable_updates: Vec<VarUpdate>,
    /// D29 network details of the final response.
    network: Option<ilixpatch_engine::NetworkInfo>,
}

/// Variables for a single send, by scope (`SendVariables` in api.ts).
#[derive(Deserialize, Default)]
#[serde(default)]
struct SendVariables {
    globals: IndexMap<String, String>,
    collection: IndexMap<String, String>,
    environment: IndexMap<String, String>,
    environment_name: Option<String>,
}

/// Cancellation tokens of single sends in progress, keyed by the UI-provided request id.
#[derive(Default)]
struct InFlight(Mutex<std::collections::HashMap<String, CancellationToken>>);

/// Removes the send from `InFlight` when it finishes (or its future is dropped).
struct InFlightGuard<'a> {
    map: &'a InFlight,
    id: String,
}

impl<'a> InFlightGuard<'a> {
    fn register(map: &'a InFlight, id: &str, token: CancellationToken) -> Self {
        map.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id.to_string(), token);
        Self {
            map,
            id: id.to_string(),
        }
    }
}

impl Drop for InFlightGuard<'_> {
    fn drop(&mut self) {
        self.map
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.id);
    }
}

/// Abort a single send started with this `request_id`. No-op if it already finished.
#[tauri::command]
fn cancel_request(in_flight: State<'_, InFlight>, request_id: String) {
    if let Some(token) = in_flight
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&request_id)
    {
        token.cancel();
    }
}

/// Cancellation tokens of the (single) running collection run and load test.
#[derive(Default)]
struct Jobs {
    run: Mutex<Option<CancellationToken>>,
    load: Mutex<Option<CancellationToken>>,
}

/// Holds a job slot while a run / load test is in progress; frees it on drop.
struct JobGuard<'a>(&'a Mutex<Option<CancellationToken>>);

impl<'a> JobGuard<'a> {
    fn acquire(slot: &'a Mutex<Option<CancellationToken>>) -> CmdResult<(Self, CancellationToken)> {
        let mut s = slot.lock().unwrap_or_else(|e| e.into_inner());
        if s.is_some() {
            return Err("a run is already in progress".into());
        }
        let token = CancellationToken::new();
        *s = Some(token.clone());
        Ok((Self(slot), token))
    }
}

impl Drop for JobGuard<'_> {
    fn drop(&mut self) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

fn cancel_job(slot: &Mutex<Option<CancellationToken>>) {
    if let Some(t) = slot.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        t.cancel();
    }
}

/// One registered project. If it failed to load, `project` is `None` and `error` is set.
#[derive(Serialize)]
struct ProjectEntry {
    path: PathBuf,
    project: Option<Project>,
    error: Option<String>,
}

/// Pre-request script → resolve `{{vars}}` → send → tests script. Heavy work
/// stays in Rust; the UI only receives what it renders. A script error is
/// reported in `script_error`; only a failed send (or unresolved variable,
/// skip, cancel) is an `Err`, carrying a classified `RequestError` (D36).
#[tauri::command]
#[allow(clippy::too_many_arguments)] // Tauri injects state as arguments
async fn send_request(
    app: tauri::AppHandle,
    engine: State<'_, SharedEngine>,
    history_store: State<'_, ilixpatch_history::HistoryStore>,
    in_flight: State<'_, InFlight>,
    oauth2: State<'_, oauth2_commands::OAuth2State>,
    bodies: State<'_, ilixpatch_engine::bodystore::ResponseStore>,
    request: Request,
    variables: SendVariables,
    history: Option<ilixpatch_history::HistoryContext>,
    request_id: Option<String>,
) -> Result<SendResult, Box<RequestError>> {
    let mut store = VarStore {
        globals: variables.globals,
        collection: variables.collection,
        environment: variables.environment,
        environment_name: variables.environment_name,
        local: IndexMap::new(),
    };
    let engine = engine.get();
    // Register a cancellation token so the UI can abort a hung call (`cancel_request`).
    // Dropping the in-flight future closes the HTTP connection immediately.
    let token = CancellationToken::new();
    let _guard = request_id
        .as_deref()
        .map(|id| InFlightGuard::register(&in_flight, id, token.clone()));
    let ctx = ExecContext {
        oauth2: Some(&*oauth2.provider),
        // D46: the exchanges of this send are grouped in the network console.
        trace: Some(ilixpatch_engine::Trace::new(
            if request.name.trim().is_empty() {
                format!("{} {}", request.method.as_str(), request.url)
            } else {
                request.name.clone()
            },
        )),
        ..Default::default()
    };
    let res = tokio::select! {
        res = execute_request(&engine, &request, &mut store, &ctx) => res,
        _ = token.cancelled() => return Err(Box::new(RequestError::cancelled())),
    };
    if res.skipped {
        return Err(Box::new(RequestError::skipped_by_script()));
    }

    // Record every send (successful or not) in the local history without delaying the
    // response; the UI refreshes its list on the `history://recorded` event.
    let sent = res.request.clone().unwrap_or_else(|| request.clone());
    let entry = ilixpatch_history::NewEntry {
        timestamp_ms: ilixpatch_history::now_ms(),
        name: request.name.clone(),
        method: sent.method,
        url: sent.url.clone(),
        status: res.response.as_ref().map(|r| r.status),
        status_text: res
            .response
            .as_ref()
            .map(|r| r.status_text.clone())
            .unwrap_or_default(),
        headers: res
            .response
            .as_ref()
            .map(|r| r.headers.clone())
            .unwrap_or_default(),
        body: None, // filled off the async runtime (may be a large temp file)
        elapsed_ms: res.response.as_ref().map_or(0.0, |r| r.elapsed_ms),
        size_bytes: res.response.as_ref().map_or(0, |r| r.size_bytes as u64),
        error: res.error.clone(),
        request: request.clone(),
        resolved_request: res.request.clone(),
        tests: serde_json::to_value(&res.tests).unwrap_or_default(),
        logs: res.logs.clone(),
        context: history.unwrap_or_default(),
    };
    let history_store = history_store.inner().clone();
    let history_body = res.response.as_ref().map(|r| r.body.clone());
    tauri::async_runtime::spawn_blocking(move || {
        // D41: read at most what history keeps (plus a little, so it marks the cut).
        let mut entry = entry;
        let hs = history_store.settings();
        if hs.save_response_body {
            let max = usize::try_from(hs.max_body_kb.saturating_mul(1024)).unwrap_or(usize::MAX);
            entry.body = history_body
                .and_then(|b| b.text_limited(max.saturating_add(4)).ok())
                .map(|(t, _)| t);
        }
        match history_store.record(entry) {
            Ok(Some(id)) => {
                let _ = tauri::Emitter::emit(&app, "history://recorded", id);
            }
            Ok(None) => {}
            Err(e) => eprintln!("history: record failed: {e}"),
        }
    });

    // D36: a failed send rejects with a classified `RequestError`.
    if let Some(e) = res.request_error {
        return Err(Box::new(e));
    }
    let resp = res.response.ok_or_else(|| {
        Box::new(RequestError::new(
            RequestErrorKind::Other,
            "No response was received.",
            res.error.clone().unwrap_or_else(|| "no response".into()),
        ))
    })?;
    let (handle, body) = response_commands::keep(&bodies, &engine, &resp);
    Ok(SendResult {
        body,
        body_handle: Some(handle),
        network: Some(resp.network),
        status: resp.status,
        status_text: resp.status_text,
        headers: resp.headers,
        elapsed_ms: resp.elapsed_ms,
        size_bytes: resp.size_bytes,
        tests: res.tests,
        logs: res.logs,
        script_error: res.script_error,
        variable_updates: res.variable_updates,
    })
}

/// Run a collection, folder or request (inside a registered project). Events
/// stream through `on_event`; returns the summary. One run at a time.
#[tauri::command]
async fn run_collection(
    app: tauri::AppHandle,
    engine: State<'_, SharedEngine>,
    registry: State<'_, Registry>,
    jobs: State<'_, Jobs>,
    oauth2: State<'_, oauth2_commands::OAuth2State>,
    options: RunOptions,
    on_event: Channel<RunEvent>,
) -> CmdResult<RunSummary> {
    let target = inside_project(&registry, &options.target)?;
    let (_guard, cancel) = JobGuard::acquire(&jobs.run)?;
    // The data file is user-chosen and may live anywhere; it is only read.
    // Global environments and globals.toml come from the app config dir (D28).
    let config_dir = env_commands::config_dir(&app)?;
    let mut config = RunConfig::from_options_in(&options, Some(&config_dir)).map_err(err)?;
    config.oauth2 = Some(oauth2.handle());
    let targets = resolve_target(&target).map_err(err)?;
    let engine = engine.get();
    let report = ilixpatch_runner::run_collection(&engine, &targets, &config, &cancel, |e| {
        let _ = on_event.send(e.clone());
    })
    .await
    .map_err(err)?;
    Ok(report.summary)
}

#[tauri::command]
fn cancel_run(jobs: State<'_, Jobs>) {
    cancel_job(&jobs.run);
}

/// Start a load test; snapshots stream ~1/s through `on_snapshot`. Returns the
/// final snapshot. One load test at a time.
#[tauri::command]
async fn start_load_test(
    app: tauri::AppHandle,
    engine: State<'_, SharedEngine>,
    registry: State<'_, Registry>,
    jobs: State<'_, Jobs>,
    oauth2: State<'_, oauth2_commands::OAuth2State>,
    options: LoadOptions,
    on_snapshot: Channel<LoadSnapshot>,
) -> CmdResult<LoadSnapshot> {
    let target = inside_project(&registry, &options.target)?;
    let (_guard, stop) = JobGuard::acquire(&jobs.load)?;
    let config_dir = env_commands::config_dir(&app)?;
    let mut config = LoadConfig::from_options_in(&options, Some(&config_dir)).map_err(err)?;
    config.oauth2 = Some(oauth2.handle());
    let targets = resolve_target(&target).map_err(err)?;
    let engine = engine.get();
    run_load_test(&engine, &targets, &config, &stop, |s| {
        let _ = on_snapshot.send(s.clone());
    })
    .await
    .map_err(err)
}

#[tauri::command]
fn stop_load_test(jobs: State<'_, Jobs>) {
    cancel_job(&jobs.load);
}

#[tauri::command(async)]
fn list_projects(registry: State<'_, Registry>) -> CmdResult<Vec<ProjectEntry>> {
    Ok(registry
        .paths()?
        .into_iter()
        .map(|path| match project::load_project(&path) {
            Ok(p) => ProjectEntry {
                path,
                project: Some(p),
                error: None,
            },
            Err(e) => ProjectEntry {
                path,
                project: None,
                error: Some(e.to_string()),
            },
        })
        .collect())
}

/// Register a folder as a project, creating `.ilixpatch/config.toml` if missing.
#[tauri::command(async)]
fn add_project(registry: State<'_, Registry>, path: String) -> CmdResult<Project> {
    project::init_project(&path).map_err(err)?;
    let p = project::load_project(&path).map_err(err)?;
    registry.add(&p.path)?;
    Ok(p)
}

/// Unregister only; never deletes files.
#[tauri::command(async)]
fn remove_project(registry: State<'_, Registry>, path: String) -> CmdResult<()> {
    registry.remove(Path::new(&path))
}

#[tauri::command(async)]
fn reload_project(path: String) -> CmdResult<Project> {
    project::load_project(&path).map_err(err)
}

/// The path, canonicalized, if it is strictly inside a registered project.
fn inside_project(registry: &Registry, path: &str) -> CmdResult<PathBuf> {
    project::resolve_inside(&registry.paths()?, path).map_err(err)
}

fn path_string(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

#[tauri::command(async)]
fn create_collection(
    registry: State<'_, Registry>,
    project_path: String,
    name: String,
) -> CmdResult<String> {
    let root = fs::canonicalize(&project_path).map_err(|e| format!("{project_path}: {e}"))?;
    if !registry.paths()?.contains(&root) {
        return Err(format!("{project_path}: not a registered project"));
    }
    project::create_collection(&root, &name)
        .map(|p| path_string(&p))
        .map_err(err)
}

#[tauri::command(async)]
fn create_folder(
    registry: State<'_, Registry>,
    parent_path: String,
    name: String,
) -> CmdResult<String> {
    let parent = inside_project(&registry, &parent_path)?;
    project::create_folder(parent, &name)
        .map(|p| path_string(&p))
        .map_err(err)
}

#[tauri::command(async)]
fn create_request(
    registry: State<'_, Registry>,
    folder_path: String,
    name: String,
) -> CmdResult<RequestFile> {
    let folder = inside_project(&registry, &folder_path)?;
    project::create_request(folder, &name).map_err(err)
}

#[tauri::command(async)]
fn save_request(registry: State<'_, Registry>, path: String, request: Request) -> CmdResult<()> {
    let path = inside_project(&registry, &path)?;
    if path.extension().is_none_or(|e| e != "toml") {
        return Err(format!("{}: requests must be .toml files", path.display()));
    }
    storage::save_request(path, &request).map_err(err)
}

/// Delete a request file, folder or collection inside a registered project.
#[tauri::command(async)]
fn delete_item(registry: State<'_, Registry>, path: String) -> CmdResult<()> {
    project::delete_item(&registry.paths()?, path).map_err(err)
}

/// macOS app menu. Same as Tauri's default but WITHOUT "Close Window" (⌘W),
/// so ⌘W reaches the web UI and closes the active tab instead of the window.
/// The Edit menu must stay: on macOS it provides copy/paste/undo in the webview.
#[cfg(target_os = "macos")]
fn app_menu(app: &tauri::AppHandle) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    use tauri::menu::{MenuBuilder, PredefinedMenuItem, SubmenuBuilder};
    let app_menu = SubmenuBuilder::new(app, "ilixPatch")
        .item(&PredefinedMenuItem::about(
            app,
            Some("About ilixPatch"),
            Some(tauri::menu::AboutMetadata {
                name: Some("ilixPatch".into()),
                version: Some(app.package_info().version.to_string()),
                authors: Some(vec!["Jorge Segovia Tormo".into()]),
                website: Some("https://github.com/jst7".into()),
                website_label: Some("github.com/jst7".into()),
                copyright: Some("© 2026 Jorge Segovia Tormo and ilixPatch contributors".into()),
                license: Some("MIT OR Apache-2.0".into()),
                ..Default::default()
            }),
        )?)
        .separator()
        .item(&PredefinedMenuItem::services(app, None)?)
        .separator()
        .item(&PredefinedMenuItem::hide(app, None)?)
        .item(&PredefinedMenuItem::hide_others(app, None)?)
        .item(&PredefinedMenuItem::show_all(app, None)?)
        .separator()
        .item(&PredefinedMenuItem::quit(app, None)?)
        .build()?;
    let edit = SubmenuBuilder::new(app, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;
    let window = SubmenuBuilder::new(app, "Window")
        .item(&PredefinedMenuItem::minimize(app, None)?)
        .item(&PredefinedMenuItem::maximize(app, None)?)
        .separator()
        .item(&PredefinedMenuItem::fullscreen(app, None)?)
        .build()?;
    MenuBuilder::new(app)
        .items(&[&app_menu, &edit, &window])
        .build()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(Jobs::default())
        .manage(InFlight::default())
        .setup(|app| {
            let dir = app.path().app_config_dir()?;
            app.manage(Registry::new(dir.join("projects.toml")));
            history_commands::init(app)?;
            netlog_commands::init(app);
            network_commands::init(app)?;
            oauth2_commands::init(app)?;
            response_commands::init(app)?;
            #[cfg(target_os = "macos")]
            app.set_menu(app_menu(app.handle())?)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            send_request,
            cancel_request,
            run_collection,
            cancel_run,
            start_load_test,
            stop_load_test,
            list_projects,
            add_project,
            remove_project,
            reload_project,
            create_collection,
            create_folder,
            create_request,
            save_request,
            delete_item,
            git_commands::git_repos,
            git_commands::git_status,
            git_commands::git_branches,
            git_commands::git_checkout,
            git_commands::git_init,
            git_commands::git_diff_file,
            git_commands::git_commit,
            git_commands::git_discard,
            git_commands::git_fetch,
            git_commands::git_pull,
            git_commands::git_push,
            git_commands::git_log,
            git_commands::git_pull_with,
            git_commands::git_merge,
            git_commands::git_merge_state,
            git_commands::git_conflict_versions,
            git_commands::git_resolve_file,
            git_commands::git_resolve_with,
            git_commands::git_abort_operation,
            git_commands::git_continue_operation,
            git_commands::git_blame_file,
            git_commands::git_blame_request,
            git_commands::git_clone,
            git_commands::git_cancel_clone,
            git_commands::git_log_page,
            git_commands::git_commit_details,
            git_commands::git_commit_file_diff,
            git_commands::git_revert_commit,
            git_commands::git_reset_to,
            git_commands::git_restore_file,
            git_commands::git_cherry_pick,
            git_commands::git_create_branch_at,
            git_commands::git_checkout_commit,
            import_commands::import_postman_preview,
            import_commands::import_postman_apply,
            oauth2_commands::oauth2_get_token,
            oauth2_commands::oauth2_cancel_flow,
            oauth2_commands::oauth2_refresh_token,
            oauth2_commands::oauth2_list_tokens,
            oauth2_commands::oauth2_delete_tokens,
            oauth2_commands::oauth2_select_token,
            history_commands::history_list,
            history_commands::history_get,
            history_commands::history_delete,
            history_commands::history_clear,
            history_commands::history_stats,
            history_commands::history_get_settings,
            history_commands::history_set_settings,
            env_commands::env_list,
            env_commands::env_save,
            env_commands::env_delete,
            env_commands::env_copy,
            env_commands::globals_get,
            env_commands::globals_save,
            auth_commands::save_collection_auth,
            auth_commands::save_folder_auth,
            auth_commands::save_collection_variables,
            network_commands::network_get_settings,
            network_commands::network_set_settings,
            response_commands::response_read,
            response_commands::response_lines,
            response_commands::response_search,
            response_commands::response_save,
            response_commands::response_release,
            response_commands::response_open,
            netlog_commands::netlog_list,
            netlog_commands::netlog_clear,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // D41: delete the temp files of large bodies on exit.
            if let tauri::RunEvent::Exit = event
                && let Some(store) = app.try_state::<ilixpatch_engine::bodystore::ResponseStore>()
            {
                store.clear();
            }
        });
}
