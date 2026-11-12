//! Network flags (Newman-compatible TLS options, redirects, proxy) shared by
//! `run` and `load`, and building the [`Engine`] from them.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Args;
use ilixpatch_engine::{
    ClientCertificate, Engine, NetworkSettings, ProxyMode, ProxySettings, load_settings,
    settings::SETTINGS_FILE,
};
use serde_json::Value;

#[derive(Args, Debug, Default)]
pub struct NetworkArgs {
    /// Client certificate (PEM) for every host; with --ssl-client-key, or a PKCS#12 (.p12/.pfx)
    #[arg(long, value_name = "PATH")]
    pub ssl_client_cert: Option<PathBuf>,
    /// Private key (PEM) of --ssl-client-cert
    #[arg(long, value_name = "PATH")]
    pub ssl_client_key: Option<PathBuf>,
    /// Passphrase of the client key / PKCS#12 bundle
    #[arg(long, value_name = "PASSPHRASE")]
    pub ssl_client_passphrase: Option<String>,
    /// Newman client certificate list: JSON [{name, matches: [url patterns], cert, key, pfx, passphrase}]
    #[arg(long, value_name = "PATH")]
    pub ssl_client_cert_list: Option<PathBuf>,
    /// Extra trusted CA certificates (PEM bundle)
    #[arg(long, value_name = "PATH")]
    pub ssl_extra_ca_certs: Option<PathBuf>,
    /// Do not follow redirects
    #[arg(long)]
    pub ignore_redirects: bool,
    /// Send every request through this proxy (http, https, socks5, socks5h)
    #[arg(long, value_name = "URL", conflicts_with = "no_proxy")]
    pub proxy: Option<String>,
    /// Hosts that bypass --proxy (NO_PROXY syntax), repeatable
    #[arg(long, value_name = "HOSTS", requires = "proxy")]
    pub proxy_bypass: Vec<String>,
    /// Ignore system / environment proxies
    #[arg(long)]
    pub no_proxy: bool,
    /// Start from the desktop app's network settings (network.toml); flags override them
    #[arg(long)]
    pub use_app_settings: bool,
}

fn path_str(p: &std::path::Path) -> String {
    p.to_string_lossy().into_owned()
}

fn is_pkcs12(p: &std::path::Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("p12") || e.eq_ignore_ascii_case("pfx"))
}

/// `"path"` or `{"src": "path"}` (Newman accepts both).
fn src(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::String(s) => Some(s.clone()),
        Value::Object(o) => o.get("src").and_then(Value::as_str).map(str::to_string),
        _ => None,
    }
}

/// Parse a Newman `--ssl-client-cert-list` file. Relative paths are relative
/// to the list file.
pub fn parse_cert_list(path: &std::path::Path) -> Result<Vec<ClientCertificate>> {
    let text = std::fs::read_to_string(path).with_context(|| format!("{}", path.display()))?;
    let v: Value = serde_json::from_str(&text).with_context(|| format!("{}", path.display()))?;
    let Value::Array(items) = v else {
        bail!("{}: expected a JSON array", path.display());
    };
    let base = path.parent().unwrap_or(std::path::Path::new("."));
    let abs = |s: String| -> String {
        let p = PathBuf::from(&s);
        if p.is_absolute() {
            s
        } else {
            path_str(&base.join(p))
        }
    };
    let mut out = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let matches: Vec<String> = match item.get("matches") {
            Some(Value::Array(m)) => m
                .iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect(),
            Some(Value::String(s)) => vec![s.clone()],
            _ => bail!("{}: entry {i}: missing \"matches\"", path.display()),
        };
        let mut cert = src(item.get("cert")).map(abs);
        let key = src(item.get("key")).map(abs);
        let mut pfx = src(item.get("pfx")).map(abs);
        if pfx.is_none()
            && cert
                .as_deref()
                .is_some_and(|c| is_pkcs12(std::path::Path::new(c)))
        {
            pfx = cert.take();
        }
        let passphrase = item
            .get("passphrase")
            .and_then(Value::as_str)
            .map(str::to_string);
        for m in matches {
            out.push(ClientCertificate {
                host: m,
                cert_path: cert.clone(),
                key_path: key.clone(),
                pfx_path: pfx.clone(),
                passphrase: passphrase.clone(),
                enabled: true,
            });
        }
    }
    Ok(out)
}

