//! A one-shot HTTP listener on 127.0.0.1 that receives the OAuth redirect.

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;

use crate::APP_NAME;

/// Largest request we accept on the callback.
const MAX_REQUEST: usize = 64 * 1024;

pub(crate) struct Loopback {
    listener: TcpListener,
    /// Path the redirect arrives on, e.g. `/callback`.
    path: String,
    /// The redirect URI given to the authorization server.
    pub redirect_uri: String,
}

/// `(host, port, path)` of a loopback callback URL (`http://localhost:8080/cb`).
pub(crate) fn parse_local_url(url: &str) -> Option<(String, u16, String)> {
    let rest = url.strip_prefix("http://")?;
    let (authority, path) = match rest.find(['/', '?', '#']) {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (h, p.parse().ok()?),
        None => (authority, 80),
    };
    if !matches!(host, "localhost" | "127.0.0.1" | "[::1]") {
        return None;
    }
    let path = path.split(['?', '#']).next().unwrap_or("/");
    let path = if path.is_empty() { "/" } else { path };
    Some((host.to_string(), port, path.to_string()))
}

impl Loopback {
    /// Listen for `callback_url` if it is a loopback URL (keeping it as the
    /// redirect URI), else on a random port with `/callback`.
    pub(crate) async fn bind(callback_url: Option<&str>) -> Result<Self, String> {
        let local = callback_url.and_then(parse_local_url);
        let (port, path) = match &local {
            Some((_, port, path)) => (*port, path.clone()),
            None => (0, "/callback".to_string()),
        };
        let listener = TcpListener::bind(("127.0.0.1", port))
            .await
            .map_err(|e| format!("can't listen on 127.0.0.1:{port} for the OAuth callback: {e}"))?;
        let actual = listener.local_addr().map_err(|e| e.to_string())?.port();
        let redirect_uri = match (local, callback_url) {
            (Some(_), Some(url)) => url.to_string(),
            _ => format!("http://127.0.0.1:{actual}{path}"),
        };
        Ok(Self {
            listener,
            path,
            redirect_uri,
        })
    }

    /// Wait for the redirect and return the full redirect URL (query and, for
    /// the implicit grant, fragment included).
    pub(crate) async fn wait(&self, timeout: Duration, cancel: &Notify) -> Result<String, String> {
        let wait = async {
            loop {
                let (stream, _) = self.listener.accept().await.map_err(|e| e.to_string())?;
                if let Some(url) = self.handle(stream).await {
                    return Ok(url);
                }
            }
        };
        tokio::select! {
            r = wait => r,
            _ = tokio::time::sleep(timeout) => Err("timed out waiting for the authorization in the browser".into()),
            _ = cancel.notified() => Err("authorization cancelled".into()),
        }
    }

    /// Serve one connection; `Some(url)` once the redirect has arrived.
    async fn handle(&self, mut stream: TcpStream) -> Option<String> {
        let (method, target, body) = read_request(&mut stream).await?;
        let (path, query) = target.split_once('?').unwrap_or((&target, ""));
        if path != self.path {
            respond(&mut stream, "404 Not Found", "text/plain", "not found").await;
            return None;
        }
        let base = self
            .redirect_uri
            .split(['?', '#'])
            .next()
            .unwrap_or(&self.redirect_uri);
        if method == "POST" {
            // From the implicit-grant page: the full browser URL incl. fragment.
            let url = String::from_utf8_lossy(&body).trim().to_string();
            respond(&mut stream, "200 OK", "text/plain", "ok").await;
            return url
                .split_once('#')
                .map(|(_, frag)| format!("{base}#{frag}"))
                .or(Some(url));
        }
        let has = |k: &str| query.split('&').any(|p| p.split('=').next() == Some(k));
        if has("code") || has("error") || has("access_token") {
            respond(
                &mut stream,
                "200 OK",
                "text/html; charset=utf-8",
                &done_page(),
            )
            .await;
            return Some(format!("{base}?{query}"));
        }
        // No query: the token is in the fragment, which only the browser sees.
        respond(
            &mut stream,
            "200 OK",
            "text/html; charset=utf-8",
            &fragment_page(),
        )
        .await;
        None
    }
}

async fn read_request(stream: &mut TcpStream) -> Option<(String, String, Vec<u8>)> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        if buf.len() > MAX_REQUEST {
            return None;
        }
        let n = tokio::time::timeout(Duration::from_secs(10), stream.read(&mut chunk))
            .await
            .ok()?
            .ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut lines = head.lines();
    let mut first = lines.next()?.split(' ');
    let (method, target) = (first.next()?.to_string(), first.next()?.to_string());
    let len: usize = lines
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.trim().eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.trim().parse().ok())
        .unwrap_or(0)
        .min(MAX_REQUEST);
    let mut body = buf[head_end..].to_vec();
    while body.len() < len {
        let n = tokio::time::timeout(Duration::from_secs(10), stream.read(&mut chunk))
            .await
            .ok()?
            .ok()?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(len);
    Some((method, target, body))
}

async fn respond(stream: &mut TcpStream, status: &str, content_type: &str, body: &str) {
    let resp = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(resp.as_bytes()).await;
    let _ = stream.shutdown().await;
}

const STYLE: &str = "body{font-family:system-ui,sans-serif;display:flex;align-items:center;justify-content:center;height:100vh;margin:0;background:#f6f7f9;color:#1d2125}\
@media (prefers-color-scheme:dark){body{background:#16181b;color:#e6e8eb}}\
main{text-align:center;padding:24px}h1{font-size:20px;margin:0 0 8px}p{opacity:.75;margin:0}";

pub(crate) fn done_page() -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{APP_NAME}</title><style>{STYLE}</style></head>\
<body><main><h1>Authorization received</h1><p>You can close this window and return to {APP_NAME}.</p></main></body></html>"
    )
}

/// Implicit grant: send the fragment back to the loopback server.
fn fragment_page() -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{APP_NAME}</title><style>{STYLE}</style></head>\
<body><main><h1 id=\"t\">Finishing authorization…</h1><p id=\"p\"></p></main><script>\
fetch(location.pathname,{{method:'POST',body:location.href}}).then(function(){{\
document.getElementById('t').textContent='Authorization received';\
document.getElementById('p').textContent='You can close this window and return to {APP_NAME}.';}})\
.catch(function(e){{document.getElementById('p').textContent=String(e);}});\
</script></body></html>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_urls() {
        assert_eq!(
            parse_local_url("http://localhost:8765/cb?x=1"),
            Some(("localhost".into(), 8765, "/cb".into()))
        );
        assert_eq!(
            parse_local_url("http://127.0.0.1:9000"),
            Some(("127.0.0.1".into(), 9000, "/".into()))
        );
        assert_eq!(parse_local_url("https://oauth.example.com/cb"), None);
        assert_eq!(parse_local_url("http://evil.com:80/cb"), None);
    }
}
