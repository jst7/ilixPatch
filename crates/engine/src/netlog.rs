//! Network console (D46): every HTTP exchange as it went over the wire — the
//! final request headers (auth, defaults added by the client), each redirect
//! hop, auth challenge rounds and OAuth 2.0 token requests — reported to an
//! observer. Bodies are previews (at most [`BODY_PREVIEW`] bytes).

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

/// Most body bytes kept per exchange.
pub const BODY_PREVIEW: usize = 64 * 1024;

/// What an exchange was part of (`None` = the request itself).
pub type Phase = Option<&'static str>;

/// One send as shown in the console: a request with its redirects and auth
/// rounds share the id.
#[derive(Debug)]
pub struct Trace {
    pub id: u64,
    /// E.g. the request name, or "Runner · name".
    pub label: String,
}

impl Trace {
    pub fn new(label: impl Into<String>) -> Arc<Self> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Arc::new(Self {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            label: label.into(),
        })
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BodyPreview {
    /// UTF-8 (lossy) text, or a note for binary bodies.
    pub text: String,
    pub size: u64,
    pub truncated: bool,
    pub binary: bool,
}

impl BodyPreview {
    pub fn from_bytes(bytes: &[u8], total: u64) -> Self {
        let cut = &bytes[..bytes.len().min(BODY_PREVIEW)];
        let binary = cut.contains(&0)
            || std::str::from_utf8(cut).is_err() && {
                // A multi-byte char cut at the end is still text.
                let n = crate::bodystore::utf8_floor(cut);
                std::str::from_utf8(&cut[..n]).is_err()
            };
        let text = if binary {
            String::new()
        } else {
            String::from_utf8_lossy(&cut[..crate::bodystore::utf8_floor(cut)]).into_owned()
        };
        Self {
            text,
            size: total,
            truncated: total > cut.len() as u64,
            binary,
        }
    }
}

/// Phases of one exchange (D46 phase 2), in milliseconds.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Timings {
    /// DNS lookup (only for a new connection to a host name).
    pub dns_ms: Option<f64>,
    /// New connection setup: TCP + TLS (+ proxy CONNECT). `None` when reused.
    pub connect_ms: Option<f64>,
    /// A pooled (keep-alive) connection was reused: no DNS or connect.
    pub reused: bool,
    /// Request sent → response headers (time to first byte, server time).
    pub wait_ms: f64,
    /// Reading the body (final response only).
    pub download_ms: Option<f64>,
}

impl Timings {
    /// From the connection timings of a send and the time until its
    /// response headers arrived.
    pub fn from_conn(c: crate::timing::ConnTiming, headers_ms: f64) -> Self {
        let connector = c.connector_ms;
        Self {
            dns_ms: c.dns_ms,
            connect_ms: connector.map(|t| (t - c.dns_ms.unwrap_or(0.0)).max(0.0)),
            reused: connector.is_none(),
            wait_ms: (headers_ms - connector.unwrap_or(0.0)).max(0.0),
            download_ms: None,
        }
    }
}

/// One request/response on the wire.
#[derive(Debug, Clone, Serialize)]
pub struct NetExchange {
    /// Groups the exchanges of one send (a request with its redirects and
    /// auth rounds); assigned by the caller, see `SendOptions::trace`.
    pub trace: Option<u64>,
    /// Label of the send (request name, "Runner · name"…).
    pub label: String,
    /// `redirect`, `digest-response`, `ntlm-negotiate`, `ntlm-authenticate`,
    /// `oauth2-token`…; `None` for the request itself.
    pub phase: Option<String>,
    pub timestamp_ms: i64,
    pub method: String,
    pub url: String,
    /// As sent, including the headers the HTTP client adds (Host, User-Agent,
    /// Accept, Accept-Encoding, Content-Length).
    pub request_headers: Vec<(String, String)>,
    pub request_body: Option<BodyPreview>,
    pub status: Option<u16>,
    pub status_text: Option<String>,
    pub response_headers: Vec<(String, String)>,
    pub response_body: Option<BodyPreview>,
    pub http_version: Option<String>,
    pub remote_addr: Option<String>,
    pub proxy: Option<String>,
    /// Time of this exchange (request sent → response headers/body read).
    pub elapsed_ms: f64,
    /// Breakdown of `elapsed_ms` (`None` when unknown).
    pub timings: Option<Timings>,
    pub error: Option<String>,
}

