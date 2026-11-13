//! Challenge–response authorization (D39): Digest and NTLM, plus helpers
//! that describe the request exactly as it goes on the wire (for signers).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use ilixpatch_auth::{AuthEnv, AuthError, digest, ntlm};
use ilixpatch_core::auth::{DigestAuth, NtlmAuth};
use ilixpatch_core::{Body, Request};

use crate::{Engine, EngineError, Response, SendOptions};

/// Auth types that need a server challenge before the real request.
#[derive(Debug, Clone, PartialEq)]
pub enum ChallengeAuth {
    /// Send; on `401` + `WWW-Authenticate: Digest …` answer once (nc=1, random
    /// cnonce). With `realm` + `nonce` configured the header is sent up-front.
    /// `disable_retry` returns the 401 as is.
    Digest(DigestAuth),
    /// Type 1 → `401` Type 2 → Type 3 over the same connection (a dedicated
    /// HTTP/1.1 client with one idle connection per host). Needs a server that
    /// keeps the connection alive; through proxies that don't, the handshake
    /// fails.
    Ntlm(NtlmAuth),
}

/// The absolute URL including the `query` table, encoded exactly as the
/// engine sends it.
pub fn full_url(req: &Request) -> Result<String, EngineError> {
    let mut url =
        reqwest::Url::parse(&req.url).map_err(|_| EngineError::InvalidUrl(req.url.clone()))?;
    if !req.query.is_empty() {
        url.query_pairs_mut()
            .extend_pairs(req.query.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    }
    Ok(url.to_string())
}

/// The body bytes exactly as the engine sends them (forms URL-encoded).
pub fn body_bytes(req: &Request) -> Vec<u8> {
    match &req.body {
        None => Vec::new(),
        Some(Body::Json { content }) | Some(Body::Text { content }) => content.as_bytes().to_vec(),
        Some(Body::Form { fields }) => {
            let mut u = reqwest::Url::parse("http://x/").expect("static URL");
            u.query_pairs_mut()
                .extend_pairs(fields.iter().map(|(k, v)| (k.as_str(), v.as_str())));
            u.query().unwrap_or_default().as_bytes().to_vec()
        }
    }
}

/// Run `f` with the real clock and OS randomness.
pub fn with_auth_env<R>(f: impl FnOnce(&mut AuthEnv) -> R) -> R {
    // Nonces, salts and RSA blinding must never fall back to weak values:
    // if the OS RNG is unavailable, fail loudly instead of signing with zeros.
    let mut random = |buf: &mut [u8]| {
        getrandom::fill(buf)
            .expect("the operating system's random number generator is unavailable");
    };
    let mut env = AuthEnv {
        now: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_default(),
        random: &mut random,
    };
    f(&mut env)
}

fn cnonce() -> String {
    let mut b = [0u8; 8];
    let _ = getrandom::fill(&mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn auth_err(kind: &str) -> impl Fn(AuthError) -> EngineError + '_ {
    move |e| EngineError::Auth(format!("{kind}: {e}"))
}

fn with_authorization(req: &Request, value: String) -> Request {
    let mut r = req.clone();
    r.headers
        .retain(|k, _| !k.eq_ignore_ascii_case("authorization"));
    r.headers.insert("Authorization".into(), value);
    r
}

fn www_authenticate(resp: &Response) -> Vec<String> {
    resp.headers
        .iter()
        .filter(|(k, _)| k.eq_ignore_ascii_case("www-authenticate"))
        .map(|(_, v)| v.clone())
        .collect()
}

/// Path and query of the request URL (the Digest `uri`).
fn request_uri(url: &str) -> String {
    match reqwest::Url::parse(url) {
        Ok(u) => match u.query() {
            Some(q) => format!("{}?{q}", u.path()),
            None => u.path().to_string(),
        },
        Err(_) => "/".into(),
    }
}

pub(crate) async fn send(
    engine: &Engine,
    req: &Request,
    opts: SendOptions,
    auth: &ChallengeAuth,
) -> Result<Response, EngineError> {
    // The challenge (401) responses are read fully but not kept.
    let probe = SendOptions {
        discard_body: true,
        ..opts.clone()
    };
    match auth {
        ChallengeAuth::Digest(cfg) => {
            let method = req.method.as_str();
            let url = full_url(req)?;
            let uri = request_uri(&url);
            let body = body_bytes(req);
            let cnonce = cfg.client_nonce.clone().unwrap_or_else(cnonce);
            if let Some(h) = digest::preemptive(cfg, method, &uri, &body, &cnonce) {
                let h = h.map_err(auth_err("Digest Auth"))?;
                return engine
                    .send_inner(&with_authorization(req, h), opts.clone(), None)
                    .await;
            }
            // When no challenge comes, this is the real response.
            let first = engine.send_inner(req, opts.clone(), None).await?;
            if first.status != 401 || cfg.disable_retry {
                return Ok(first);
            }
            let Some(ch) = digest::parse_challenge(&www_authenticate(&first)) else {
                return Ok(first);
            };
            let h = digest::authorization(cfg, &ch, method, &uri, &body, 1, &cnonce)
                .map_err(auth_err("Digest Auth"))?;
            let retry = SendOptions {
                phase: Some("digest-response"),
                ..opts
            };
            engine
                .send_inner(&with_authorization(req, h), retry, None)
                .await
        }
        ChallengeAuth::Ntlm(cfg) => {
            // One client (and so one pooled connection) for the whole handshake.
            let pool = Mutex::new(HashMap::new());
            let t1 = ntlm::negotiate(cfg).map_err(auth_err("NTLM"))?;
            let negotiate = SendOptions {
                phase: Some("ntlm-negotiate"),
                ..probe
            };
            let first = engine
                .send_inner(&with_authorization(req, t1), negotiate, Some(&pool))
                .await?;
            let www = www_authenticate(&first);
            let has_type2 = www.iter().any(|v| {
                let v = v.trim();
                v.len() > 5 && v[..5].eq_ignore_ascii_case("ntlm ")
            });
            if first.status != 401 || cfg.disable_retry || !has_type2 {
                return Ok(first);
            }
            let t3 = with_auth_env(|env| ntlm::authenticate(cfg, &www, env))
                .map_err(auth_err("NTLM"))?;
            let authenticate = SendOptions {
                phase: Some("ntlm-authenticate"),
                ..opts
            };
            engine
                .send_inner(&with_authorization(req, t3), authenticate, Some(&pool))
                .await
        }
    }
}