impl NetworkArgs {
    /// The effective settings: app settings (if requested) or defaults, then flags.
    pub fn settings(&self, insecure: bool) -> Result<NetworkSettings> {
        let mut s = if self.use_app_settings {
            let dir = crate::app_config_dir().context("no config directory on this system")?;
            load_settings(dir.join(SETTINGS_FILE)).map_err(anyhow::Error::msg)?
        } else {
            NetworkSettings::default()
        };
        if insecure {
            s.verify_ssl = false;
        }
        if self.ignore_redirects {
            s.follow_redirects = false;
        }
        if let Some(ca) = &self.ssl_extra_ca_certs {
            s.ca_certificates.push(path_str(ca));
        }
        // Newman: the list takes precedence over the single certificate.
        if let Some(list) = &self.ssl_client_cert_list {
            s.client_certificates.splice(0..0, parse_cert_list(list)?);
        }
        if let Some(cert) = &self.ssl_client_cert {
            let pkcs12 = is_pkcs12(cert);
            s.client_certificates.push(ClientCertificate {
                host: "*".into(),
                cert_path: (!pkcs12).then(|| path_str(cert)),
                key_path: self.ssl_client_key.as_deref().map(path_str),
                pfx_path: pkcs12.then(|| path_str(cert)),
                passphrase: self.ssl_client_passphrase.clone(),
                enabled: true,
            });
        } else if self.ssl_client_key.is_some() {
            bail!("--ssl-client-key requires --ssl-client-cert");
        }
        if let Some(url) = &self.proxy {
            s.proxy = ProxySettings {
                mode: ProxyMode::Custom,
                url: Some(url.clone()),
                username: None,
                password: None,
                bypass: self.proxy_bypass.clone(),
            };
        } else if self.no_proxy {
            s.proxy.mode = ProxyMode::None;
        }
        Ok(s)
    }

    pub fn engine(&self, insecure: bool) -> Result<Engine> {
        Engine::with_settings(self.settings(insecure)?).map_err(|e| anyhow::anyhow!("{e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newman_cert_list() {
        let dir = tempfile::tempdir().unwrap();
        let list = dir.path().join("certs.json");
        // An absolute path on this OS ("/abs" has no drive on Windows, so it is not absolute there).
        let abs_cert = if cfg!(windows) {
            "C:/abs/client.crt"
        } else {
            "/abs/client.crt"
        };
        std::fs::write(
            &list,
            r#"[{"name":"a","matches":["https://api.example.com/*","https://*.corp.com/*"],
                 "key":{"src":"./client.key"},"cert":{"src":"ABS_CERT"},"passphrase":"x"},
                {"name":"b","matches":"https://p12.example.com/*","cert":"bundle.p12"}]"#
                .replace("ABS_CERT", abs_cert),
        )
        .unwrap();
        let certs = parse_cert_list(&list).unwrap();
        assert_eq!(certs.len(), 3);
        assert_eq!(certs[0].cert_path.as_deref(), Some(abs_cert));
        assert!(
            certs[0]
                .key_path
                .as_deref()
                .unwrap()
                .ends_with("client.key")
        );
        assert_eq!(certs[0].passphrase.as_deref(), Some("x"));
        assert!(
            certs[2]
                .pfx_path
                .as_deref()
                .unwrap()
                .ends_with("bundle.p12")
        );
        assert!(certs[2].cert_path.is_none());
        for c in &certs {
            ilixpatch_engine::HostPattern::parse(&c.host).unwrap();
        }
    }

    #[test]
    fn flags_override_settings() {
        let args = NetworkArgs {
            ignore_redirects: true,
            proxy: Some("http://p:3128".into()),
            proxy_bypass: vec!["localhost".into()],
            ..Default::default()
        };
        let s = args.settings(true).unwrap();
        assert!(!s.verify_ssl && !s.follow_redirects);
        assert_eq!(s.proxy.mode, ProxyMode::Custom);
        let s = NetworkArgs {
            no_proxy: true,
            ..Default::default()
        }
        .settings(false)
        .unwrap();
        assert_eq!(s.proxy.mode, ProxyMode::None);
        assert!(s.verify_ssl);
    }
}
