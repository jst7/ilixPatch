//! D39 challenge–response tests: Digest (validated independently with MD5)
//! and NTLM over a single keep-alive connection.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use ilixpatch_core::auth::{DigestAuth, NtlmAuth};
use ilixpatch_core::{HttpMethod, Request};
use md5::{Digest, Md5};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::{ChallengeAuth, Engine};

fn req(url: &str) -> Request {
    Request {
        name: "t".into(),
        method: HttpMethod::Get,
        url: url.into(),
        headers: Default::default(),
        query: Default::default(),
        body: None,
        scripts: Default::default(),
        settings: Default::default(),
        auth: Default::default(),
    }
}

/// Read one request head (no bodies in these tests). `None` on EOF.
async fn read_head(s: &mut TcpStream) -> Option<String> {
    let mut buf = Vec::new();
    let mut b = [0u8; 1];
    while !buf.ends_with(b"\r\n\r\n") {
        if s.read(&mut b).await.ok()? == 0 {
            return None;
        }
        buf.push(b[0]);
    }
    Some(String::from_utf8_lossy(&buf).into_owned())
}

fn header<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim().eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

async fn respond(s: &mut TcpStream, status: &str, extra: &str, body: &str) {
    let r = format!(
        "HTTP/1.1 {status}\r\n{extra}content-length: {}\r\n\r\n{body}",
        body.len()
    );
    let _ = s.write_all(r.as_bytes()).await;
}

fn md5_hex(s: &str) -> String {
    Md5::digest(s.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn param<'a>(h: &'a str, name: &str) -> Option<&'a str> {
    h.split(',').find_map(|p| {
        let (k, v) = p.trim().trim_start_matches("Digest ").split_once('=')?;
        (k.trim() == name).then(|| v.trim().trim_matches('"'))
    })
}

/// 401 + Digest challenge (qop=auth); validates the answer with MD5 (RFC 7616).
async fn digest_server(requests: Arc<AtomicUsize>) -> String {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = l.accept().await {
            let requests = requests.clone();
            tokio::spawn(async move {
                while let Some(head) = read_head(&mut s).await {
                    requests.fetch_add(1, Ordering::SeqCst);
                    let target = head.split_whitespace().nth(1).unwrap_or("/").to_string();
                    let Some(auth) =
                        header(&head, "authorization").filter(|a| a.starts_with("Digest "))
                    else {
                        respond(
                            &mut s,
                            "401 Unauthorized",
                            "www-authenticate: Basic realm=\"x\"\r\nwww-authenticate: Digest realm=\"test\", nonce=\"abc123\", qop=\"auth\", opaque=\"op\"\r\n",
                            "",
                        )
                        .await;
                        continue;
                    };
                    let ha1 = md5_hex("user:test:pass");
                    let ha2 = md5_hex(&format!("GET:{}", param(auth, "uri").unwrap_or("")));
                    let expected = md5_hex(&format!(
                        "{ha1}:abc123:{}:{}:auth:{ha2}",
                        param(auth, "nc").unwrap_or(""),
                        param(auth, "cnonce").unwrap_or("")
                    ));
                    let ok = param(auth, "response") == Some(expected.as_str())
                        && param(auth, "uri") == Some(target.as_str())
                        && param(auth, "opaque") == Some("op");
                    if ok {
                        respond(&mut s, "200 OK", "", "welcome").await;
                    } else {
                        respond(&mut s, "403 Forbidden", "", auth).await;
                    }
                }
            });
        }
    });
    format!("http://{addr}")
}

#[tokio::test]
async fn digest_answers_the_challenge() {
    let count = Arc::new(AtomicUsize::new(0));
    let base = digest_server(count.clone()).await;
    let cfg = DigestAuth {
        username: "user".into(),
        password: "pass".into(),
        ..Default::default()
    };
    let engine = Engine::new();
    let r = engine
        .send_with_auth(
            &req(&format!("{base}/dir/index?x=1")),
            None,
            &ChallengeAuth::Digest(cfg.clone()),
        )
        .await
        .unwrap();
    assert_eq!((r.status, r.body_text().as_str()), (200, "welcome"));
    assert_eq!(count.load(Ordering::SeqCst), 2);

    // disable_retry: the 401 is returned as is.
    let r = engine
        .send_with_auth(
            &req(&format!("{base}/x")),
            None,
            &ChallengeAuth::Digest(DigestAuth {
                disable_retry: true,
                ..cfg
            }),
        )
        .await
        .unwrap();
    assert_eq!(r.status, 401);
}

