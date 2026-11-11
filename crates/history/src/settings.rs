//! Retention settings, persisted as `history.toml`.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::Result;

/// User-facing history settings (mirrors `HistorySettings` in `app/src/lib/api.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistorySettings {
    pub enabled: bool,
    /// Delete entries older than N days; `None` = keep forever.
    pub max_days: Option<u32>,
    /// Max total storage in MB; oldest entries are pruned first. `None` = unlimited.
    pub max_size_mb: Option<u64>,
    pub save_response_body: bool,
    /// Bodies larger than this are truncated (`body_truncated = true`).
    pub max_body_kb: u64,
}

impl Default for HistorySettings {
    fn default() -> Self {
        Self {
            enabled: true,
            max_days: Some(30),
            max_size_mb: Some(200),
            save_response_body: true,
            max_body_kb: 1024,
        }
    }
}

impl HistorySettings {
    /// `Some(0)` limits mean "unlimited", like `None`.
    pub fn normalized(mut self) -> Self {
        self.max_days = self.max_days.filter(|d| *d > 0);
        self.max_size_mb = self.max_size_mb.filter(|m| *m > 0);
        self
    }

    pub(crate) fn max_size_bytes(&self) -> Option<u64> {
        self.max_size_mb.map(|mb| mb.saturating_mul(1024 * 1024))
    }

    pub(crate) fn max_body_bytes(&self) -> usize {
        usize::try_from(self.max_body_kb.saturating_mul(1024)).unwrap_or(usize::MAX)
    }

    /// Load from `path`; a missing file yields the defaults.
    pub fn load(path: &Path) -> Result<Self> {
        match fs::read_to_string(path) {
            Ok(text) => Ok(toml::from_str::<SettingsFile>(&text)?.into()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let text = toml::to_string_pretty(&SettingsFile::from(self.clone()))?;
        let tmp = path.with_extension("toml.tmp");
        fs::write(&tmp, text)?;
        fs::rename(&tmp, path)?;
        Ok(())
    }
}

/// On-disk form. TOML has no `null`, so "unlimited" is written as `0`;
/// missing keys fall back to the defaults.
#[derive(Serialize, Deserialize)]
#[serde(default)]
struct SettingsFile {
    enabled: bool,
    /// 0 = keep forever.
    max_days: u32,
    /// 0 = unlimited.
    max_size_mb: u64,
    save_response_body: bool,
    max_body_kb: u64,
}

impl Default for SettingsFile {
    fn default() -> Self {
        HistorySettings::default().into()
    }
}

impl From<HistorySettings> for SettingsFile {
    fn from(s: HistorySettings) -> Self {
        Self {
            enabled: s.enabled,
            max_days: s.max_days.unwrap_or(0),
            max_size_mb: s.max_size_mb.unwrap_or(0),
            save_response_body: s.save_response_body,
            max_body_kb: s.max_body_kb,
        }
    }
}

impl From<SettingsFile> for HistorySettings {
    fn from(f: SettingsFile) -> Self {
        Self {
            enabled: f.enabled,
            max_days: Some(f.max_days),
            max_size_mb: Some(f.max_size_mb),
            save_response_body: f.save_response_body,
            max_body_kb: f.max_body_kb,
        }
        .normalized()
    }
}
