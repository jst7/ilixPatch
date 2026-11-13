//! Engine tests against local HTTP/HTTPS servers (no network access).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ilixpatch_core::{HttpMethod, RequestSettings};
use rcgen::{BasicConstraints, CertificateParams, DnType, IsCa, Issuer, KeyPair};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio_rustls::rustls::server::WebPkiClientVerifier;
use tokio_rustls::rustls::{self, RootCertStore, ServerConfig};

use super::*;

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
        extract: Default::default(),
    }
}

/// Minimal HTTP/1.1 handler. Routes:
/// - `/redirect/<n>` → 302 to `/redirect/<n-1>`; `/redirect/0` → 200
/// - `/see-other` → 303 to `/echo`
/// - anything else → 200 with body `<METHOD> <target> [proxy-auth]`
async fn handle<S: AsyncRead + AsyncWrite + Unpin>(mut s: S) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = s.read(&mut chunk).await.unwrap_or(0);
        if n == 0 {
            return;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    let head = String::from_utf8_lossy(&buf).to_string();
    let mut parts = head.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("/").to_string();
    let path = target
        .split_once("://")
        .and_then(|(_, rest)| rest.find('/').map(|i| rest[i..].to_string()))
        .unwrap_or_else(|| target.clone());
    let proxy_auth = head
        .lines()
        .find(|l| l.to_ascii_lowercase().starts_with("proxy-authorization:"))
        .map(|l| format!(" {}", l.trim()))
        .unwrap_or_default();
    let (status, location, body) = if let Some(n) = path.strip_prefix("/redirect/") {
        let n: u32 = n.parse().unwrap_or(0);
        if n == 0 {
            ("200 OK", None, "done".to_string())
        } else {
            (
                "302 Found",
                Some(format!("/redirect/{}", n - 1)),
                String::new(),
            )
        }
    } else if path == "/see-other" {
        ("303 See Other", Some("/echo".to_string()), String::new())
    } else if path == "/missing" {
        ("404 Not Found", None, "nope".to_string())
    } else {
        ("200 OK", None, format!("{method} {target}{proxy_auth}"))
    };
    let loc = location
        .map(|l| format!("location: {l}\r\n"))
        .unwrap_or_default();
    let resp = format!(
        "HTTP/1.1 {status}\r\n{loc}content-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = s.write_all(resp.as_bytes()).await;
    let _ = s.shutdown().await;
}

async fn http_server() -> String {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((s, _)) = l.accept().await {
            tokio::spawn(handle(s));
        }
    });
    format!("http://{addr}")
}

struct Pki {
    dir: tempfile::TempDir,
    ca_der: CertificateDer<'static>,
    server_cert: CertificateDer<'static>,
    server_key: Vec<u8>,
    wrong_name: (CertificateDer<'static>, Vec<u8>),
    expired: (CertificateDer<'static>, Vec<u8>),
}

impl Pki {
    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }
}

fn pki() -> Pki {
    let ca_key = KeyPair::generate().unwrap();
    let mut ca_params = CertificateParams::new(Vec::<String>::new()).unwrap();
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params
        .distinguished_name
        .push(DnType::CommonName, "ilixPatch Test CA");
    let ca = ca_params.self_signed(&ca_key).unwrap();
    let issuer = Issuer::new(ca_params, ca_key);

    let server_key = KeyPair::generate().unwrap();
    let mut sp =
        CertificateParams::new(vec!["localhost".to_string(), "127.0.0.1".to_string()]).unwrap();
    sp.distinguished_name.push(DnType::CommonName, "localhost");
    let server = sp.signed_by(&server_key, &issuer).unwrap();

    // Valid chain, wrong name.
    let other_key = KeyPair::generate().unwrap();
    let wrong_name = CertificateParams::new(vec!["other.example".to_string()])
        .unwrap()
        .signed_by(&other_key, &issuer)
        .unwrap();
    // Right name, expired.
    let expired_key = KeyPair::generate().unwrap();
    let mut ep = CertificateParams::new(vec!["127.0.0.1".to_string()]).unwrap();
    ep.not_before = rcgen::date_time_ymd(2000, 1, 1);
    ep.not_after = rcgen::date_time_ymd(2001, 1, 1);
    let expired = ep.signed_by(&expired_key, &issuer).unwrap();

    let client_key = KeyPair::generate().unwrap();
    let mut cp = CertificateParams::new(vec!["client".to_string()]).unwrap();
    cp.distinguished_name.push(DnType::CommonName, "client");
    let client = cp.signed_by(&client_key, &issuer).unwrap();

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("ca.pem"), ca.pem()).unwrap();
    std::fs::write(dir.path().join("client.pem"), client.pem()).unwrap();
    std::fs::write(dir.path().join("client.key"), client_key.serialize_pem()).unwrap();

