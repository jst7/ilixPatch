//! Token store, flows against a local fake authorization server, and the provider.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use ilixpatch_auth::oauth2::Token;
use ilixpatch_core::auth::{ClientAuthentication, OAuth2Auth, OAuth2Grant};
use ilixpatch_oauth2::{Flows, Opener, StoredToken, TokenProvider, TokenStore};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[derive(Debug, Clone)]
struct Req {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
    body: String,
}

impl Req {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
    fn param<'a>(s: &'a str, key: &str) -> Option<&'a str> {
        s.split('&')
            .find_map(|p| p.strip_prefix(&format!("{key}=")))
    }
    fn query(&self, key: &str) -> Option<&str> {
        Self::param(self.target.split_once('?')?.1, key)
    }
    fn form(&self, key: &str) -> Option<&str> {
        Self::param(&self.body, key)
    }
}

type Handler = Arc<dyn Fn(&Req) -> (u16, Vec<(String, String)>, String) + Send + Sync>;

/// A tiny HTTP server recording every request.
async fn server(handler: Handler) -> (String, Arc<Mutex<Vec<Req>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen2 = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = listener.accept().await else {
                return;
            };
            let handler = handler.clone();
            let seen = seen2.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                let head_end = loop {
                    if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        break i + 4;
                    }
                    let n = s.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                };
                let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
                let mut lines = head.lines();
                let mut first = lines.next().unwrap().split(' ');
                let (method, target) = (
                    first.next().unwrap().to_string(),
                    first.next().unwrap().to_string(),
                );
                let headers: Vec<(String, String)> = lines
                    .filter_map(|l| l.split_once(':'))
                    .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
                    .collect();
                let len: usize = headers
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                    .and_then(|(_, v)| v.parse().ok())
                    .unwrap_or(0);
                let mut body = buf[head_end..].to_vec();
                while body.len() < len {
                    let n = s.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    body.extend_from_slice(&chunk[..n]);
                }
                let req = Req {
                    method,
                    target,
                    headers,
                    body: String::from_utf8_lossy(&body).to_string(),
                };
                seen.lock().unwrap().push(req.clone());
                let (status, hdrs, body) = handler(&req);
                let mut resp = format!(
                    "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n",
                    body.len()
                );
                for (k, v) in hdrs {
                    resp.push_str(&format!("{k}: {v}\r\n"));
                }
                resp.push_str("\r\n");
                resp.push_str(&body);
                let _ = s.write_all(resp.as_bytes()).await;
            });
        }
    });
    (base, seen)
}

fn json(body: &str) -> (u16, Vec<(String, String)>, String) {
    (
        200,
        vec![("Content-Type".into(), "application/json".into())],
        body.into(),
    )
}

fn decode(s: &str) -> String {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                out.push(u8::from_str_radix(&s[i + 1..i + 3], 16).unwrap());
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).unwrap()
}

