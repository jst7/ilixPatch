//! Short English explanations and hints for classified send failures (D36).

use ilixpatch_runner::{RequestError, RequestErrorKind as K};

/// `(title, likely cause, hint)` for a failed send.
pub fn explain(e: &RequestError) -> (&'static str, String, &'static str) {
    let host = e.host.as_deref().unwrap_or("the host");
    let hp = match (&e.host, e.port) {
        (Some(h), Some(p)) => format!("{h}:{p}"),
        _ => host.to_string(),
    };
    match e.kind {
        K::InvalidUrl => (
            "invalid URL",
            format!("'{}' could not be used", e.url.as_deref().unwrap_or("")),
            "Use an absolute URL with a scheme, e.g. https://api.example.com/path (check headers too).",
        ),
        K::UndefinedVariables => (
            "undefined variables",
            format!(
                "{} not defined in any scope",
                e.variables.as_deref().unwrap_or_default().join(", ")
            ),
            "Select an environment (-e), define them in the collection, or pass --env-var key=value.",
        ),
        K::Dns => (
            "DNS lookup failed",
            format!("could not resolve {host}"),
            "Check the host name for typos, your network/VPN, or the environment's base URL.",
        ),
        K::ConnectionRefused => (
            "connection refused",
            format!("nothing is listening on {hp}"),
            "Is the server running? Check the host and port.",
        ),
        K::ConnectionReset => (
            "connection reset",
            format!("{hp} closed the connection unexpectedly"),
            "The server crashed or dropped the request; check its logs, or http vs https on this port.",
        ),
        K::Timeout => (
            "timeout",
            match e.timeout_ms {
                Some(ms) => format!("no response from {hp} within {ms} ms"),
                None => format!("{hp} did not answer in time"),
            },
            "The server is slow or unreachable (firewall?). Increase --timeout-request.",
        ),
        K::TlsUnknownIssuer => (
            "untrusted certificate",
            format!("the certificate of {host} is signed by an unknown authority (self-signed?)"),
            "Trust its CA with --ssl-extra-ca-certs <pem>, or skip verification with -k/--insecure.",
        ),
        K::TlsExpired => (
            "certificate expired",
            format!("the certificate of {host} is expired or not yet valid"),
            "Renew the server certificate or check the system clock; -k/--insecure skips verification.",
        ),
        K::TlsHostnameMismatch => (
            "certificate name mismatch",
            format!("the certificate of {host} was issued for another name"),
            "Use the host name in the certificate, or skip verification with -k/--insecure.",
        ),
        K::TlsHandshake => (
            "TLS handshake failed",
            format!("could not establish TLS with {hp}"),
            "Is this port really HTTPS? Check the TLS versions the server supports.",
        ),
        K::TlsClientCertificate => (
            "client certificate rejected",
            format!("{host} requires a client certificate it trusts"),
            "Pass --ssl-client-cert/--ssl-client-key (or --ssl-client-cert-list) with a valid certificate.",
        ),
        K::Proxy => (
            "proxy error",
            format!(
                "could not connect through {}",
                e.proxy.as_deref().unwrap_or("the proxy")
            ),
            "Check --proxy or HTTP(S)_PROXY, or bypass it with --no-proxy.",
        ),
        K::TooManyRedirects => (
            "too many redirects",
            "the redirect limit was reached".into(),
            "Look for a redirect loop, or use --ignore-redirects to see the first response.",
        ),
        K::Body => (
            "response body error",
            "the response body could not be read".into(),
            "The connection broke while downloading, or the content encoding is invalid.",
        ),
        K::SkippedByScript => (
            "skipped",
            "pm.execution.skipRequest() was called".into(),
            "Remove the skipRequest() call from the pre-request script to send it.",
        ),
        K::Cancelled => ("cancelled", "the run was cancelled".into(), ""),
        K::Other => ("request failed", e.message.clone(), ""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refused_hint() {
        let e =
            RequestError::new(K::ConnectionRefused, "m", "d").with_url("http://localhost:3000/x");
        let (title, cause, hint) = explain(&e);
        assert_eq!(title, "connection refused");
        assert_eq!(cause, "nothing is listening on localhost:3000");
        assert!(hint.starts_with("Is the server running?"));
    }
}