    // PKCS#12 bundle of the client identity.
    let mut ks = p12_keystore::KeyStore::new();
    ks.add_entry(
        "client",
        p12_keystore::KeyStoreEntry::PrivateKeyChain(p12_keystore::PrivateKeyChain::new(
            b"client".as_slice(),
            p12_keystore::PrivateKey::from_der(&client_key.serialize_der()).unwrap(),
            [p12_keystore::Certificate::from_der(client.der()).unwrap()],
        )),
    );
    std::fs::write(
        dir.path().join("client.p12"),
        ks.writer("s3cret").write().unwrap(),
    )
    .unwrap();

    Pki {
        dir,
        ca_der: ca.der().clone(),
        server_cert: server.der().clone(),
        server_key: server_key.serialize_der(),
        wrong_name: (wrong_name.der().clone(), other_key.serialize_der()),
        expired: (expired.der().clone(), expired_key.serialize_der()),
    }
}

async fn https_server(pki: &Pki, require_client_cert: bool) -> String {
    https_server_with(
        pki.server_cert.clone(),
        pki.server_key.clone(),
        require_client_cert.then(|| pki.ca_der.clone()),
    )
    .await
}

/// HTTPS server with this certificate; requires a client certificate signed
/// by `client_ca` when given.
async fn https_server_with(
    cert: CertificateDer<'static>,
    key: Vec<u8>,
    client_ca: Option<CertificateDer<'static>>,
) -> String {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = ServerConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .unwrap();
    let builder = if let Some(ca) = client_ca {
        let mut roots = RootCertStore::empty();
        roots.add(ca).unwrap();
        builder.with_client_cert_verifier(
            WebPkiClientVerifier::builder_with_provider(Arc::new(roots), provider)
                .build()
                .unwrap(),
        )
    } else {
        builder.with_no_client_auth()
    };
    let config = builder
        .with_single_cert(
            vec![cert],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key)),
        )
        .unwrap();
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((s, _)) = l.accept().await {
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                if let Ok(tls) = acceptor.accept(s).await {
                    handle(tls).await;
                }
            });
        }
    });
    format!("https://127.0.0.1:{}", addr.port())
}

fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

#[tokio::test]
async fn download_follows_settings_and_limits() {
    let base = http_server().await;
    let engine = Engine::new();
    let body = engine
        .download(&format!("{base}/spec.yaml"), 1024)
        .await
        .unwrap();
    assert_eq!(String::from_utf8(body).unwrap(), "GET /spec.yaml");
    // Redirects are followed.
    let body = engine
        .download(&format!("{base}/redirect/1"), 1024)
        .await
        .unwrap();
    assert_eq!(body, b"done");
    let err = engine
        .download(&format!("{base}/redirect/11"), 1024)
        .await
        .unwrap_err();
    assert!(err.starts_with("too many redirects"), "{err}");
    assert_eq!(
        engine.download(&format!("{base}/missing"), 1024).await,
        Err("HTTP 404 Not Found".into())
    );
    let err = engine
        .download(&format!("{base}/echo"), 3)
        .await
        .unwrap_err();
    assert!(err.starts_with("larger than"), "{err}");
    assert!(engine.download("not a url", 10).await.is_err());
}

