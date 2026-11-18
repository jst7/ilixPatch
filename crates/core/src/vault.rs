//! Secrets at rest (D60): environment secrets, globals marked secret and
//! OAuth 2.0 tokens are encrypted with AES-256-GCM.
//!
//! * A random **data key** encrypts the secrets. It is stored in
//!   `<config_dir>/vault.toml` encrypted ("wrapped") with a key derived from
//!   the user's **master password** (Argon2id). Changing the password only
//!   re-wraps the data key; no secret file has to be rewritten.
//! * The default password is [`DEFAULT_PASSWORD`] (`pass`): with it nothing is
//!   asked, and the app recommends changing it.
//! * Encrypted values are text: `ilxv1:<base64(nonce ‖ ciphertext)>`.
//!
//! The process has one vault state ([`state`]): `NotConfigured` (libraries,
//! tests: secrets are read and written in plain text as before),
//! `Locked` (the app or CLI could not open it: reading or writing a secret is
//! an error) or `Unlocked`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

pub const DEFAULT_PASSWORD: &str = "pass";
pub const VAULT_FILE: &str = "vault.toml";
/// Environment variable with the master password for the CLI / CI.
pub const PASSWORD_ENV: &str = "ILIXPATCH_PASSWORD";
const PREFIX: &str = "ilxv1:";
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;
// Argon2id, OWASP's recommended minimum: 19 MiB, 2 passes, 1 lane.
const M_KIB: u32 = 19 * 1024;
const T_COST: u32 = 2;
const P_COST: u32 = 1;

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum VaultError {
    #[error("wrong master password")]
    WrongPassword,
    #[error("secrets are locked: unlock them with your master password (CLI: set {PASSWORD_ENV})")]
    Locked,
    #[error(
        "a secret could not be decrypted (it was encrypted with another key, or it is corrupt)"
    )]
    Undecryptable,
    #[error("the new password is empty")]
    EmptyPassword,
    #[error("{0}")]
    Io(String),
}

pub type Result<T> = std::result::Result<T, VaultError>;

#[derive(Debug, Serialize, Deserialize)]
struct VaultFile {
    version: u32,
    kdf: String,
    m_kib: u32,
    t_cost: u32,
    p_cost: u32,
    /// Base64 salt for the password key.
    salt: String,
    /// The data key, encrypted with the password key (`ilxv1:` format).
    wrapped_key: String,
}

/// An opened vault: holds the data key in memory (zeroed on drop).
pub struct Vault {
    key: Zeroizing<[u8; KEY_LEN]>,
}

impl std::fmt::Debug for Vault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Vault")
    }
}

fn random<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    // OS randomness; failing here means the system can't give any.
    getrandom::fill(&mut b).expect("the OS random number generator is unavailable");
    b
}

fn io(path: &Path, e: impl std::fmt::Display) -> VaultError {
    VaultError::Io(format!("{}: {e}", path.display()))
}

fn derive(password: &str, salt: &[u8], f: &VaultFile) -> Result<Zeroizing<[u8; KEY_LEN]>> {
    let params = argon2::Params::new(f.m_kib, f.t_cost, f.p_cost, Some(KEY_LEN))
        .map_err(|e| VaultError::Io(format!("vault parameters: {e}")))?;
    let argon = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    let mut out = Zeroizing::new([0u8; KEY_LEN]);
    argon
        .hash_password_into(password.as_bytes(), salt, out.as_mut())
        .map_err(|e| VaultError::Io(format!("key derivation: {e}")))?;
    Ok(out)
}

fn cipher(key: &[u8; KEY_LEN]) -> Aes256Gcm {
    Aes256Gcm::new(&Key::<Aes256Gcm>::from(*key))
}

fn seal(key: &[u8; KEY_LEN], plain: &[u8]) -> String {
    let nonce = random::<NONCE_LEN>();
    let ct = cipher(key)
        .encrypt(&Nonce::from(nonce), plain)
        .expect("AES-GCM encryption does not fail for in-memory data");
    let mut out = nonce.to_vec();
    out.extend_from_slice(&ct);
    format!("{PREFIX}{}", B64.encode(out))
}

fn open_sealed(key: &[u8; KEY_LEN], text: &str) -> Option<Vec<u8>> {
    let raw = B64.decode(text.strip_prefix(PREFIX)?).ok()?;
    if raw.len() < NONCE_LEN {
        return None;
    }
    let (nonce, ct) = raw.split_at(NONCE_LEN);
    let nonce = Nonce::try_from(nonce).ok()?;
    cipher(key).decrypt(&nonce, ct).ok()
}

