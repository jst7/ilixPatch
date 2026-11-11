//! Parsed import sets kept in memory between preview and apply.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::Parsed;

/// Sessions expire after this long without being used.
const TTL: Duration = Duration::from_secs(30 * 60);

#[derive(Default)]
pub struct Sessions {
    inner: Mutex<Vec<(String, Instant, Arc<Parsed>)>>,
}

impl Sessions {
    pub const fn new() -> Self {
        Self {
            inner: Mutex::new(Vec::new()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<(String, Instant, Arc<Parsed>)>> {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.retain(|(_, t, _)| t.elapsed() < TTL);
        g
    }

    /// Store `parsed`, returning its session id.
    pub fn insert(&self, parsed: Parsed) -> String {
        let id = crate::stable_id(&format!("{:?}{:p}", std::time::SystemTime::now(), &parsed));
        self.lock()
            .push((id.clone(), Instant::now(), Arc::new(parsed)));
        id
    }

    pub fn get(&self, id: &str) -> Option<Arc<Parsed>> {
        let mut g = self.lock();
        let entry = g.iter_mut().find(|(i, _, _)| i == id)?;
        entry.1 = Instant::now();
        Some(entry.2.clone())
    }

    pub fn remove(&self, id: &str) {
        self.lock().retain(|(i, _, _)| i != id);
    }
}