#[tokio::test]
async fn self_signed_needs_ca_or_no_verification() {
    let pki = pki();
    let url = format!("{}/hello", https_server(&pki, false).await);

    let err = Engine::new().send(&req(&url)).await.unwrap_err();
    assert!(
        err.to_string().to_lowercase().contains("certificate"),
        "{err}"
    );

    let with_ca = Engine::with_settings(NetworkSettings {
        ca_certificates: vec![s(&pki.path("ca.pem"))],
        ..Default::default()
    })
    .unwrap();
    let r = with_ca.send(&req(&url)).await.unwrap();
    assert_eq!(r.body_text(), "GET /hello");
    let n = &r.network;
    assert_eq!(n.http_version, "HTTP/1.1");
    assert!(n.remote_addr.as_deref().unwrap().starts_with("127.0.0.1:"));
    assert_eq!(n.final_url, url);
    assert_eq!(n.proxy, None);
    let tls = n.tls.as_ref().unwrap();
    assert!(tls.verified);
    let cert = tls.peer_certificate.as_ref().unwrap();
    assert_eq!(cert.subject, "CN=localhost");
    assert_eq!(cert.issuer, "CN=ilixPatch Test CA");
    assert_eq!(cert.san, ["localhost", "127.0.0.1"]);
    assert_eq!(cert.sha256.len(), 32 * 3 - 1);
    assert!(cert.not_before.ends_with('Z') && cert.not_before < cert.not_after);

    let insecure = Engine::with_settings(NetworkSettings {
        verify_ssl: false,
        ..Default::default()
    })
    .unwrap();
    let r = insecure.send(&req(&url)).await.unwrap();
    assert!(!r.network.tls.unwrap().verified);

    // Per-request override beats the global setting, both ways.
    let mut r = req(&url);
    r.settings.verify_ssl = Some(false);
    assert!(Engine::new().send(&r).await.is_ok());
    r.settings.verify_ssl = Some(true);
    assert!(insecure.send(&r).await.is_err());
}

#[tokio::test]
async fn mutual_tls_with_pem_and_pkcs12() {
    let pki = pki();
    let url = format!("{}/mtls", https_server(&pki, true).await);
    let ca = vec![s(&pki.path("ca.pem"))];

    let no_identity = Engine::with_settings(NetworkSettings {
        ca_certificates: ca.clone(),
        ..Default::default()
    })
    .unwrap();
    assert!(no_identity.send(&req(&url)).await.is_err());

    let pem = Engine::with_settings(NetworkSettings {
        ca_certificates: ca.clone(),
        client_certificates: vec![ClientCertificate {
            host: "127.0.0.1".into(),
            cert_path: Some(s(&pki.path("client.pem"))),
            key_path: Some(s(&pki.path("client.key"))),
            pfx_path: None,
            passphrase: None,
            enabled: true,
        }],
        ..Default::default()
    })
    .unwrap();
    assert_eq!(pem.send(&req(&url)).await.unwrap().body_text(), "GET /mtls");

    let p12 = |pass: &str, host: &str| {
        Engine::with_settings(NetworkSettings {
            ca_certificates: ca.clone(),
            client_certificates: vec![ClientCertificate {
                host: host.into(),
                cert_path: None,
                key_path: None,
                pfx_path: Some(s(&pki.path("client.p12"))),
                passphrase: Some(pass.into()),
                enabled: true,
            }],
            ..Default::default()
        })
    };
    let ok = p12("s3cret", "127.0.0.1").unwrap();
    assert_eq!(ok.send(&req(&url)).await.unwrap().body_text(), "GET /mtls");

    // Identity only for another host: not sent.
    let other = p12("s3cret", "*.example.com").unwrap();
    assert!(other.send(&req(&url)).await.is_err());

    let Err(EngineError::Config(msg)) = p12("wrong", "127.0.0.1") else {
        panic!("wrong passphrase must fail");
    };
    assert!(msg.starts_with("client_certificates[0].pfx_path"), "{msg}");
}

#[tokio::test]
async fn redirects_are_recorded_and_limited() {
    let base = http_server().await;
    let engine = Engine::new();

    let r = engine
        .send(&req(&format!("{base}/redirect/3")))
        .await
        .unwrap();
    assert_eq!(r.status, 200);
    assert_eq!(r.body_text(), "done");
    let hops: Vec<_> = r
        .network
        .redirects
        .iter()
        .map(|h| (h.status, h.url.clone()))
        .collect();
    assert_eq!(
        hops,
        [
            (302, format!("{base}/redirect/2")),
            (302, format!("{base}/redirect/1")),
            (302, format!("{base}/redirect/0")),
        ]
    );
    assert_eq!(r.network.final_url, format!("{base}/redirect/0"));
    assert!(r.network.tls.is_none());

    let mut no_follow = req(&format!("{base}/redirect/3"));
    no_follow.settings.follow_redirects = Some(false);
    let r = engine.send(&no_follow).await.unwrap();
    assert_eq!(r.status, 302);
    assert!(r.network.redirects.is_empty());

    let mut limited = req(&format!("{base}/redirect/3"));
    limited.settings.max_redirects = Some(2);
    assert!(matches!(
        engine.send(&limited).await,
        Err(EngineError::TooManyRedirects(2))
    ));

    let global_no_follow = Engine::with_settings(NetworkSettings {
        follow_redirects: false,
        ..Default::default()
    })
    .unwrap();
    assert_eq!(
        global_no_follow
            .send(&req(&format!("{base}/redirect/1")))
            .await
            .unwrap()
            .status,
        302
    );

    // 303 turns a POST into a GET.
    let mut post = req(&format!("{base}/see-other"));
    post.method = HttpMethod::Post;
    post.body = Some(Body::Text {
        content: "x".into(),
    });
    let r = engine.send(&post).await.unwrap();
    assert_eq!(r.body_text(), "GET /echo");
}

