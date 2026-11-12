//! Global network settings (`NetworkSettings` in api.ts) and their
//! persistence in the user's config dir (`network.toml`, mode 0600).

use std::fs;
use std::path::Path;

pub use ilixpatch_core::HttpVersionPref;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientCertificate {
    /// `api.example.com`, `*.example.com` or `*`, optionally with `:port`.
    pub host: String,
    #[serde(default)]
    pub cert_path: Option<String>,
    #[serde(default)]
    pub key_path: Option<String>,
    #[serde(default)]
    pub pfx_path: Option<String>,
    /// For the PFX/P12 bundle or an encrypted PKCS#8 PEM key.
    #[serde(default)]
    pub passphrase: Option<String>,
    #[serde(default = "yes")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProxyMode {
    /// Environment variables (`HTTPS_PROXY`, `NO_PROXY`…) and the OS proxy configuration.
    #[default]
    System,
    None,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ProxySettings {
    #[serde(default)]
    pub mode: ProxyMode,
    /// `http://proxy:8080`, `https://…`, `socks5://…` or `socks5h://…`.
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    /// `NO_PROXY` syntax entries.
    #[serde(default)]
    pub bypass: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TlsVersion {
    #[serde(rename = "1.2")]
    Tls12,
    #[serde(rename = "1.3")]
    Tls13,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NetworkSettings {
    pub verify_ssl: bool,
    /// Trust the OS root certificates (the bundled Mozilla roots are always trusted).
    pub use_system_ca: bool,
    /// Extra trusted CA certificates (PEM files).
    pub ca_certificates: Vec<String>,
    pub client_certificates: Vec<ClientCertificate>,
    pub proxy: ProxySettings,
    pub follow_redirects: bool,
    pub max_redirects: u32,
    /// 0 = no timeout.
    pub timeout_ms: u64,
    pub min_tls_version: Option<TlsVersion>,
    pub http_version: HttpVersionPref,
    /// D41: bodies up to this many KB stay in memory (and go whole to the UI).
    pub inline_body_kb: u64,
    /// D41: stop downloading after this many MB (body incomplete); 0 = unlimited.
    pub max_body_mb: u64,
}

fn yes() -> bool {
    true
}

impl Default for NetworkSettings {
    fn default() -> Self {
        Self {
            verify_ssl: true,
            use_system_ca: true,
            ca_certificates: Vec::new(),
            client_certificates: Vec::new(),
            proxy: ProxySettings::default(),
            follow_redirects: true,
            max_redirects: 10,
            timeout_ms: 0,
            min_tls_version: None,
            http_version: HttpVersionPref::Auto,
            inline_body_kb: 1024,
            max_body_mb: 1024,
        }
    }
}

/// File name inside the app config dir.
pub const SETTINGS_FILE: &str = "network.toml";

/// Load settings; a missing file gives the defaults.
pub fn load_settings(path: impl AsRef<Path>) -> Result<NetworkSettings, String> {
    let path = path.as_ref();
    match fs::read_to_string(path) {
        Ok(s) => toml::from_str(&s).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(NetworkSettings::default()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Save settings with owner-only permissions (they may hold passphrases and
/// proxy passwords). Does not validate; see [`crate::Engine::with_settings`].
pub fn save_settings(path: impl AsRef<Path>, settings: &NetworkSettings) -> Result<(), String> {
    let path = path.as_ref();
    let err = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| err(&e))?;
    }
    let text = toml::to_string_pretty(settings).map_err(|e| err(&e))?;
    let tmp = path.with_extension("toml.tmp");
    {
        let mut opts = fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        use std::io::Write;
        let mut f = opts.open(&tmp).map_err(|e| err(&e))?;
        f.write_all(text.as_bytes()).map_err(|e| err(&e))?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600)).map_err(|e| err(&e))?;
    }
    fs::rename(&tmp, path).map_err(|e| err(&e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_shape_matches_api_ts() {
        let v = serde_json::to_value(NetworkSettings::default()).unwrap();
        let mut keys: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "ca_certificates",
                "client_certificates",
                "follow_redirects",
                "http_version",
                "inline_body_kb",
                "max_body_mb",
                "max_redirects",
                "min_tls_version",
                "proxy",
                "timeout_ms",
                "use_system_ca",
                "verify_ssl"
            ]
        );
        assert_eq!(v["proxy"]["mode"], "system");
        assert_eq!(v["http_version"], "auto");
        let s: NetworkSettings = serde_json::from_value(serde_json::json!({
            "verify_ssl": false, "use_system_ca": true, "ca_certificates": [],
            "client_certificates": [{"host": "*.x.com", "cert_path": null, "key_path": null,
                "pfx_path": "/a.p12", "passphrase": "pw", "enabled": true}],
            "proxy": {"mode": "custom", "url": "http://p:1", "username": null, "password": null, "bypass": ["localhost"]},
            "follow_redirects": true, "max_redirects": 5, "timeout_ms": 0,
            "min_tls_version": "1.3", "http_version": "http2"
        }))
        .unwrap();
        assert_eq!(s.min_tls_version, Some(TlsVersion::Tls13));
        assert_eq!(s.proxy.mode, ProxyMode::Custom);
    }

    #[test]
    fn save_load_roundtrip_with_private_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("cfg").join(SETTINGS_FILE);
        assert_eq!(load_settings(&p).unwrap(), NetworkSettings::default());
        let s = NetworkSettings {
            verify_ssl: false,
            min_tls_version: Some(TlsVersion::Tls12),
            proxy: ProxySettings {
                mode: ProxyMode::Custom,
                url: Some("http://p:8080".into()),
                password: Some("secret".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        save_settings(&p, &s).unwrap();
        assert_eq!(load_settings(&p).unwrap(), s);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&p).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }
}
