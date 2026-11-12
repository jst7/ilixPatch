//! Sends fully-resolved [`Request`]s.
//!
//! An [`Engine`] is built from [`NetworkSettings`] (TLS verification, CA
//! certificates, client certificates, proxy, redirects, timeout, TLS and HTTP
//! versions). It caches one pooled `reqwest::Client` per effective TLS/HTTP
//! configuration, so connections and TLS sessions are reused between requests
//! while per-request overrides ([`ilixpatch_core::RequestSettings`]) still work.
//! Redirects are followed by the engine itself so each hop is recorded.
//! See `README.md` in this crate.

pub mod challenge;
pub mod error;
pub mod proxy;
pub mod settings;
pub mod tls;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ilixpatch_core::{Body, HttpVersionPref, Request};
use reqwest::header::{HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use challenge::{ChallengeAuth, body_bytes, full_url, with_auth_env};
pub use error::{ErrorContext, RequestError, RequestErrorKind, classify, detail_chain};
pub use proxy::{Bypass, HostPattern};
pub use reqwest::Url;
pub use settings::{
    ClientCertificate, NetworkSettings, ProxyMode, ProxySettings, TlsVersion, load_settings,
    save_settings,
};
pub use tls::{CertificateInfo, certificate_info};

/// `User-Agent` sent by default.
pub const USER_AGENT: &str = concat!("ilixpatch/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("invalid URL '{0}'")]
    InvalidUrl(String),
    #[error("{}", error_chain(.0))]
    Http(#[from] reqwest::Error),
    #[error("request timed out after {0} ms")]
    Timeout(u128),
    #[error("too many redirects (max {0})")]
    TooManyRedirects(u32),
    /// Invalid network settings; the message names the offending field.
    #[error("{0}")]
    Config(String),
    /// Authorization could not be computed (D39); names the auth type and field.
    #[error("{0}")]
    Auth(String),
}

/// `Display` of an error and its sources, without repetitions.
fn error_chain(e: &dyn std::error::Error) -> String {
    let mut s = e.to_string();
    let mut src = e.source();
    while let Some(inner) = src {
        let msg = inner.to_string();
        if !s.contains(&msg) {
            s.push_str(": ");
            s.push_str(&msg);
        }
        src = inner.source();
    }
    s
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedirectHop {
    pub status: u16,
    /// The `Location` followed (absolute).
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TlsDetails {
    pub peer_certificate: Option<CertificateInfo>,
    /// Whether the certificate chain and host name were verified.
    pub verified: bool,
}

/// Network details of a response (`ResponseNetworkInfo` in api.ts).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct NetworkInfo {
    /// `HTTP/1.1`, `HTTP/2`…
    pub http_version: String,
    pub remote_addr: Option<String>,
    pub redirects: Vec<RedirectHop>,
    pub final_url: String,
    /// Present for HTTPS responses.
    pub tls: Option<TlsDetails>,
    /// Proxy used (credentials stripped), if any.
    pub proxy: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Response {
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    #[serde(skip)]
    pub body: Vec<u8>,
    /// Time from sending the request until the full body was received
    /// (including every redirect hop).
    pub elapsed_ms: f64,
    pub size_bytes: usize,
    #[serde(skip)]
    pub network: NetworkInfo,
}

impl Response {
    pub fn body_text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    pub fn content_type(&self) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
            .map(|(_, v)| v.as_str())
    }
}

/// Key of the client cache: everything that needs a separate `reqwest::Client`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ClientKey {
    verify: bool,
    http1_only: bool,
    h2_prior_knowledge: bool,
    /// Index into `Inner::identities`.
    identity: Option<usize>,
}

/// Resolved proxy configuration.
enum ProxyPlan {
    System,
    None,
    Custom { url: reqwest::Url, bypass: Bypass },
}

struct Inner {
    settings: NetworkSettings,
    roots: Vec<reqwest::Certificate>,
    identities: Vec<(HostPattern, reqwest::Identity)>,
    proxy: ProxyPlan,
    clients: Mutex<HashMap<ClientKey, reqwest::Client>>,
}

/// Cheap to clone (shared client cache).
#[derive(Clone)]
pub struct Engine {
    inner: Arc<Inner>,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

/// The effective per-request configuration (global settings + overrides).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Effective {
    pub verify_ssl: bool,
    pub follow_redirects: bool,
    pub max_redirects: u32,
    pub timeout: Option<Duration>,
    pub http_version: HttpVersionPref,
}

fn http_version_name(v: reqwest::Version) -> String {
    match v {
        reqwest::Version::HTTP_09 => "HTTP/0.9",
        reqwest::Version::HTTP_10 => "HTTP/1.0",
        reqwest::Version::HTTP_11 => "HTTP/1.1",
        reqwest::Version::HTTP_2 => "HTTP/2",
        reqwest::Version::HTTP_3 => "HTTP/3",
        _ => "HTTP",
    }
    .to_string()
}

fn proxy_from_env(scheme: &str) -> Option<String> {
    let names: &[&str] = if scheme == "https" {
        &["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"]
    } else {
        &["HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy"]
    };
    names
        .iter()
        .find_map(|n| std::env::var(n).ok().filter(|v| !v.trim().is_empty()))
}

impl Engine {
    /// Default settings (verify TLS, system proxy, follow up to 10 redirects).
    pub fn new() -> Self {
        Self::with_settings(NetworkSettings::default()).expect("default settings are valid")
    }

    /// Build an engine, loading every certificate file. Errors name the
    /// offending settings field, e.g. `client_certificates[0].pfx_path: …`.
    pub fn with_settings(settings: NetworkSettings) -> Result<Self, EngineError> {
        let cfg = |field: String, e: String| EngineError::Config(format!("{field}: {e}"));

        let mut roots = Vec::new();
        for (i, path) in settings.ca_certificates.iter().enumerate() {
            roots.extend(
                tls::load_ca_file(path).map_err(|e| cfg(format!("ca_certificates[{i}]"), e))?,
            );
        }

        let mut identities = Vec::new();
        for (i, c) in settings.client_certificates.iter().enumerate() {
            if !c.enabled {
                continue;
            }
            let field = |f: &str| format!("client_certificates[{i}].{f}");
            let pattern = HostPattern::parse(&c.host).map_err(|e| cfg(field("host"), e))?;
            let nonempty = |s: &Option<String>| s.clone().filter(|s| !s.trim().is_empty());
            let pem = match (nonempty(&c.pfx_path), nonempty(&c.cert_path)) {
                (Some(pfx), _) => tls::identity_pem_from_pkcs12(&pfx, c.passphrase.as_deref())
                    .map_err(|e| cfg(field("pfx_path"), e))?,
                (None, Some(cert)) => tls::identity_pem_from_files(
                    &cert,
                    nonempty(&c.key_path).as_deref(),
                    c.passphrase.as_deref(),
                )
                .map_err(|e| cfg(field("cert_path"), e))?,
                (None, None) => {
                    return Err(cfg(
                        field("cert_path"),
                        "set cert_path (+ key_path) or pfx_path".into(),
                    ));
                }
            };
            let id =
                tls::identity_from_pem(&pem, "identity").map_err(|e| cfg(field("cert_path"), e))?;
            identities.push((pattern, id));
        }

        let proxy = match settings.proxy.mode {
            ProxyMode::System => ProxyPlan::System,
            ProxyMode::None => ProxyPlan::None,
            ProxyMode::Custom => {
                let raw = settings
                    .proxy
                    .url
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| cfg("proxy.url".into(), "required for a custom proxy".into()))?;
                let mut url = reqwest::Url::parse(raw)
                    .map_err(|e| cfg("proxy.url".into(), format!("'{raw}': {e}")))?;
                if !matches!(url.scheme(), "http" | "https" | "socks5" | "socks5h") {
                    return Err(cfg(
                        "proxy.url".into(),
                        format!(
                            "unsupported scheme '{}' (use http, https, socks5 or socks5h)",
                            url.scheme()
                        ),
                    ));
                }
                if url.host_str().is_none() {
                    return Err(cfg("proxy.url".into(), format!("'{raw}' has no host")));
                }
                if let Some(u) = settings.proxy.username.as_deref().filter(|u| !u.is_empty()) {
                    url.set_username(u).map_err(|_| {
                        cfg("proxy.username".into(), "cannot be set on this URL".into())
                    })?;
                    url.set_password(settings.proxy.password.as_deref())
                        .map_err(|_| {
                            cfg("proxy.password".into(), "cannot be set on this URL".into())
                        })?;
                }
                ProxyPlan::Custom {
                    url,
                    bypass: Bypass::parse(&settings.proxy.bypass),
                }
            }
        };

        let engine = Self {
            inner: Arc::new(Inner {
                settings,
                roots,
                identities,
                proxy,
                clients: Mutex::new(HashMap::new()),
            }),
        };
        // Build the default client now so configuration errors surface early.
        let key = ClientKey {
            verify: engine.inner.settings.verify_ssl,
            http1_only: engine.inner.settings.http_version == HttpVersionPref::Http1,
            h2_prior_knowledge: false,
            identity: None,
        };
        engine.client(key)?;
        for i in 0..engine.inner.identities.len() {
            engine.client(ClientKey {
                identity: Some(i),
                ..key
            })?;
        }
        Ok(engine)
    }

    pub fn settings(&self) -> &NetworkSettings {
        &self.inner.settings
    }

    /// A pooled client configured with the global network settings (proxy, extra CAs,
    /// TLS verification, HTTP version) and the client certificate matching `url`'s host.
    /// Used for side requests such as OAuth 2.0 token endpoints, so they behave exactly
    /// like regular sends (e.g. behind a corporate proxy).
    pub fn client_for_url(&self, url: &str) -> Result<reqwest::Client, EngineError> {
        let parsed =
            reqwest::Url::parse(url).map_err(|_| EngineError::InvalidUrl(url.to_string()))?;
        self.client(ClientKey {
            verify: self.inner.settings.verify_ssl,
            http1_only: self.inner.settings.http_version == HttpVersionPref::Http1,
            h2_prior_knowledge: false,
            identity: self.identity_for(&parsed),
        })
    }

    /// Merge the global settings, the caller's timeout and the request's
    /// `[settings]`. Precedence: request settings > `timeout` argument >
    /// global settings.
    pub fn effective(&self, req: &Request, timeout: Option<Duration>) -> Effective {
        let g = &self.inner.settings;
        let s = &req.settings;
        let ms = |t: u64| (t > 0).then(|| Duration::from_millis(t));
        Effective {
            verify_ssl: s.verify_ssl.unwrap_or(g.verify_ssl),
            follow_redirects: s.follow_redirects.unwrap_or(g.follow_redirects),
            max_redirects: s.max_redirects.unwrap_or(g.max_redirects),
            timeout: match s.timeout_ms {
                Some(t) => ms(t),
                None => timeout.or(ms(g.timeout_ms)),
            },
            http_version: s.http_version.unwrap_or(g.http_version),
        }
    }

    fn client(&self, key: ClientKey) -> Result<reqwest::Client, EngineError> {
        if let Some(c) = self
            .inner
            .clients
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&key)
        {
            return Ok(c.clone());
        }
        let client = self.build_client(key, false)?;
        self.inner
            .clients
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(key, client.clone());
        Ok(client)
    }

    /// Build a client. `connection_bound` (NTLM): HTTP/1.1 and at most one
    /// idle connection per host, so a handshake reuses the same socket.
    fn build_client(
        &self,
        key: ClientKey,
        connection_bound: bool,
    ) -> Result<reqwest::Client, EngineError> {
        let s = &self.inner.settings;
        let mut b = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .connect_timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .tls_info(true)
            .danger_accept_invalid_certs(!key.verify)
            .tls_built_in_native_certs(s.use_system_ca);
        for c in &self.inner.roots {
            b = b.add_root_certificate(c.clone());
        }
        if let Some(v) = s.min_tls_version {
            b = b.min_tls_version(match v {
                TlsVersion::Tls12 => reqwest::tls::Version::TLS_1_2,
                TlsVersion::Tls13 => reqwest::tls::Version::TLS_1_3,
            });
        }
        if key.http1_only || connection_bound {
            b = b.http1_only();
        }
        if connection_bound {
            b = b.pool_max_idle_per_host(1);
        }
        if key.h2_prior_knowledge {
            b = b.http2_prior_knowledge();
        }
        if let Some(i) = key.identity {
            b = b.identity(self.inner.identities[i].1.clone());
        }
        b = match &self.inner.proxy {
            ProxyPlan::System => b,
            ProxyPlan::None => b.no_proxy(),
            ProxyPlan::Custom { url, bypass } => {
                let (url, bypass) = (url.clone(), bypass.clone());
                b.proxy(reqwest::Proxy::custom(move |target| {
                    let host = target.host_str()?;
                    (!bypass.matches(host, target.port_or_known_default())).then(|| url.clone())
                }))
            }
        };
        b.build()
            .map_err(|e| EngineError::Config(format!("HTTP client: {}", error_chain(&e))))
    }

    /// The client identity (index) for a URL: first enabled pattern that matches.
    fn identity_for(&self, url: &reqwest::Url) -> Option<usize> {
        let host = url.host_str()?;
        let port = url.port_or_known_default();
        self.inner
            .identities
            .iter()
            .position(|(p, _)| p.matches(host, port))
    }

    /// Describe a failed [`Engine::send_with_timeout`] of `req` (D36): kind,
    /// short message, full chain and facts (host, port, timeout, proxy).
    pub fn describe_error(
        &self,
        err: &EngineError,
        req: &Request,
        timeout: Option<Duration>,
    ) -> RequestError {
        let eff = self.effective(req, timeout);
        let url = reqwest::Url::parse(&req.url).ok();
        let proxy = url.as_ref().and_then(|u| self.proxy_for(u));
        error::classify(
            err,
            &ErrorContext {
                url: &req.url,
                timeout: eff.timeout,
                proxy,
            },
        )
    }

    /// The proxy a URL goes through, credentials stripped (best effort for
    /// the system mode: environment variables only).
    pub fn proxy_for(&self, url: &reqwest::Url) -> Option<String> {
        let host = url.host_str()?;
        let port = url.port_or_known_default();
        match &self.inner.proxy {
            ProxyPlan::None => None,
            ProxyPlan::Custom { url: p, bypass } => {
                (!bypass.matches(host, port)).then(|| proxy::strip_credentials(p))
            }
            ProxyPlan::System => {
                let raw = proxy_from_env(url.scheme())?;
                let no_proxy = ["NO_PROXY", "no_proxy"]
                    .iter()
                    .find_map(|n| std::env::var(n).ok())
                    .unwrap_or_default();
                if Bypass::parse(&[no_proxy]).matches(host, port) {
                    return None;
                }
                let with_scheme = if raw.contains("://") {
                    raw
                } else {
                    format!("http://{raw}")
                };
                reqwest::Url::parse(&with_scheme)
                    .ok()
                    .map(|u| proxy::strip_credentials(&u))
            }
        }
    }

    /// Send a request. Variables must already be resolved (see
    /// [`ilixpatch_core::Scope::resolve_request`]).
    pub async fn send(&self, req: &Request) -> Result<Response, EngineError> {
        self.send_with_timeout(req, None).await
    }

    /// Like [`Engine::send`], with a default total timeout for this request
    /// (overridden by the request's own `settings.timeout_ms`).
    pub async fn send_with_timeout(
        &self,
        req: &Request,
        timeout: Option<Duration>,
    ) -> Result<Response, EngineError> {
        self.send_inner(req, timeout, None).await
    }

    /// Send with a challenge–response auth (Digest, NTLM); see [`ChallengeAuth`].
    pub async fn send_with_auth(
        &self,
        req: &Request,
        timeout: Option<Duration>,
        auth: &ChallengeAuth,
    ) -> Result<Response, EngineError> {
        challenge::send(self, req, timeout, auth).await
    }

    /// `dedicated`: per-exchange clients (NTLM) instead of the shared cache.
    pub(crate) async fn send_inner(
        &self,
        req: &Request,
        timeout: Option<Duration>,
        dedicated: Option<&Mutex<HashMap<ClientKey, reqwest::Client>>>,
    ) -> Result<Response, EngineError> {
        let eff = self.effective(req, timeout);
        let mut url =
            reqwest::Url::parse(&req.url).map_err(|_| EngineError::InvalidUrl(req.url.clone()))?;
        if url.host_str().is_none() {
            return Err(EngineError::InvalidUrl(req.url.clone()));
        }
        if !req.query.is_empty() {
            url.query_pairs_mut()
                .extend_pairs(req.query.iter().map(|(k, v)| (k.as_str(), v.as_str())));
        }
        let mut method = reqwest::Method::from_bytes(req.method.as_str().as_bytes())
            .expect("HttpMethod is always a valid method");
        let mut headers: Vec<(String, String)> = req
            .headers
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let mut body = req.body.clone();
        let mut redirects = Vec::new();

        let start = Instant::now();
        let remaining = |start: Instant| -> Result<Option<Duration>, EngineError> {
            match eff.timeout {
                None => Ok(None),
                Some(t) => t
                    .checked_sub(start.elapsed())
                    .filter(|d| !d.is_zero())
                    .map(Some)
                    .ok_or(EngineError::Timeout(t.as_millis())),
            }
        };
        let map_err = |e: reqwest::Error| match eff.timeout {
            Some(t) if e.is_timeout() => EngineError::Timeout(t.as_millis()),
            _ => EngineError::Http(e),
        };

        let resp = loop {
            let key = ClientKey {
                verify: eff.verify_ssl,
                http1_only: eff.http_version == HttpVersionPref::Http1,
                h2_prior_knowledge: eff.http_version == HttpVersionPref::Http2
                    && url.scheme() == "http",
                identity: self.identity_for(&url),
            };
            let client = match dedicated {
                None => self.client(key)?,
                Some(pool) => {
                    let existing = pool
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .get(&key)
                        .cloned();
                    match existing {
                        Some(c) => c,
                        None => {
                            let c = self.build_client(key, true)?;
                            pool.lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .insert(key, c.clone());
                            c
                        }
                    }
                }
            };
            let mut builder = client.request(method.clone(), url.clone());
            if let Some(t) = remaining(start)? {
                builder = builder.timeout(t);
            }
            let has_content_type = headers
                .iter()
                .any(|(k, _)| k.eq_ignore_ascii_case("content-type"));
            for (k, v) in &headers {
                let name = HeaderName::from_bytes(k.as_bytes())
                    .map_err(|_| EngineError::Config(format!("invalid header name '{k}'")))?;
                let value = HeaderValue::from_str(v)
                    .map_err(|_| EngineError::Config(format!("invalid value for header '{k}'")))?;
                builder = builder.header(name, value);
            }
            builder = match &body {
                None => builder,
                Some(Body::Json { content }) => {
                    let b = builder.body(content.clone());
                    if has_content_type {
                        b
                    } else {
                        b.header("content-type", "application/json")
                    }
                }
                Some(Body::Text { content }) => builder.body(content.clone()),
                Some(Body::Form { fields }) => builder.form(fields),
            };
            let resp = builder.send().await.map_err(map_err)?;
            let status = resp.status();
            let location = resp
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|l| l.to_str().ok())
                .map(str::to_string);
            let (true, Some(location)) =
                (eff.follow_redirects && status.is_redirection(), location)
            else {
                break resp;
            };
            if !matches!(status.as_u16(), 301 | 302 | 303 | 307 | 308) {
                break resp;
            }
            if redirects.len() as u32 >= eff.max_redirects {
                return Err(EngineError::TooManyRedirects(eff.max_redirects));
            }
            let next = url
                .join(&location)
                .map_err(|_| EngineError::InvalidUrl(location.clone()))?;
            redirects.push(RedirectHop {
                status: status.as_u16(),
                url: next.to_string(),
            });
            // 301/302 on POST and 303 on anything but HEAD become GET without body.
            let to_get = match status.as_u16() {
                303 => method != reqwest::Method::HEAD,
                301 | 302 => method == reqwest::Method::POST,
                _ => false,
            };
            if to_get {
                method = reqwest::Method::GET;
                body = None;
                headers.retain(|(k, _)| {
                    !k.eq_ignore_ascii_case("content-type")
                        && !k.eq_ignore_ascii_case("content-length")
                });
            }
            // Never leak credentials to another origin.
            if next.origin() != url.origin() {
                headers.retain(|(k, _)| {
                    !["authorization", "cookie", "proxy-authorization"]
                        .iter()
                        .any(|h| k.eq_ignore_ascii_case(h))
                });
            }
            url = next;
        };

        let status = resp.status();
        let tls = (resp.url().scheme() == "https").then(|| TlsDetails {
            peer_certificate: resp
                .extensions()
                .get::<reqwest::tls::TlsInfo>()
                .and_then(|i| i.peer_certificate())
                .and_then(certificate_info),
            verified: eff.verify_ssl,
        });
        let network = NetworkInfo {
            http_version: http_version_name(resp.version()),
            remote_addr: resp.remote_addr().map(|a| a.to_string()),
            redirects,
            final_url: resp.url().to_string(),
            tls,
            proxy: self.proxy_for(resp.url()),
        };
        let headers = resp
            .headers()
            .iter()
            .map(|(k, v)| {
                (
                    k.to_string(),
                    String::from_utf8_lossy(v.as_bytes()).into_owned(),
                )
            })
            .collect();
        let body = resp.bytes().await.map_err(map_err)?.to_vec();
        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;

        Ok(Response {
            status: status.as_u16(),
            status_text: status.canonical_reason().unwrap_or_default().to_string(),
            headers,
            size_bytes: body.len(),
            body,
            elapsed_ms,
            network,
        })
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod auth_tests;