#[tokio::test]
async fn custom_proxy_and_bypass() {
    let target = http_server().await;
    // The "proxy" answers by itself, echoing the absolute-form target.
    let proxy = http_server().await;
    let settings = |bypass: Vec<String>| NetworkSettings {
        proxy: ProxySettings {
            mode: ProxyMode::Custom,
            url: Some(proxy.clone()),
            username: Some("user".into()),
            password: Some("pw".into()),
            bypass,
        },
        ..Default::default()
    };
    let engine = Engine::with_settings(settings(vec![])).unwrap();
    let r = engine.send(&req(&format!("{target}/x"))).await.unwrap();
    assert_eq!(
        r.body_text(),
        format!("GET {target}/x proxy-authorization: Basic dXNlcjpwdw==")
    );
    assert_eq!(r.network.proxy.as_deref(), Some(proxy.as_str()));

    let bypassing = Engine::with_settings(settings(vec!["localhost, 127.0.0.1".into()])).unwrap();
    let r = bypassing.send(&req(&format!("{target}/x"))).await.unwrap();
    assert_eq!(r.body_text(), "GET /x");
    assert_eq!(r.network.proxy, None);

    let none = Engine::with_settings(NetworkSettings {
        proxy: ProxySettings {
            mode: ProxyMode::None,
            ..Default::default()
        },
        ..Default::default()
    })
    .unwrap();
    assert_eq!(
        none.send(&req(&format!("{target}/y")))
            .await
            .unwrap()
            .body_text(),
        "GET /y"
    );
}