/// A minimal NTLM Type 2 (challenge) message with an empty target info.
fn type2() -> String {
    let mut m = Vec::new();
    m.extend_from_slice(b"NTLMSSP\0");
    m.extend_from_slice(&2u32.to_le_bytes());
    // Target name: empty, at offset 48.
    m.extend_from_slice(&0u16.to_le_bytes());
    m.extend_from_slice(&0u16.to_le_bytes());
    m.extend_from_slice(&48u32.to_le_bytes());
    // Flags: UNICODE | NTLM | ALWAYS_SIGN | TARGET_INFO | EXTENDED_SESSIONSECURITY.
    m.extend_from_slice(&0x0088_8205u32.to_le_bytes());
    m.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]); // server challenge
    m.extend_from_slice(&[0; 8]); // reserved
    // Target info: MsvAvEOL (4 bytes) at offset 48.
    m.extend_from_slice(&4u16.to_le_bytes());
    m.extend_from_slice(&4u16.to_le_bytes());
    m.extend_from_slice(&48u32.to_le_bytes());
    m.extend_from_slice(&[0, 0, 0, 0]);
    B64.encode(m)
}

/// NTLM: Type 1 → 401 Type 2 → Type 3, which must arrive on the SAME connection.
async fn ntlm_server(connections: Arc<AtomicUsize>) -> String {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = l.accept().await {
            connections.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async move {
                let mut negotiated = false;
                while let Some(head) = read_head(&mut s).await {
                    let auth = header(&head, "authorization").unwrap_or("");
                    let msg = auth
                        .strip_prefix("NTLM ")
                        .and_then(|b| B64.decode(b).ok())
                        .unwrap_or_default();
                    let kind = msg.get(8).copied().unwrap_or(0);
                    if msg.starts_with(b"NTLMSSP\0") && kind == 1 {
                        negotiated = true;
                        let extra = format!("www-authenticate: NTLM {}\r\n", type2());
                        respond(&mut s, "401 Unauthorized", &extra, "").await;
                    } else if msg.starts_with(b"NTLMSSP\0") && kind == 3 && negotiated {
                        respond(&mut s, "200 OK", "", "same connection").await;
                    } else {
                        respond(&mut s, "401 Unauthorized", "www-authenticate: NTLM\r\n", "").await;
                    }
                }
            });
        }
    });
    format!("http://{addr}")
}

#[tokio::test]
async fn ntlm_handshake_reuses_the_connection() {
    let conns = Arc::new(AtomicUsize::new(0));
    let base = ntlm_server(conns.clone()).await;
    let cfg = NtlmAuth {
        username: "user".into(),
        password: "pass".into(),
        domain: Some("DOM".into()),
        ..Default::default()
    };
    let seen: Arc<std::sync::Mutex<Vec<crate::NetExchange>>> = Arc::default();
    let sink = seen.clone();
    let opts = crate::SendOptions {
        trace: Some(crate::Trace::new("ntlm")),
        ..Default::default()
    };
    let r = Engine::new()
        .with_observer(Arc::new(move |ex| sink.lock().unwrap().push(ex)))
        .send_opts(
            &req(&format!("{base}/")),
            opts,
            Some(&ChallengeAuth::Ntlm(cfg)),
        )
        .await
        .unwrap();
    assert_eq!((r.status, r.body_text().as_str()), (200, "same connection"));
    assert_eq!(conns.load(Ordering::SeqCst), 1);
    // D46: both rounds show in the network console, with the NTLM headers.
    let seen = seen.lock().unwrap();
    let phases: Vec<_> = seen
        .iter()
        .map(|e| (e.phase.as_deref(), e.status))
        .collect();
    assert_eq!(
        phases,
        [
            (Some("ntlm-negotiate"), Some(401)),
            (Some("ntlm-authenticate"), Some(200))
        ]
    );
    assert!(
        seen[1]
            .request_headers
            .iter()
            .any(|(k, v)| k == "authorization" && v.starts_with("NTLM "))
    );
}

#[test]
fn wire_helpers() {
    let mut r = req("http://h/p");
    r.query.insert("a b".into(), "c&d".into());
    r.body = Some(ilixpatch_core::Body::Form {
        fields: [("x".to_string(), "1 2".to_string())].into_iter().collect(),
    });
    assert_eq!(crate::full_url(&r).unwrap(), "http://h/p?a+b=c%26d");
    assert_eq!(crate::body_bytes(&r), b"x=1+2");
}
