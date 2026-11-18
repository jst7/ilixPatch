//! Mock servers (D62): serve the examples of a project or collection while the
//! app is open. One server per target; each request is sent to the UI as a
//! `mock://request` event.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use ilixpatch_mock::{MockLogEntry, MockOptions, MockServer, RouteInfo, Source};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::registry::Registry;

type CmdResult<T> = Result<T, String>;

#[derive(Default)]
pub struct Mocks(Mutex<HashMap<String, MockServer>>);

/// `MockStatus` in api.ts.
#[derive(Debug, Clone, Serialize)]
pub struct MockStatus {
    /// The served project / collection directory (also the server's id).
    pub target: String,
    pub url: String,
    pub port: u16,
    pub routes: Vec<RouteInfo>,
}

#[derive(Debug, Clone, Serialize)]
struct MockEvent {
    target: String,
    entry: MockLogEntry,
}

fn status(target: &str, s: &MockServer) -> MockStatus {
    MockStatus {
        target: target.to_string(),
        url: s.url(),
        port: s.addr.port(),
        routes: s.routes.clone(),
    }
}

/// Start (or restart) the mock server of `target` on `port` (0 = any free port).
#[tauri::command]
pub async fn mock_start(
    app: AppHandle,
    registry: State<'_, Registry>,
    mocks: State<'_, Mocks>,
    target: String,
    port: u16,
    delay_ms: u64,
) -> CmdResult<MockStatus> {
    let roots = registry.paths()?;
    let dir = std::fs::canonicalize(&target).map_err(|e| format!("{target}: {e}"))?;
    let inside = roots
        .iter()
        .filter_map(|r| std::fs::canonicalize(r).ok())
        .any(|r| dir.starts_with(&r));
    if !inside {
        return Err(format!(
            "{}: not inside a registered project",
            dir.display()
        ));
    }
    // Restarting on the same port: stop the old one first.
    if let Some(old) = mocks
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&target)
    {
        old.stop();
    }
    let emit_target = target.clone();
    let on_request = Arc::new(move |entry: MockLogEntry| {
        let _ = app.emit(
            "mock://request",
            MockEvent {
                target: emit_target.clone(),
                entry,
            },
        );
    });
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let server = ilixpatch_mock::start(
        Source::new(&dir),
        addr,
        MockOptions { delay_ms },
        Some(on_request),
    )
    .await?;
    let st = status(&target, &server);
    mocks
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(target, server);
    Ok(st)
}

#[tauri::command]
pub fn mock_stop(mocks: State<'_, Mocks>, target: String) {
    if let Some(s) = mocks
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&target)
    {
        s.stop();
    }
}

/// Running servers.
#[tauri::command]
pub fn mock_list(mocks: State<'_, Mocks>) -> Vec<MockStatus> {
    mocks
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .map(|(t, s)| status(t, s))
        .collect()
}