#[test]
fn settings_validation_names_the_field() {
    let err = |s: NetworkSettings| match Engine::with_settings(s) {
        Err(EngineError::Config(m)) => m,
        Err(e) => panic!("unexpected error {e}"),
        Ok(_) => panic!("expected an error"),
    };
    assert!(
        err(NetworkSettings {
            ca_certificates: vec!["/nope/ca.pem".into()],
            ..Default::default()
        })
        .starts_with("ca_certificates[0]: /nope/ca.pem")
    );
    let proxy = |url: &str| NetworkSettings {
        proxy: ProxySettings {
            mode: ProxyMode::Custom,
            url: Some(url.into()),
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(err(proxy("ftp://p:1")).starts_with("proxy.url"));
    assert!(err(proxy("")).starts_with("proxy.url"));
    assert!(Engine::with_settings(proxy("socks5h://127.0.0.1:1080")).is_ok());
    let m = err(NetworkSettings {
        client_certificates: vec![ClientCertificate {
            host: "bad host".into(),
            cert_path: None,
            key_path: None,
            pfx_path: None,
            passphrase: None,
            enabled: true,
        }],
        ..Default::default()
    });
    assert!(m.starts_with("client_certificates[0].host"), "{m}");
    // Disabled entries are not validated.
    assert!(
        Engine::with_settings(NetworkSettings {
            client_certificates: vec![ClientCertificate {
                host: "bad host".into(),
                cert_path: None,
                key_path: None,
                pfx_path: None,
                passphrase: None,
                enabled: false,
            }],
            ..Default::default()
        })
        .is_ok()
    );
}

#[test]
fn request_settings_override_precedence() {
    let engine = Engine::with_settings(NetworkSettings {
        timeout_ms: 5000,
        max_redirects: 7,
        http_version: HttpVersionPref::Http2,
        ..Default::default()
    })
    .unwrap();
    let mut r = req("http://x");
    let e = engine.effective(&r, None);
    assert_eq!(e.timeout, Some(Duration::from_secs(5)));
    assert_eq!(e.max_redirects, 7);
    assert_eq!(e.http_version, HttpVersionPref::Http2);
    assert!(e.verify_ssl && e.follow_redirects);
    // Caller timeout (e.g. --timeout-request) beats the global one…
    assert_eq!(
        engine
            .effective(&r, Some(Duration::from_millis(10)))
            .timeout,
        Some(Duration::from_millis(10))
    );
    // …and the request's own settings beat both (0 = no timeout).
    r.settings = RequestSettings {
        timeout_ms: Some(0),
        http_version: Some(HttpVersionPref::Http1),
        max_redirects: Some(1),
        ..Default::default()
    };
    let e = engine.effective(&r, Some(Duration::from_millis(10)));
    assert_eq!(e.timeout, None);
    assert_eq!(e.http_version, HttpVersionPref::Http1);
    assert_eq!(e.max_redirects, 1);
}

#[tokio::test]
async fn invalid_url_is_rejected() {
    assert!(matches!(
        Engine::new().send(&req("not a url")).await,
        Err(EngineError::InvalidUrl(_))
    ));
}

#[tokio::test]
async fn timeout_is_reported() {
    // A server that accepts but never answers.
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move {
        let mut keep = Vec::new();
        while let Ok((s, _)) = l.accept().await {
            keep.push(s);
        }
    });
    let mut r = req(&format!("http://{addr}/"));
    r.settings.timeout_ms = Some(150);
    let err = Engine::new().send(&r).await.unwrap_err();
    assert!(matches!(err, EngineError::Timeout(150)), "{err}");
    assert_eq!(err.to_string(), "request timed out after 150 ms");
}

// ---------- D36: error classification ----------

async fn fail(engine: &Engine, r: &Request) -> RequestError {
    let err = engine.send(r).await.expect_err("request must fail");
    engine.describe_error(&err, r, None)
}

#[tokio::test]
async fn classifies_connection_errors() {
    // Closed port: bind, take the port, drop the listener.
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let url = format!("http://127.0.0.1:{port}/x");
    let e = fail(&Engine::new(), &req(&url)).await;
    assert_eq!(e.kind, RequestErrorKind::ConnectionRefused, "{e:?}");
    assert_eq!(e.host.as_deref(), Some("127.0.0.1"));
    assert_eq!(e.port, Some(port));
    assert_eq!(e.url.as_deref(), Some(url.as_str()));
    assert_eq!(
        e.message,
        format!("Connection refused by 127.0.0.1:{port}.")
    );
    assert!(e.detail.contains(" → "), "{}", e.detail);

    let e = fail(&Engine::new(), &req("http://nonexistent.invalid/")).await;
    assert_eq!(e.kind, RequestErrorKind::Dns, "{e:?}");
    assert_eq!(e.host.as_deref(), Some("nonexistent.invalid"));
    assert_eq!(e.port, Some(80));

    let e = fail(&Engine::new(), &req("not a url")).await;
    assert_eq!(e.kind, RequestErrorKind::InvalidUrl);

    // A proxy that refuses connections.
    let proxied = Engine::with_settings(NetworkSettings {
        proxy: ProxySettings {
            mode: ProxyMode::Custom,
            url: Some(format!("http://user:pw@127.0.0.1:{port}")),
            ..Default::default()
        },
        ..Default::default()
    })
    .unwrap();
    let e = fail(&proxied, &req("http://example.com/")).await;
    assert_eq!(e.kind, RequestErrorKind::Proxy, "{e:?}");
    assert_eq!(e.proxy, Some(format!("http://127.0.0.1:{port}")));
    assert_eq!(e.host.as_deref(), Some("example.com"));
}

#[tokio::test]
async fn classifies_timeout_and_redirect_limit() {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move {
        let mut keep = Vec::new();
        while let Ok((s, _)) = l.accept().await {
            keep.push(s);
        }
    });
    let mut r = req(&format!("http://{addr}/"));
    r.settings.timeout_ms = Some(120);
    let e = fail(&Engine::new(), &r).await;
    assert_eq!(e.kind, RequestErrorKind::Timeout);
    assert_eq!(e.timeout_ms, Some(120));
    assert_eq!(e.message, "The request timed out after 120 ms.");

    let base = http_server().await;
    let mut r = req(&format!("{base}/redirect/5"));
    r.settings.max_redirects = Some(2);
    let e = fail(&Engine::new(), &r).await;
    assert_eq!(e.kind, RequestErrorKind::TooManyRedirects);
}

