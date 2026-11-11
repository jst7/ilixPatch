//! Descriptive request errors (D36): turn a failed send into a stable
//! [`RequestErrorKind`] plus the facts a UI or CLI needs to explain the
//! likely cause (`RequestError` in api.ts).

use std::error::Error as StdError;
use std::io;
use std::time::Duration;

use ilixpatch_core::VarError;
use rustls::{AlertDescription, CertificateError};
use serde::{Deserialize, Serialize};

use crate::EngineError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestErrorKind {
    InvalidUrl,
    UndefinedVariables,
    Dns,
    ConnectionRefused,
    ConnectionReset,
    Timeout,
    TlsUnknownIssuer,
    TlsExpired,
    TlsHostnameMismatch,
    TlsHandshake,
    TlsClientCertificate,
    Proxy,
    TooManyRedirects,
    Body,
    SkippedByScript,
    Cancelled,
    Other,
}

impl RequestErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidUrl => "invalid_url",
            Self::UndefinedVariables => "undefined_variables",
            Self::Dns => "dns",
            Self::ConnectionRefused => "connection_refused",
            Self::ConnectionReset => "connection_reset",
            Self::Timeout => "timeout",
            Self::TlsUnknownIssuer => "tls_unknown_issuer",
            Self::TlsExpired => "tls_expired",
            Self::TlsHostnameMismatch => "tls_hostname_mismatch",
            Self::TlsHandshake => "tls_handshake",
            Self::TlsClientCertificate => "tls_client_certificate",
            Self::Proxy => "proxy",
            Self::TooManyRedirects => "too_many_redirects",
            Self::Body => "body",
            Self::SkippedByScript => "skipped_by_script",
            Self::Cancelled => "cancelled",
            Self::Other => "other",
        }
    }
}

/// A failed send, classified (`RequestError` in api.ts).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestError {
    pub kind: RequestErrorKind,
    /// Short English summary.
    pub message: String,
    /// Full technical chain, sources joined with " → ".
    pub detail: String,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub url: Option<String>,
    pub timeout_ms: Option<u64>,
    /// For `undefined_variables`.
    pub variables: Option<Vec<String>>,
    /// Proxy in use (credentials stripped).
    pub proxy: Option<String>,
}

impl std::fmt::Display for RequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for RequestError {}

impl RequestError {
    pub fn new(
        kind: RequestErrorKind,
        message: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            message: message.into(),
            detail: detail.into(),
            host: None,
            port: None,
            url: None,
            timeout_ms: None,
            variables: None,
            proxy: None,
        }
    }

    /// Fill `url`, `host` and `port` from a URL string (ignored if it does not parse).
    pub fn with_url(mut self, url: &str) -> Self {
        self.url = Some(url.to_string());
        if let Ok(u) = reqwest::Url::parse(url) {
            self.host = u.host_str().map(|h| h.trim_matches(['[', ']']).to_string());
            self.port = u.port_or_known_default();
        }
        self
    }

    pub fn cancelled() -> Self {
        Self::new(
            RequestErrorKind::Cancelled,
            "The request was cancelled.",
            "cancelled",
        )
    }

    pub fn skipped_by_script() -> Self {
        Self::new(
            RequestErrorKind::SkippedByScript,
            "The request was skipped by its pre-request script.",
            "pm.execution.skipRequest() was called in the pre-request script",
        )
    }

    /// A `{{variable}}` resolution failure.
    pub fn from_var_error(e: &VarError, url: &str) -> Self {
        let mut err = match e {
            VarError::Undefined(vars) => {
                let mut r = Self::new(
                    RequestErrorKind::UndefinedVariables,
                    format!(
                        "Undefined variable{}: {}.",
                        if vars.len() == 1 { "" } else { "s" },
                        vars.join(", ")
                    ),
                    e.to_string(),
                );
                r.variables = Some(vars.clone());
                r
            }
            VarError::Unclosed(_) => Self::new(
                RequestErrorKind::Other,
                "A '{{' is never closed with '}}'.",
                e.to_string(),
            ),
        };
        // The URL may contain the unresolved `{{var}}`: keep it as text only.
        err.url = Some(url.to_string());
        err
    }
}