/// Fake authorization server: /authorize redirects back, /token issues tokens.
fn auth_server() -> Handler {
    Arc::new(|r: &Req| {
        if r.target.starts_with("/authorize") {
            let redirect = decode(r.query("redirect_uri").unwrap());
            let state = r.query("state").unwrap();
            let loc = if r.query("response_type") == Some("token") {
                format!(
                    "{redirect}#access_token=imp-tok&token_type=bearer&expires_in=60&state={state}"
                )
            } else {
                format!("{redirect}?code=abc&state={state}")
            };
            return (302, vec![("Location".into(), loc)], String::new());
        }
        match r.form("grant_type") {
            Some("client_credentials") => {
                json(r#"{"access_token":"cc-tok","token_type":"Bearer","expires_in":3600}"#)
            }
            Some("password") if r.form("username") == Some("bad") => (
                400,
                vec![("Content-Type".into(), "application/json".into())],
                r#"{"error":"invalid_grant","error_description":"bad credentials"}"#.into(),
            ),
            Some("password") => json(r#"{"access_token":"pw-tok","expires_in":3600}"#),
            Some("refresh_token") => json(r#"{"access_token":"refreshed-tok","expires_in":3600}"#),
            Some("authorization_code") if r.form("code") == Some("abc") => {
                json(r#"{"access_token":"code-tok","refresh_token":"rt-1","expires_in":3600}"#)
            }
            _ => (
                400,
                vec![("Content-Type".into(), "application/json".into())],
                r#"{"error":"invalid_grant","error_description":"bad"}"#.into(),
            ),
        }
    })
}

fn cfg(base: &str, grant: OAuth2Grant) -> OAuth2Auth {
    OAuth2Auth {
        grant_type: grant,
        token_name: "test".into(),
        auth_url: Some(format!("{base}/authorize")),
        access_token_url: Some(format!("{base}/token")),
        client_id: "client".into(),
        client_secret: Some("secret".into()),
        scope: Some("read".into()),
        username: Some("ann".into()),
        password: Some("pw".into()),
        use_local_callback: true,
        auto_refresh: true,
        auto_fetch: true,
        ..Default::default()
    }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn stored(
    name: &str,
    access: &str,
    expires_at: Option<u64>,
    refresh: Option<&str>,
    created_at: u64,
) -> StoredToken {
    let mut t = StoredToken::from_token(
        name,
        Token {
            access_token: access.into(),
            refresh_token: refresh.map(str::to_string),
            expires_at,
            ..Default::default()
        },
        created_at,
    );
    t.created_at = created_at;
    t
}

#[test]
fn store_round_trip_and_selection() {
    let dir = tempfile::tempdir().unwrap();
    let store = TokenStore::new(dir.path());
    assert!(store.list(None).unwrap().is_empty());
    let a = store.add(stored("gh", "a", None, None, 100)).unwrap();
    let b = store.add(stored("gh", "b", None, None, 200)).unwrap();
    store.add(stored("other", "c", None, None, 300)).unwrap();

    let gh: Vec<_> = store
        .list(Some("gh"))
        .unwrap()
        .into_iter()
        .map(|t| t.access_token)
        .collect();
    assert_eq!(gh, ["b", "a"]);
    assert_eq!(store.list(None).unwrap().len(), 3);
    // The last added is selected; selection can move.
    assert_eq!(store.current("gh").unwrap().unwrap().id, b.id);
    store.select("gh", &a.id).unwrap();
    assert_eq!(store.current("gh").unwrap().unwrap().id, a.id);
    assert!(store.select("other", &a.id).is_err());
    store.delete(std::slice::from_ref(&a.id)).unwrap();
    assert_eq!(store.current("gh").unwrap().unwrap().id, b.id);

    // Persisted, private file.
    let again = TokenStore::new(dir.path());
    assert_eq!(again.list(None).unwrap().len(), 2);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(store.path())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    let v: serde_json::Value =
        serde_json::from_slice(&std::fs::read(store.path()).unwrap()).unwrap();
    assert_eq!(v["version"], 1);
    // Deleting the selected token clears the selection (the newest is used).
    assert!(v["selected"].get("gh").is_none());
}

#[tokio::test]
async fn client_credentials_basic_and_body_auth() {
    let (base, seen) = server(auth_server()).await;
    let flows = Flows::new(None);
    let mut c = cfg(&base, OAuth2Grant::ClientCredentials);
    let t = flows.get_token(&c).await.unwrap();
    assert_eq!(t.access_token, "cc-tok");
    assert!(t.expires_at.unwrap() > now());
    {
        let reqs = seen.lock().unwrap();
        let r = reqs.last().unwrap();
        assert_eq!(r.method, "POST");
        assert_eq!(
            r.header("authorization"),
            Some("Basic Y2xpZW50OnNlY3JldA==")
        );
        assert!(r.form("client_secret").is_none());
    }
    c.client_authentication = ClientAuthentication::Body;
    flows.get_token(&c).await.unwrap();
    {
        let reqs = seen.lock().unwrap();
        let r = reqs.last().unwrap();
        assert!(r.header("authorization").is_none());
        assert_eq!(r.form("client_id"), Some("client"));
        assert_eq!(r.form("client_secret"), Some("secret"));
    }
    let t = flows
        .get_token(&cfg(&base, OAuth2Grant::Password))
        .await
        .unwrap();
    assert_eq!(t.access_token, "pw-tok");
    assert_eq!(
        seen.lock().unwrap().last().unwrap().form("username"),
        Some("ann")
    );

    // Error responses are readable.
    let mut bad = cfg(&base, OAuth2Grant::Password);
    bad.username = Some("bad".into());
    let err = flows.get_token(&bad).await.unwrap_err();
    assert!(
        err.contains("invalid_grant") || err.contains("bad"),
        "{err}"
    );
}

/// Opener that follows the browser redirect like a real browser would.
fn browser_opener(implicit: bool) -> (Opener, Arc<Mutex<Option<String>>>) {
    let opened = Arc::new(Mutex::new(None));
    let opened2 = opened.clone();
    let opener: Opener = Arc::new(move |url: &str| {
        *opened2.lock().unwrap() = Some(url.to_string());
        let url = url.to_string();
        tokio::spawn(async move {
            if implicit {
                // The fragment never reaches the server: emulate the page's JS POST.
                let client = reqwest::Client::builder()
                    .redirect(reqwest::redirect::Policy::none())
                    .build()
                    .unwrap();
                let resp = client.get(&url).send().await.unwrap();
                let loc = resp.headers()["location"].to_str().unwrap().to_string();
                let page = client.get(&loc).send().await.unwrap().text().await.unwrap();
                assert!(page.contains("location.href"));
                let path_url = loc.split('#').next().unwrap().to_string();
                client.post(&path_url).body(loc).send().await.unwrap();
            } else {
                let page = reqwest::get(&url).await.unwrap().text().await.unwrap();
                assert!(page.contains("You can close this window"));
            }
        });
        Ok(())
    });
    (opener, opened)
}

#[tokio::test]
async fn authorization_code_with_pkce_end_to_end() {
    let (base, seen) = server(auth_server()).await;
    let (opener, opened) = browser_opener(false);
    let flows = Flows::new(Some(opener)).with_browser_timeout(Duration::from_secs(10));
    let t = flows
        .get_token(&cfg(&base, OAuth2Grant::AuthorizationCodePkce))
        .await
        .unwrap();
    assert_eq!(t.access_token, "code-tok");
    assert_eq!(t.refresh_token.as_deref(), Some("rt-1"));
    let url = opened.lock().unwrap().clone().unwrap();
    assert!(url.contains("code_challenge="), "{url}");
    assert!(
        url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A"),
        "{url}"
    );
    let reqs = seen.lock().unwrap();
    let token_req = reqs
        .iter()
        .find(|r| r.form("grant_type") == Some("authorization_code"))
        .unwrap();
    assert!(
        token_req
            .form("code_verifier")
            .is_some_and(|v| v.len() >= 43)
    );
    assert!(token_req.form("redirect_uri").is_some());
}

#[tokio::test]
async fn implicit_token_via_fragment() {
    let (base, _) = server(auth_server()).await;
    let (opener, _) = browser_opener(true);
    let flows = Flows::new(Some(opener)).with_browser_timeout(Duration::from_secs(10));
    let t = flows
        .get_token(&cfg(&base, OAuth2Grant::Implicit))
        .await
        .unwrap();
    assert_eq!(t.access_token, "imp-tok");
}

#[tokio::test]
async fn browser_flow_cancel_and_callback_rules() {
    let (base, _) = server(auth_server()).await;
    let noop: Opener = Arc::new(|_| Ok(()));
    let flows = Arc::new(Flows::new(Some(noop)));
    let f2 = flows.clone();
    let c = cfg(&base, OAuth2Grant::AuthorizationCode);
    let task = tokio::spawn(async move { f2.get_token(&c).await });
    tokio::time::sleep(Duration::from_millis(200)).await;
    flows.cancel();
    let err = task.await.unwrap().unwrap_err();
    assert!(err.contains("cancelled"), "{err}");

    let mut c = cfg(&base, OAuth2Grant::AuthorizationCode);
    c.use_local_callback = false;
    c.callback_url = Some("https://oauth.example.com/cb".into());
    let err = flows.get_token(&c).await.unwrap_err();
    assert!(err.contains("localhost callback"), "{err}");
    // No opener (CLI): clear error.
    let err = Flows::new(None)
        .get_token(&cfg(&base, OAuth2Grant::AuthorizationCode))
        .await
        .unwrap_err();
    assert!(err.contains("browser"), "{err}");
}

#[tokio::test]
async fn provider_uses_refreshes_and_fetches() {
    let (base, seen) = server(auth_server()).await;
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(TokenStore::new(dir.path()));
    let p = TokenProvider::new(store.clone(), Arc::new(Flows::new(None)));
    let n = now();

    // Valid stored token: no network.
    let mut c = cfg(&base, OAuth2Grant::AuthorizationCode);
    store
        .add(stored("test", "stored-tok", Some(n + 3600), None, n))
        .unwrap();
    assert_eq!(p.access_token(&c).await.unwrap(), "stored-tok");
    assert!(seen.lock().unwrap().is_empty());

    // Literal access token wins.
    c.access_token = Some("literal".into());
    assert_eq!(p.access_token(&c).await.unwrap(), "literal");
    c.access_token = None;

    // Expired with a refresh token → refreshed in place, refresh token kept.
    let ids: Vec<String> = store
        .list(None)
        .unwrap()
        .into_iter()
        .map(|t| t.id)
        .collect();
    store.delete(&ids).unwrap();
    let old = store
        .add(stored("test", "old", Some(n - 10), Some("rt-9"), n))
        .unwrap();
    assert_eq!(p.access_token(&c).await.unwrap(), "refreshed-tok");
    let now_stored = store.get(&old.id).unwrap().unwrap();
    assert_eq!(now_stored.access_token, "refreshed-tok");
    assert_eq!(now_stored.refresh_token.as_deref(), Some("rt-9"));
    assert_eq!(
        seen.lock().unwrap().last().unwrap().form("refresh_token"),
        Some("rt-9")
    );

    // Expired, auto_refresh off, code grant → error telling the user what to do.
    let ids: Vec<String> = store
        .list(None)
        .unwrap()
        .into_iter()
        .map(|t| t.id)
        .collect();
    store.delete(&ids).unwrap();
    store
        .add(stored("test", "expired", Some(n - 10), Some("rt"), n))
        .unwrap();
    c.auto_refresh = false;
    let err = p.access_token(&c).await.unwrap_err();
    assert!(err.contains("Get New Access Token"), "{err}");

    // client_credentials + auto_fetch → fetched, stored and selected.
    let mut cc = cfg(&base, OAuth2Grant::ClientCredentials);
    cc.token_name = "machine".into();
    assert_eq!(p.access_token(&cc).await.unwrap(), "cc-tok");
    let current = store.current("machine").unwrap().unwrap();
    assert_eq!(current.access_token, "cc-tok");
    let before = seen.lock().unwrap().len();
    assert_eq!(p.access_token(&cc).await.unwrap(), "cc-tok");
    assert_eq!(seen.lock().unwrap().len(), before, "valid token reused");
    cc.auto_fetch = false;
    cc.token_name = "nothing".into();
    assert!(p.access_token(&cc).await.is_err());
}

#[test]
fn resolves_variables_in_config() {
    use indexmap::IndexMap;
    let mut vars = IndexMap::new();
    vars.insert("id".to_string(), "abc".to_string());
    vars.insert("host".to_string(), "auth.example.com".to_string());
    let scope = ilixpatch_core::Scope::new().push(&vars);
    let mut c = cfg("https://{{host}}", OAuth2Grant::ClientCredentials);
    c.client_id = "{{id}}".into();
    let r = ilixpatch_oauth2::resolve_config(&c, &scope).unwrap();
    assert_eq!(r.client_id, "abc");
    assert_eq!(
        r.access_token_url.as_deref(),
        Some("https://auth.example.com/token")
    );
    c.client_id = "{{missing}}".into();
    assert!(ilixpatch_oauth2::resolve_config(&c, &scope).is_err());
}