#[tokio::test]
async fn classifies_tls_errors() {
    let pki = pki();
    let trusting = Engine::with_settings(NetworkSettings {
        ca_certificates: vec![s(&pki.path("ca.pem"))],
        ..Default::default()
    })
    .unwrap();

    let url = format!("{}/x", https_server(&pki, false).await);
    let e = fail(&Engine::new(), &req(&url)).await;
    assert_eq!(e.kind, RequestErrorKind::TlsUnknownIssuer, "{e:?}");
    assert_eq!(e.host.as_deref(), Some("127.0.0.1"));

    let (cert, key) = pki.wrong_name.clone();
    let url = format!("{}/x", https_server_with(cert, key, None).await);
    let e = fail(&trusting, &req(&url)).await;
    assert_eq!(e.kind, RequestErrorKind::TlsHostnameMismatch, "{e:?}");

    let (cert, key) = pki.expired.clone();
    let url = format!("{}/x", https_server_with(cert, key, None).await);
    let e = fail(&trusting, &req(&url)).await;
    assert_eq!(e.kind, RequestErrorKind::TlsExpired, "{e:?}");

    let url = format!("{}/x", https_server(&pki, true).await);
    let e = fail(&trusting, &req(&url)).await;
    // With TLS 1.3 the server rejects a missing client certificate after the
    // handshake (alert + close). On Windows the OS often aborts the socket
    // (WSAECONNABORTED, os error 10053) before the client reads that alert, so
    // the failure surfaces as a reset connection instead.
    if cfg!(windows) {
        assert!(
            matches!(
                e.kind,
                RequestErrorKind::TlsClientCertificate | RequestErrorKind::ConnectionReset
            ),
            "{e:?}"
        );
    } else {
        assert_eq!(e.kind, RequestErrorKind::TlsClientCertificate, "{e:?}");
    }
}

#[tokio::test]
async fn network_console_reports_each_hop() {
    let base = http_server().await;
    let seen: Arc<std::sync::Mutex<Vec<NetExchange>>> = Arc::default();
    let sink = seen.clone();
    let engine = Engine::new().with_observer(Arc::new(move |ex| sink.lock().unwrap().push(ex)));
    let mut r = req(&format!("{base}/redirect/2"));
    r.headers.insert("X-Test".into(), "1".into());
    let trace = Trace::new("my request");
    let opts = SendOptions {
        trace: Some(trace.clone()),
        ..Default::default()
    };
    engine.send_opts(&r, opts, None).await.unwrap();
    // Untraced sends (load tests) are not reported.
    engine.send(&req(&format!("{base}/x"))).await.unwrap();
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 3, "two redirect hops and the final response");
    assert!(
        seen.iter()
            .all(|e| e.trace == Some(trace.id) && e.label == "my request")
    );
    assert_eq!(seen[0].phase.as_deref(), Some("redirect"));
    assert_eq!(seen[1].phase.as_deref(), Some("redirect"));
    assert_eq!(seen[2].phase, None);
    assert_eq!(seen[2].status, Some(200));
    let h = &seen[0].request_headers;
    let get = |n: &str| h.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone());
    assert_eq!(get("x-test").as_deref(), Some("1"));
    assert_eq!(get("user-agent").as_deref(), Some(USER_AGENT));
    assert!(get("host").is_some());
    assert!(
        seen[2]
            .response_body
            .as_ref()
            .is_some_and(|b| b.text == "done")
    );
}

#[tokio::test]
async fn network_console_timings() {
    // "localhost" goes through the resolver, so the DNS lookup is timed too.
    let base = http_server().await.replace("127.0.0.1", "localhost");
    let seen: Arc<std::sync::Mutex<Vec<NetExchange>>> = Arc::default();
    let sink = seen.clone();
    let engine = Engine::new().with_observer(Arc::new(move |ex| sink.lock().unwrap().push(ex)));
    let opts = SendOptions {
        trace: Some(Trace::new("t")),
        ..Default::default()
    };
    engine
        .send_opts(&req(&format!("{base}/x")), opts, None)
        .await
        .unwrap();
    let seen = seen.lock().unwrap();
    let t = seen[0].timings.clone().expect("timings");
    assert!(!t.reused, "first send opens a connection");
    assert!(t.dns_ms.is_some(), "localhost is resolved");
    assert!(t.connect_ms.is_some());
    assert!(t.download_ms.is_some());
    let sum = t.dns_ms.unwrap() + t.connect_ms.unwrap() + t.wait_ms + t.download_ms.unwrap();
    assert!(
        sum <= seen[0].elapsed_ms + 1.0,
        "{t:?} vs {}",
        seen[0].elapsed_ms
    );
}
