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
pub mod netlog;
pub mod proxy;
pub mod settings;
pub mod timing;
pub mod tls;
pub mod update;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ilixpatch_core::{Body, HttpVersionPref, Request};
use reqwest::header::{HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use challenge::{ChallengeAuth, body_bytes, full_url, with_auth_env};
pub use error::{ErrorContext, RequestError, RequestErrorKind, classify, detail_chain};
pub use ilixpatch_bodystore::{self as bodystore, Body as ResponseBody, BodySink, SinkConfig};
pub use netlog::{BodyPreview, NetExchange, NetObserver, Trace};
pub use proxy::{Bypass, HostPattern};
pub use reqwest::Url;
pub use settings::{
    ClientCertificate, NetworkSettings, ProxyMode, ProxySettings, TlsVersion, load_settings,
    save_settings,
};
pub use tls::{CertificateInfo, certificate_info};
pub use update::{LatestRelease, UpdateCheck};

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
    /// The body could not be stored (temp file I/O, D41).
    #[error("could not store the response body: {0}")]
    Body(Box<std::io::Error>),
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
    /// D41: in memory up to `inline_body_kb`, else a temp file (deleted
    /// with the last clone). Empty when the body was discarded.
    #[serde(skip)]
    pub body: ResponseBody,
    /// `false` when the download stopped at `max_body_mb`.
    #[serde(skip)]
    pub body_complete: bool,
    /// Time from sending the request until the full body was received
    /// (including every redirect hop).
    pub elapsed_ms: f64,
    /// Bytes received (also when the body was discarded).
    pub size_bytes: usize,
    #[serde(skip)]
    pub network: NetworkInfo,
}

impl Response {
    /// The whole body as text. Reads a spilled body completely: for small
    /// bodies and tests; consumers use [`Body::text_limited`].
    pub fn body_text(&self) -> String {
        self.body
            .text_limited(usize::MAX)
            .map(|(t, _)| t)
            .unwrap_or_default()
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
#[derive(Clone)]
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
    /// D41: where large bodies are spilled (system temp when `None`).
    spill_dir: Option<std::path::PathBuf>,
    /// D46: network console; called for every exchange of a traced send.
    observer: Option<NetObserver>,
}

/// Per-send options.
#[derive(Debug, Clone, Default)]
pub struct SendOptions {
    /// Default total timeout (the request's own `settings.timeout_ms` wins).
    pub timeout: Option<Duration>,
    /// Count the body bytes but don't keep them (load tests, runs without tests).
    pub discard_body: bool,
    /// D46: report the exchanges of this send to the engine's observer under
    /// this id (`None`: not reported, e.g. load tests).
    pub trace: Option<Arc<netlog::Trace>>,
    /// D46: what this send is part of (`ntlm-negotiate`…), for the console.
    pub phase: netlog::Phase,
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
                spill_dir: None,
                observer: None,
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

    /// Download a document, such as an OpenAPI spec to import (D53): a GET
    /// with the same network settings as regular sends (proxy, CAs, client
    /// certificate), following redirects (the pooled clients don't), failing
    /// on non-2xx statuses and bodies over `max_bytes`.
    pub async fn download(&self, url: &str, max_bytes: u64) -> Result<Vec<u8>, String> {
        const MAX_REDIRECTS: u32 = 10;
        let mut current = reqwest::Url::parse(url)
            .map_err(|_| EngineError::InvalidUrl(url.into()).to_string())?;
        let mut redirects = 0;
        let mut resp = loop {
            let client = self
                .client_for_url(current.as_str())
                .map_err(|e| e.to_string())?;
            let resp = client
                .get(current.clone())
                .header(
                    reqwest::header::ACCEPT,
                    "application/json, application/yaml;q=0.9, text/yaml;q=0.9, */*;q=0.5",
                )
                .timeout(std::time::Duration::from_secs(60))
                .send()
                .await
                .map_err(|e| EngineError::from(e).to_string())?;
            let next = resp
                .status()
                .is_redirection()
                .then(|| resp.headers().get(reqwest::header::LOCATION))
                .flatten()
                .and_then(|l| l.to_str().ok())
                .and_then(|l| current.join(l).ok());
            match next {
                Some(_) if redirects == MAX_REDIRECTS => {
                    return Err(EngineError::TooManyRedirects(MAX_REDIRECTS).to_string());
                }
                Some(next) => {
                    redirects += 1;
                    current = next;
                }
                None => break resp,
            }
        };
        let status = resp.status();
        if !status.is_success() {
            return Err(format!("HTTP {status}"));
        }
        let too_large = || format!("larger than {} MB", max_bytes / (1024 * 1024));
        if resp.content_length().is_some_and(|n| n > max_bytes) {
            return Err(too_large());
        }
        let mut out = Vec::new();
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| EngineError::from(e).to_string())?
        {
            if (out.len() + chunk.len()) as u64 > max_bytes {
                return Err(too_large());
            }
            out.extend_from_slice(&chunk);
        }
        Ok(out)
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
            // D46 phase 2: DNS and connection timings for the network console.
            .dns_resolver(Arc::new(timing::TimedResolver))
            .connector_layer(timing::TimingLayer)
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
        self.send_opts(
            req,
            SendOptions {
                timeout,
                ..Default::default()
            },
            None,
        )
        .await
    }