pub type NetObserver = Arc<dyn Fn(NetExchange) + Send + Sync>;

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

fn header_pairs(h: &reqwest::header::HeaderMap) -> Vec<(String, String)> {
    h.iter()
        .map(|(k, v)| {
            (
                k.to_string(),
                String::from_utf8_lossy(v.as_bytes()).into_owned(),
            )
        })
        .collect()
}

/// The request as it goes on the wire: its headers plus the ones the client
/// adds when they are missing. Bodies are previewed when they are in memory.
pub fn capture_request(
    req: &reqwest::Request,
    user_agent: Option<&str>,
) -> (Vec<(String, String)>, Option<BodyPreview>) {
    let mut headers = header_pairs(req.headers());
    let has =
        |hs: &Vec<(String, String)>, n: &str| hs.iter().any(|(k, _)| k.eq_ignore_ascii_case(n));
    let url = req.url();
    let mut added: Vec<(String, String)> = Vec::new();
    if !has(&headers, "host") {
        let host = url.host_str().unwrap_or_default();
        let host = match url.port() {
            Some(p) => format!("{host}:{p}"),
            None => host.to_string(),
        };
        added.push(("host".into(), host));
    }
    if let Some(ua) = user_agent
        && !has(&headers, "user-agent")
    {
        added.push(("user-agent".into(), ua.into()));
    }
    if !has(&headers, "accept") {
        added.push(("accept".into(), "*/*".into()));
    }
    if !has(&headers, "accept-encoding") {
        added.push(("accept-encoding".into(), "gzip, br".into()));
    }
    let body = req.body().and_then(|b| b.as_bytes());
    if let Some(b) = body
        && !has(&headers, "content-length")
    {
        added.push(("content-length".into(), b.len().to_string()));
    }
    added.append(&mut headers);
    let preview = body.map(|b| BodyPreview::from_bytes(b, b.len() as u64));
    (added, preview)
}

pub fn response_headers(resp: &reqwest::Response) -> Vec<(String, String)> {
    header_pairs(resp.headers())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_text_binary_and_cut() {
        let p = BodyPreview::from_bytes(b"hello", 5);
        assert_eq!(
            (p.text.as_str(), p.binary, p.truncated),
            ("hello", false, false)
        );
        let p = BodyPreview::from_bytes(&[0x89, b'P', b'N', b'G', 0, 1], 6);
        assert!(p.binary && p.text.is_empty());
        let big = vec![b'a'; BODY_PREVIEW + 10];
        let p = BodyPreview::from_bytes(&big, big.len() as u64);
        assert!(p.truncated && p.text.len() == BODY_PREVIEW);
        // "é" cut in half at the end is still text.
        let p = BodyPreview::from_bytes(&[b'a', 0xc3], 2);
        assert!(!p.binary);
    }

    #[test]
    fn captured_request_has_client_headers() {
        let c = reqwest::Client::new();
        let r = c
            .post("https://example.com:8443/x")
            .header("Authorization", "Bearer t")
            .body("{}")
            .build()
            .unwrap();
        let (h, body) = capture_request(&r, Some("ua/1"));
        let get = |n: &str| h.iter().find(|(k, _)| k == n).map(|(_, v)| v.as_str());
        assert_eq!(get("host"), Some("example.com:8443"));
        assert_eq!(get("user-agent"), Some("ua/1"));
        assert_eq!(get("content-length"), Some("2"));
        assert_eq!(get("authorization"), Some("Bearer t"));
        assert_eq!(body.unwrap().text, "{}");
    }
}
