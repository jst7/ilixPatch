//! Per-user OAuth 2.0 token store: `<data dir>/oauth2-tokens.json`, `0600` on
//! unix, never inside a project.
//!
//! ```json
//! {
//!   "version": 1,
//!   "tokens": [ { "id": "…", "token_name": "github", "access_token": "…", … } ],
//!   "selected": { "github": "<token id>" }
//! }
//! ```

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ilixpatch_auth::oauth2::Token;
use serde::{Deserialize, Serialize};

pub const TOKENS_FILE: &str = "oauth2-tokens.json";

/// Seconds before `expires_at` at which a token already counts as expired.
pub const EXPIRY_SKEW: u64 = 30;

/// `OAuth2StoredToken` in api.ts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredToken {
    pub id: String,
    pub token_name: String,
    pub access_token: String,
    #[serde(default)]
    pub token_type: Option<String>,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub id_token: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
    /// Unix seconds.
    #[serde(default)]
    pub expires_at: Option<u64>,
    /// Unix seconds.
    pub created_at: u64,
    #[serde(default)]
    pub raw: serde_json::Value,
}

impl StoredToken {
    pub fn from_token(token_name: &str, t: Token, now: u64) -> Self {
        Self {
            id: crate::random_id(),
            token_name: token_name.to_string(),
            access_token: t.access_token,
            token_type: t.token_type,
            refresh_token: t.refresh_token,
            id_token: t.id_token,
            scope: t.scope,
            expires_at: t.expires_at,
            created_at: now,
            raw: t.raw,
        }
    }

    pub fn is_valid(&self, now: u64) -> bool {
        !self.access_token.is_empty() && self.expires_at.is_none_or(|e| e > now + EXPIRY_SKEW)
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct FileData {
    #[serde(default = "one")]
    version: u32,
    #[serde(default)]
    tokens: Vec<StoredToken>,
    #[serde(default)]
    selected: BTreeMap<String, String>,
}

fn one() -> u32 {
    1
}

pub struct TokenStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl TokenStore {
    /// Store in `<data_dir>/oauth2-tokens.json`.
    pub fn new(data_dir: impl AsRef<Path>) -> Self {
        Self {
            path: data_dir.as_ref().join(TOKENS_FILE),
            lock: Mutex::new(()),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn read(&self) -> Result<FileData, String> {
        match fs::read(&self.path) {
            Ok(b) => {
                serde_json::from_slice(&b).map_err(|e| format!("{}: {e}", self.path.display()))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(FileData {
                version: 1,
                ..Default::default()
            }),
            Err(e) => Err(format!("{}: {e}", self.path.display())),
        }
    }

    /// Atomic write (temp + rename), owner-only permissions.
    fn write(&self, data: &FileData) -> Result<(), String> {
        let err = |e: std::io::Error| format!("{}: {e}", self.path.display());
        let dir = self.path.parent().unwrap_or(Path::new("."));
        fs::create_dir_all(dir).map_err(err)?;
        let tmp = dir.join(format!(".{TOKENS_FILE}.{}.tmp", std::process::id()));
        let json = serde_json::to_vec_pretty(data).map_err(|e| e.to_string())?;
        let res = (|| {
            let mut opts = fs::OpenOptions::new();
            opts.write(true).create(true).truncate(true);
            #[cfg(unix)]
            std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
            let mut f = opts.open(&tmp)?;
            f.write_all(&json)?;
            f.sync_all()?;
            fs::rename(&tmp, &self.path)
        })();
        if res.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        res.map_err(err)
    }

    fn update<T>(&self, f: impl FnOnce(&mut FileData) -> Result<T, String>) -> Result<T, String> {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut data = self.read()?;
        let out = f(&mut data)?;
        self.write(&data)?;
        Ok(out)
    }

    /// All tokens, or those for `token_name`; newest first.
    pub fn list(&self, token_name: Option<&str>) -> Result<Vec<StoredToken>, String> {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut tokens: Vec<_> = self
            .read()?
            .tokens
            .into_iter()
            .filter(|t| token_name.is_none_or(|n| t.token_name == n))
            .collect();
        tokens.sort_by_key(|t| std::cmp::Reverse(t.created_at));
        Ok(tokens)
    }

    pub fn get(&self, id: &str) -> Result<Option<StoredToken>, String> {
        Ok(self.list(None)?.into_iter().find(|t| t.id == id))
    }

    /// Add a token and make it the selected one for its name.
    pub fn add(&self, token: StoredToken) -> Result<StoredToken, String> {
        self.update(|d| {
            d.selected
                .insert(token.token_name.clone(), token.id.clone());
            d.tokens.push(token.clone());
            Ok(token)
        })
    }

    /// Replace a stored token (same id), e.g. after a refresh.
    pub fn replace(&self, token: StoredToken) -> Result<StoredToken, String> {
        self.update(|d| {
            let slot = d
                .tokens
                .iter_mut()
                .find(|t| t.id == token.id)
                .ok_or_else(|| format!("unknown token {}", token.id))?;
            *slot = token.clone();
            Ok(token)
        })
    }

    pub fn delete(&self, ids: &[String]) -> Result<(), String> {
        self.update(|d| {
            d.tokens.retain(|t| !ids.contains(&t.id));
            d.selected.retain(|_, id| !ids.contains(id));
            Ok(())
        })
    }

    pub fn select(&self, token_name: &str, id: &str) -> Result<(), String> {
        self.update(|d| {
            if !d
                .tokens
                .iter()
                .any(|t| t.id == id && t.token_name == token_name)
            {
                return Err(format!("no token {id} named {token_name:?}"));
            }
            d.selected.insert(token_name.to_string(), id.to_string());
            Ok(())
        })
    }

    pub fn selected_id(&self, token_name: &str) -> Result<Option<String>, String> {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        Ok(self.read()?.selected.get(token_name).cloned())
    }

    /// The token to use for `token_name`: the selected one if any, else the newest.
    pub fn current(&self, token_name: &str) -> Result<Option<StoredToken>, String> {
        let tokens = self.list(Some(token_name))?;
        let selected = self.selected_id(token_name)?;
        Ok(selected
            .and_then(|id| tokens.iter().find(|t| t.id == id).cloned())
            .or_else(|| tokens.into_iter().next()))
    }
}