/// Whether `s` is an encrypted value (`ilxv1:…`).
pub fn is_encrypted(s: &str) -> bool {
    s.starts_with(PREFIX)
}

fn vault_path(config_dir: &Path) -> PathBuf {
    config_dir.join(VAULT_FILE)
}

fn read_file(path: &Path) -> Result<Option<VaultFile>> {
    match fs::read_to_string(path) {
        Ok(t) => toml::from_str(&t).map(Some).map_err(|e| io(path, e)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io(path, e)),
    }
}

fn write_file(path: &Path, f: &VaultFile) -> Result<()> {
    let text = format!(
        "# ilixPatch secrets key (D60), encrypted with your master password.\n\
         # Deleting this file makes every encrypted secret unreadable.\n{}",
        toml::to_string_pretty(f).map_err(|e| io(path, e))?
    );
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
    }
    let tmp = path.with_extension("toml.tmp");
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    use std::io::Write;
    let mut file = opts.open(&tmp).map_err(|e| io(&tmp, e))?;
    file.write_all(text.as_bytes()).map_err(|e| io(&tmp, e))?;
    file.sync_all().map_err(|e| io(&tmp, e))?;
    fs::rename(&tmp, path).map_err(|e| io(path, e))
}

fn wrap(data_key: &[u8; KEY_LEN], password: &str) -> Result<VaultFile> {
    let salt = random::<16>();
    let mut f = VaultFile {
        version: 1,
        kdf: "argon2id".into(),
        m_kib: M_KIB,
        t_cost: T_COST,
        p_cost: P_COST,
        salt: B64.encode(salt),
        wrapped_key: String::new(),
    };
    let kek = derive(password, &salt, &f)?;
    f.wrapped_key = seal(&kek, data_key);
    Ok(f)
}

impl Vault {
    /// Open the vault in `config_dir` with `password`. If there is none yet,
    /// one is created with a new data key, protected by [`DEFAULT_PASSWORD`]
    /// (and then `password` must be that one).
    pub fn open(config_dir: &Path, password: &str) -> Result<Self> {
        let path = vault_path(config_dir);
        let file = match read_file(&path)? {
            Some(f) => f,
            None => {
                let mut key = random::<KEY_LEN>();
                let f = wrap(&key, DEFAULT_PASSWORD)?;
                write_file(&path, &f)?;
                key.zeroize();
                f
            }
        };
        let salt = B64
            .decode(&file.salt)
            .map_err(|e| io(&path, format!("salt: {e}")))?;
        let kek = derive(password, &salt, &file)?;
        let mut data = open_sealed(&kek, &file.wrapped_key).ok_or(VaultError::WrongPassword)?;
        if data.len() != KEY_LEN {
            data.zeroize();
            return Err(io(&path, "the stored key has the wrong length"));
        }
        let mut key = Zeroizing::new([0u8; KEY_LEN]);
        key.copy_from_slice(&data);
        data.zeroize();
        Ok(Self { key })
    }

    /// Re-wrap the data key with `new` (the secrets are not rewritten).
    pub fn change_password(config_dir: &Path, old: &str, new: &str) -> Result<()> {
        if new.is_empty() {
            return Err(VaultError::EmptyPassword);
        }
        let vault = Self::open(config_dir, old)?;
        write_file(&vault_path(config_dir), &wrap(&vault.key, new)?)
    }

    /// Forgotten password: a new data key with [`DEFAULT_PASSWORD`]. Secrets
    /// encrypted with the old key can no longer be read.
    pub fn reset(config_dir: &Path) -> Result<Self> {
        let key = Zeroizing::new(random::<KEY_LEN>());
        write_file(&vault_path(config_dir), &wrap(&key, DEFAULT_PASSWORD)?)?;
        Ok(Self { key })
    }

    pub fn encrypt(&self, plain: &str) -> String {
        seal(&self.key, plain.as_bytes())
    }

    pub fn decrypt(&self, text: &str) -> Result<String> {
        let bytes = open_sealed(&self.key, text).ok_or(VaultError::Undecryptable)?;
        String::from_utf8(bytes).map_err(|_| VaultError::Undecryptable)
    }
}

// ---------- process-wide state ----------