    /// Send with a challenge–response auth (Digest, NTLM); see [`ChallengeAuth`].
    pub async fn send_with_auth(
        &self,
        req: &Request,
        timeout: Option<Duration>,
        auth: &ChallengeAuth,
    ) -> Result<Response, EngineError> {
        self.send_opts(
            req,
            SendOptions {
                timeout,
                ..Default::default()
            },
            Some(auth),
        )
        .await
    }

    /// The general send: options plus an optional challenge–response auth.
    /// The body is streamed: kept in memory up to `inline_body_kb`, then
    /// spilled to a temp file, and cut at `max_body_mb`. Dropping the future
    /// (cancel) closes the connection and deletes a partial temp file.
    pub async fn send_opts(
        &self,
        req: &Request,
        opts: SendOptions,
        auth: Option<&ChallengeAuth>,
    ) -> Result<Response, EngineError> {
        match auth {
            None => self.send_inner(req, opts, None).await,
            Some(a) => challenge::send(self, req, opts, a).await,
        }
    }

    fn modify(self, f: impl FnOnce(&mut Inner)) -> Self {
        let mut inner = Arc::try_unwrap(self.inner).unwrap_or_else(|arc| Inner {
            settings: arc.settings.clone(),
            roots: arc.roots.clone(),
            identities: arc.identities.clone(),
            proxy: arc.proxy.clone(),
            clients: Mutex::new(HashMap::new()),
            spill_dir: arc.spill_dir.clone(),
            observer: arc.observer.clone(),
        });
        f(&mut inner);
        Self {
            inner: Arc::new(inner),
        }
    }

    /// Where large bodies of this engine are spilled.
    pub fn with_spill_dir(self, dir: impl Into<std::path::PathBuf>) -> Self {
        let dir = dir.into();
        self.modify(|i| i.spill_dir = Some(dir))
    }

    /// D46: report every exchange of traced sends (`SendOptions::trace`).
    pub fn with_observer(self, observer: NetObserver) -> Self {
        self.modify(|i| i.observer = Some(observer))
    }

