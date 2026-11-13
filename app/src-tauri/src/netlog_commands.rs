//! Network console (D46): the last exchanges sent by the app (requests,
//! redirects, auth rounds, OAuth 2.0 token requests), in memory only — they
//! may contain secrets, so they are never written to disk.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use ilixpatch_engine::{NetExchange, NetObserver};
use serde::Serialize;
use tauri::{Emitter, Manager, State};

/// Entries kept (oldest dropped first).
const MAX_ENTRIES: usize = 500;

#[derive(Serialize, Clone)]
pub struct NetEntry {
    pub id: u64,
    #[serde(flatten)]
    pub exchange: NetExchange,
}

#[derive(Default)]
struct Inner {
    entries: VecDeque<NetEntry>,
    next_id: u64,
}

#[derive(Default)]
pub struct NetLog {
    inner: Mutex<Inner>,
}

impl NetLog {
    fn push(&self, exchange: NetExchange) -> NetEntry {
        let mut i = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        i.next_id += 1;
        let entry = NetEntry {
            id: i.next_id,
            exchange,
        };
        i.entries.push_back(entry.clone());
        while i.entries.len() > MAX_ENTRIES {
            i.entries.pop_front();
        }
        entry
    }
}

pub fn init(app: &tauri::App) {
    app.manage(Arc::new(NetLog::default()));
}

/// The observer given to the engine and the OAuth 2.0 flows: store the
/// exchange and push it to the UI (`netlog://entry`).
pub fn observer(app: &tauri::AppHandle) -> NetObserver {
    let log = app.state::<Arc<NetLog>>().inner().clone();
    let app = app.clone();
    Arc::new(move |ex: NetExchange| {
        let entry = log.push(ex);
        let _ = app.emit("netlog://entry", entry);
    })
}

#[tauri::command]
pub fn netlog_list(log: State<'_, Arc<NetLog>>) -> Vec<NetEntry> {
    let i = log.inner.lock().unwrap_or_else(|e| e.into_inner());
    i.entries.iter().cloned().collect()
}

#[tauri::command]
pub fn netlog_clear(log: State<'_, Arc<NetLog>>) {
    log.inner
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entries
        .clear();
}