/// `Display` of an error and its sources (including errors wrapped inside
/// `io::Error`), without repetitions, joined with " → ".
pub fn detail_chain(e: &(dyn StdError + 'static)) -> String {
    let mut parts: Vec<String> = Vec::new();
    for err in chain(e) {
        let msg = err.to_string();
        if !msg.is_empty() && !parts.iter().any(|p| p.contains(&msg)) {
            parts.push(msg);
        }
    }
    parts.join(" → ")
}

/// Every error in the chain. `io::Error::source()` skips the wrapped error,
/// so it is visited explicitly.
fn chain<'a>(e: &'a (dyn StdError + 'static)) -> Vec<&'a (dyn StdError + 'static)> {
    let mut out = Vec::new();
    let mut stack = vec![e];
    while let Some(cur) = stack.pop() {
        if out.len() > 32 {
            break;
        }
        out.push(cur);
        if let Some(io) = cur.downcast_ref::<io::Error>()
            && let Some(inner) = io.get_ref()
        {
            let inner: &(dyn StdError + 'static) = inner;
            if let Some(src) = cur.source() {
                stack.push(src);
            }
            stack.push(inner);
            continue;
        }
        if let Some(src) = cur.source() {
            stack.push(src);
        }
    }
    out
}

fn classify_rustls(e: &rustls::Error) -> RequestErrorKind {
    use RequestErrorKind as K;
    use rustls::Error as E;
    match e {
        E::InvalidCertificate(CertificateError::UnknownIssuer) => K::TlsUnknownIssuer,
        E::InvalidCertificate(
            CertificateError::Expired
            | CertificateError::ExpiredContext { .. }
            | CertificateError::NotValidYet
            | CertificateError::NotValidYetContext { .. },
        ) => K::TlsExpired,
        E::InvalidCertificate(
            CertificateError::NotValidForName | CertificateError::NotValidForNameContext { .. },
        ) => K::TlsHostnameMismatch,
        E::NoCertificatesPresented
        | E::AlertReceived(
            AlertDescription::CertificateRequired
            | AlertDescription::BadCertificate
            | AlertDescription::UnknownCA
            | AlertDescription::CertificateUnknown
            | AlertDescription::CertificateRevoked
            | AlertDescription::CertificateExpired
            | AlertDescription::UnsupportedCertificate
            | AlertDescription::AccessDenied,
        ) => K::TlsClientCertificate,
        _ => K::TlsHandshake,
    }
}

/// Facts about the request being classified.
#[derive(Debug, Clone, Default)]
pub struct ErrorContext<'a> {
    /// The URL that was being requested (the failing hop).
    pub url: &'a str,
    pub timeout: Option<Duration>,
    /// Proxy used for this URL (credentials stripped).
    pub proxy: Option<String>,
}

fn host_port(err: &RequestError) -> String {
    match (&err.host, err.port) {
        (Some(h), Some(p)) => format!("{h}:{p}"),
        (Some(h), None) => h.clone(),
        _ => "the server".into(),
    }
}

/// Classify an engine error.
pub fn classify(e: &EngineError, ctx: &ErrorContext<'_>) -> RequestError {
    use RequestErrorKind as K;
    // `EngineError::Http`'s own Display already concatenates the sources;
    // start from the reqwest error so each source is its own step.
    let detail = match e {
        EngineError::Http(h) => detail_chain(h),
        other => detail_chain(other),
    };
    let base = |kind: K, message: String| {
        let mut r = RequestError::new(kind, message, detail.clone()).with_url(ctx.url);
        r.proxy = ctx.proxy.clone();
        r
    };
    match e {
        EngineError::InvalidUrl(u) => {
            return base(K::InvalidUrl, format!("'{u}' is not a valid absolute URL."));
        }
        EngineError::Timeout(ms) => {
            let mut r = base(K::Timeout, format!("The request timed out after {ms} ms."));
            r.timeout_ms = u64::try_from(*ms).ok();
            return r;
        }
        EngineError::TooManyRedirects(n) => {
            return base(
                K::TooManyRedirects,
                format!("Stopped after {n} redirects (redirect limit reached)."),
            );
        }
        EngineError::Config(m) => {
            let kind = if m.starts_with("client_certificates") {
                K::TlsClientCertificate
            } else if m.starts_with("invalid header") || m.starts_with("invalid value for header") {
                K::InvalidUrl
            } else {
                K::Other
            };
            return base(kind, format!("{m}."));
        }
        EngineError::Http(_) => {}
    }

    let errors = chain(e);
    let reqwest_err = errors
        .iter()
        .find_map(|x| x.downcast_ref::<reqwest::Error>());
    let lower = detail.to_lowercase();

    let mut kind = None;
    // TLS: the most specific information.
    for x in &errors {
        if let Some(t) = x.downcast_ref::<rustls::Error>() {
            kind = Some(classify_rustls(t));
            break;
        }
    }
    if kind.is_none() && reqwest_err.is_some_and(|r| r.is_timeout()) {
        kind = Some(K::Timeout);
    }
    if kind.is_none() {
        for x in &errors {
            if let Some(io) = x.downcast_ref::<io::Error>() {
                kind = match io.kind() {
                    io::ErrorKind::ConnectionRefused => Some(K::ConnectionRefused),
                    io::ErrorKind::ConnectionReset
                    | io::ErrorKind::ConnectionAborted
                    | io::ErrorKind::BrokenPipe
                    | io::ErrorKind::UnexpectedEof => Some(K::ConnectionReset),
                    io::ErrorKind::TimedOut => Some(K::Timeout),
                    _ => None,
                };
                if kind.is_some() {
                    break;
                }
            }
        }
    }
    if kind.is_none() {
        kind = if lower.contains("dns error")
            || lower.contains("failed to lookup address")
            || lower.contains("nodename nor servname")
            || lower.contains("name or service not known")
            || lower.contains("no such host")
        {
            Some(K::Dns)
        } else if lower.contains("tunnel") || lower.contains("proxy") || lower.contains("socks") {
            Some(K::Proxy)
        } else if lower.contains("certificate")
            || lower.contains("handshake")
            || lower.contains("tls")
        {
            Some(K::TlsHandshake)
        } else if lower.contains("connection closed")
            || lower.contains("incompletemessage")
            || lower.contains("connection reset")
        {
            Some(K::ConnectionReset)
        } else if reqwest_err.is_some_and(|r| r.is_body() || r.is_decode()) {
            Some(K::Body)
        } else if reqwest_err.is_some_and(|r| r.is_builder()) {
            Some(K::InvalidUrl)
        } else {
            None
        };
    }
    let mut kind = kind.unwrap_or(K::Other);

    // A connection-level failure while a proxy is in use is (most likely) the proxy's.
    if ctx.proxy.is_some() && matches!(kind, K::ConnectionRefused | K::ConnectionReset | K::Dns) {
        kind = K::Proxy;
    }

    let mut r = base(kind, String::new());
    if kind == K::Timeout {
        r.timeout_ms = ctx.timeout.and_then(|t| u64::try_from(t.as_millis()).ok());
    }
    let hp = host_port(&r);
    let host = r.host.clone().unwrap_or_else(|| "the host".into());
    r.message = match kind {
        K::Dns => format!("Could not resolve host '{host}'."),
        K::ConnectionRefused => format!("Connection refused by {hp}."),
        K::ConnectionReset => format!("The connection to {hp} was closed unexpectedly."),
        K::Timeout => match r.timeout_ms {
            Some(ms) => format!("The request timed out after {ms} ms."),
            None => format!("The connection to {hp} timed out."),
        },
        K::TlsUnknownIssuer => {
            format!("The certificate of {host} is not signed by a trusted authority.")
        }
        K::TlsExpired => format!("The certificate of {host} is expired or not yet valid."),
        K::TlsHostnameMismatch => {
            format!("The certificate of {host} is not valid for this host name.")
        }
        K::TlsClientCertificate => {
            format!("{host} did not accept the client certificate (or none was sent).")
        }
        K::TlsHandshake => format!("The TLS handshake with {hp} failed."),
        K::Proxy => match &r.proxy {
            Some(p) => format!("Could not connect through the proxy {p}."),
            None => "Could not connect through the proxy.".into(),
        },
        K::Body => "The response body could not be read.".into(),
        K::InvalidUrl => "The request could not be built (invalid URL or header).".into(),
        _ => "The request failed.".into(),
    };
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_shape_matches_api_ts() {
        let mut e = RequestError::new(RequestErrorKind::UndefinedVariables, "m", "d")
            .with_url("https://[::1]:8443/x");
        e.variables = Some(vec!["a".into()]);
        let v = serde_json::to_value(&e).unwrap();
        let mut keys: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "detail",
                "host",
                "kind",
                "message",
                "port",
                "proxy",
                "timeout_ms",
                "url",
                "variables"
            ]
        );
        assert_eq!(v["kind"], "undefined_variables");
        assert_eq!(v["host"], "::1");
        assert_eq!(v["port"], 8443);
        assert_eq!(
            serde_json::to_value(RequestErrorKind::TlsClientCertificate).unwrap(),
            RequestErrorKind::TlsClientCertificate.as_str()
        );
    }

    #[test]
    fn var_errors() {
        let e = RequestError::from_var_error(
            &VarError::Undefined(vec!["a".into(), "b".into()]),
            "{{base}}/x",
        );
        assert_eq!(e.kind, RequestErrorKind::UndefinedVariables);
        assert_eq!(
            e.variables.as_deref(),
            Some(&["a".to_string(), "b".to_string()][..])
        );
        assert_eq!(e.message, "Undefined variables: a, b.");
        assert!(e.host.is_none());
    }

    #[test]
    fn io_inner_errors_are_visited() {
        let inner = rustls::Error::InvalidCertificate(CertificateError::UnknownIssuer);
        let io = io::Error::other(inner);
        let kinds: Vec<_> = chain(&io)
            .into_iter()
            .filter_map(|e| e.downcast_ref::<rustls::Error>().map(classify_rustls))
            .collect();
        assert_eq!(kinds, [RequestErrorKind::TlsUnknownIssuer]);
        assert!(
            detail_chain(&io).contains("UnknownIssuer")
                || detail_chain(&io).contains("unknown issuer")
        );
    }
}