#[derive(Debug, Clone, Default)]
pub enum State {
    /// Nobody configured a vault (libraries, tests): plain text, as before D60.
    #[default]
    NotConfigured,
    /// The app / CLI tried and could not open it.
    Locked,
    Unlocked(Arc<Vault>),
}

static STATE: RwLock<State> = RwLock::new(State::NotConfigured);

pub fn state() -> State {
    STATE.read().unwrap_or_else(|e| e.into_inner()).clone()
}

pub fn set_state(s: State) {
    *STATE.write().unwrap_or_else(|e| e.into_inner()) = s;
}

/// Open with `password` (or [`DEFAULT_PASSWORD`]) and install the result:
/// `Unlocked`, or `Locked` on a wrong password. Returns whether it opened.
pub fn install(config_dir: &Path, password: Option<&str>) -> Result<bool> {
    match Vault::open(config_dir, password.unwrap_or(DEFAULT_PASSWORD)) {
        Ok(v) => {
            set_state(State::Unlocked(Arc::new(v)));
            Ok(true)
        }
        Err(VaultError::WrongPassword) => {
            set_state(State::Locked);
            Ok(false)
        }
        Err(e) => {
            set_state(State::Locked);
            Err(e)
        }
    }
}

/// Whether the vault in `config_dir` still uses [`DEFAULT_PASSWORD`].
pub fn uses_default_password(config_dir: &Path) -> bool {
    Vault::open(config_dir, DEFAULT_PASSWORD).is_ok()
}

/// A secret value for writing: encrypted when unlocked, plain when no vault
/// is configured, an error when locked.
pub fn seal_value(plain: &str) -> Result<String> {
    match state() {
        State::Unlocked(v) => Ok(v.encrypt(plain)),
        State::NotConfigured => Ok(plain.to_string()),
        State::Locked => Err(VaultError::Locked),
    }
}

/// A stored secret value for reading: decrypted if encrypted (which needs the
/// vault), as is otherwise (plain text from before D60).
pub fn open_value(stored: &str) -> Result<String> {
    if !is_encrypted(stored) {
        return Ok(stored.to_string());
    }
    match state() {
        State::Unlocked(v) => v.decrypt(stored),
        _ => Err(VaultError::Locked),
    }
}

/// Like [`open_value`], but a value encrypted with another key (after a
/// reset, or copied from another machine) reads as empty instead of failing:
/// the secret has to be entered again. Locked is still an error.
pub fn open_value_or_empty(stored: &str) -> Result<String> {
    match open_value(stored) {
        Err(VaultError::Undecryptable) => Ok(String::new()),
        other => other,
    }
}

/// Whether plain-text secrets should be rewritten encrypted (vault unlocked).
pub fn should_migrate(stored_plain: bool) -> bool {
    stored_plain && matches!(state(), State::Unlocked(_))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_password_change_and_reset() {
        let dir = tempfile::tempdir().unwrap();
        let v = Vault::open(dir.path(), DEFAULT_PASSWORD).unwrap();
        assert!(uses_default_password(dir.path()));
        let sealed = v.encrypt("s3cret");
        assert!(is_encrypted(&sealed) && !sealed.contains("s3cret"));
        assert_ne!(sealed, v.encrypt("s3cret"), "random nonce");
        assert_eq!(v.decrypt(&sealed).unwrap(), "s3cret");

        assert_eq!(
            Vault::open(dir.path(), "nope").unwrap_err(),
            VaultError::WrongPassword
        );
        Vault::change_password(dir.path(), DEFAULT_PASSWORD, "hunter2").unwrap();
        assert!(!uses_default_password(dir.path()));
        assert_eq!(
            Vault::change_password(dir.path(), "wrong", "x").unwrap_err(),
            VaultError::WrongPassword
        );
        // Same data key: old secrets still open with the new password.
        let v2 = Vault::open(dir.path(), "hunter2").unwrap();
        assert_eq!(v2.decrypt(&sealed).unwrap(), "s3cret");
        let text = fs::read_to_string(dir.path().join(VAULT_FILE)).unwrap();
        assert!(text.contains("argon2id") && !text.contains("hunter2"));

        let v3 = Vault::reset(dir.path()).unwrap();
        assert!(uses_default_password(dir.path()));
        assert_eq!(v3.decrypt(&sealed).unwrap_err(), VaultError::Undecryptable);
        assert_eq!(
            v3.decrypt("ilxv1:!!").unwrap_err(),
            VaultError::Undecryptable
        );
    }
}