    /// The observer, for sends that set a trace id.
    fn observer_for<'s>(
        &'s self,
        opts: &'s SendOptions,
    ) -> Option<(&'s NetObserver, &'s netlog::Trace)> {
        Some((self.inner.observer.as_ref()?, opts.trace.as_deref()?))
    }

    fn sink_config(&self, discard: bool) -> SinkConfig {
        let s = &self.inner.settings;
        SinkConfig {
            inline_limit: (s.inline_body_kb.saturating_mul(1024)).min(usize::MAX as u64) as usize,
            max_bytes: (s.max_body_mb > 0).then(|| s.max_body_mb.saturating_mul(1024 * 1024)),
            dir: self.inner.spill_dir.clone(),
            discard,
        }
    }

    /// `dedicated`: per-exchange clients (NTLM) instead of the shared cache.
    pub(crate) async fn send_inner(
        &self,
        req: &Request,
        opts: SendOptions,
        dedicated: Option<&Mutex<HashMap<ClientKey, reqwest::Client>>>,
    ) -> Result<Response, EngineError> {
        let timeout = opts.timeout;
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

        let (resp, pending, hop_start) = loop {
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
            let request = builder.build().map_err(map_err)?;
            let observed = self.observer_for(&opts);
            let captured = observed.map(|_| netlog::capture_request(&request, Some(USER_AGENT)));
            let hop_start = Instant::now();
            let (sent, conn_timing) = timing::timed(client.execute(request)).await;
            let hop_timings =
                netlog::Timings::from_conn(conn_timing, hop_start.elapsed().as_secs_f64() * 1000.0);
            let resp = match sent {
                Ok(r) => r,
                Err(e) => {
                    let err = map_err(e);
                    if let (Some((obs, trace)), Some((h, b))) = (observed, captured) {
                        obs(NetExchange {
                            trace: Some(trace.id),
                            label: trace.label.clone(),
                            phase: opts.phase.map(str::to_string),
                            timestamp_ms: netlog::now_ms(),
                            method: method.to_string(),
                            url: url.to_string(),
                            request_headers: h,
                            request_body: b,
                            status: None,
                            status_text: None,
                            response_headers: Vec::new(),
                            response_body: None,
                            http_version: None,
                            remote_addr: None,
                            proxy: self.proxy_for(&url),
                            elapsed_ms: hop_start.elapsed().as_secs_f64() * 1000.0,
                            timings: Some(hop_timings.clone()),
                            error: Some(detail_chain(&err)),
                        });
                    }
                    return Err(err);
                }
            };
            let status = resp.status();
            // The exchange is reported when we know whether it is a redirect hop.
            let mut pending = observed.zip(captured).map(|((obs, trace), (h, b))| {
                (
                    obs,
                    NetExchange {
                        trace: Some(trace.id),
                        label: trace.label.clone(),
                        phase: opts.phase.map(str::to_string),
                        timestamp_ms: netlog::now_ms(),
                        method: method.to_string(),
                        url: url.to_string(),
                        request_headers: h,
                        request_body: b,
                        status: Some(status.as_u16()),
                        status_text: status.canonical_reason().map(str::to_string),
                        response_headers: netlog::response_headers(&resp),
                        response_body: None,
                        http_version: Some(http_version_name(resp.version())),
                        remote_addr: resp.remote_addr().map(|a| a.to_string()),
                        proxy: self.proxy_for(&url),
                        elapsed_ms: 0.0,
                        timings: Some(hop_timings),
                        error: None,
                    },
                )
            });
            let location = resp
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|l| l.to_str().ok())
                .map(str::to_string);
            let (true, Some(location)) =
                (eff.follow_redirects && status.is_redirection(), location)
            else {
                break (resp, pending, hop_start);
            };
            if !matches!(status.as_u16(), 301 | 302 | 303 | 307 | 308) {
                break (resp, pending, hop_start);
            }
            if let Some((obs, mut ex)) = pending.take() {
                ex.phase = Some("redirect".into());
                ex.elapsed_ms = hop_start.elapsed().as_secs_f64() * 1000.0;
                obs(ex);
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
        // D41: stream the body chunk by chunk (decompressed by reqwest).
        let mut resp = resp;
        let download_start = Instant::now();
        let mut sink = BodySink::new(self.sink_config(opts.discard_body));
        while let Some(chunk) = resp.chunk().await.map_err(map_err)? {
            if !sink
                .push(&chunk)
                .map_err(|e| EngineError::Body(Box::new(e)))?
            {
                break; // max_body_mb reached
            }
        }
        // Closes the connection if the body was cut.
        drop(resp);
        let size = sink.len();
        let (body, body_complete) = sink.finish().map_err(|e| EngineError::Body(Box::new(e)))?;
        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
        if let Some((obs, mut ex)) = pending {
            ex.elapsed_ms = hop_start.elapsed().as_secs_f64() * 1000.0;
            if let Some(t) = ex.timings.as_mut() {
                t.download_ms = Some(download_start.elapsed().as_secs_f64() * 1000.0);
            }
            ex.response_body = (size > 0).then(|| {
                let head = body.preview(netlog::BODY_PREVIEW).unwrap_or_default();
                let mut p = BodyPreview::from_bytes(&head, size);
                if opts.discard_body {
                    p.text = String::new();
                }
                p
            });
            obs(ex);
        }

        Ok(Response {
            status: status.as_u16(),
            status_text: status.canonical_reason().unwrap_or_default().to_string(),
            headers,
            size_bytes: size.min(usize::MAX as u64) as usize,
            body,
            body_complete,
            elapsed_ms,
            network,
        })
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod auth_tests;
#[cfg(test)]
mod large_body_tests;
