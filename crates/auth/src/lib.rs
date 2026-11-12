//! Pure authorization logic (D39): no I/O, no HTTP client, deterministic given `AuthEnv`.
//!
//! CONTRACT (fixed — the engine/runner/app integrate against these signatures; the
//! implementation agent may add private items, modules and dependencies, not change these).
//! The `Auth` model lives in `ilixpatch_core::auth`; fields are already variable-resolved.

use ilixpatch_core::auth::{AddTo, AsapAuth, Auth};
use serde::{Deserialize, Serialize};
use thiserror::Error;

mod akamai;
mod awsv4;
mod hawk;
mod jwt;
mod oauth1;
mod util;

pub mod digest;
pub mod ntlm;
pub mod oauth2;

#[derive(Debug, Error, Clone, PartialEq)]
pub enum AuthError {
    /// A required field is empty/invalid; `field` is the model field name (e.g. "private_key").
    #[error("{field}: {message}")]
    Invalid { field: String, message: String },
    /// This auth type needs something this function can't do (e.g. OAuth 2.0 without a token).
    #[error("{0}")]
    Unsupported(String),
}

/// The request as it will be sent (after variables and pre-request script).
#[derive(Debug, Clone)]
pub struct HttpParts<'a> {
    pub method: &'a str,
    /// Absolute URL including the query string.
    pub url: &'a str,
    pub headers: &'a [(String, String)],
    pub body: &'a [u8],
}

/// Clock and randomness, injectable for deterministic tests.
pub struct AuthEnv<'a> {
    /// Unix seconds.
    pub now: u64,
    /// Fills the buffer with random bytes (nonces, PKCE verifiers…).
    pub random: &'a mut dyn FnMut(&mut [u8]),
}

/// Changes to apply to the outgoing request.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AuthOutput {
    /// Set (replace, case-insensitive) these headers.
    pub headers: Vec<(String, String)>,
    /// Append these query parameters (already decoded; the caller encodes).
    pub query: Vec<(String, String)>,
    /// Replace the body (OAuth 1.0 "add to body"), with its content type.
    pub body: Option<(Vec<u8>, String)>,
}

impl AuthOutput {
    fn header(name: &str, value: String) -> Self {
        AuthOutput {
            headers: vec![(name.to_string(), value)],
            ..Default::default()
        }
    }
}

/// Single-shot auth types: no_auth, api_key, bearer, basic, jwt, oauth1, hawk, awsv4, akamai, asap.
/// `Inherit` must be resolved by the caller first (→ `AuthError::Unsupported`).
/// Digest/NTLM → use the `digest` / `ntlm` modules; OAuth2 → `oauth2::apply_token`.
///
/// Conveniences: Digest with `realm` + `nonce` configured returns the pre-emptive header;
/// OAuth 2.0 with a literal `access_token` applies it. Otherwise those return `Unsupported`.
pub fn sign(auth: &Auth, req: &HttpParts, env: &mut AuthEnv) -> Result<AuthOutput, AuthError> {
    match auth {
        Auth::Inherit => Err(AuthError::Unsupported(
            "\"inherit\" must be resolved to the parent folder/collection auth before signing"
                .into(),
        )),
        Auth::NoAuth => Ok(AuthOutput::default()),
        Auth::ApiKey(c) => {
            // Like Postman: nothing is sent while the key name is empty.
            if c.key.trim().is_empty() {
                return Ok(AuthOutput::default());
            }
            let pair = (c.key.trim().to_string(), c.value.clone());
            Ok(match c.add_to {
                AddTo::Query => AuthOutput {
                    query: vec![pair],
                    ..Default::default()
                },
                _ => AuthOutput {
                    headers: vec![pair],
                    ..Default::default()
                },
            })
        }
        Auth::Bearer(c) => {
            let token = c.token.trim();
            // Like Postman: an empty token sends no header.
            if token.is_empty() {
                return Ok(AuthOutput::default());
            }
            Ok(AuthOutput::header(
                "Authorization",
                format!("Bearer {token}"),
            ))
        }
        Auth::Basic(c) => {
            let cred = util::b64(format!("{}:{}", c.username, c.password).as_bytes());
            Ok(AuthOutput::header("Authorization", format!("Basic {cred}")))
        }
        Auth::Jwt(c) => jwt::sign_auth(c, env),
        Auth::Digest(c) => {
            let u = util::parse_url(req.url)?;
            let cnonce = util::hex(&util::random_bytes(env, 8));
            match digest::preemptive(c, req.method, &util::request_target(&u), req.body, &cnonce) {
                Some(v) => Ok(AuthOutput::header("Authorization", v?)),
                None => Err(AuthError::Unsupported(
                    "Digest needs the server's 401 challenge (or realm and nonce configured): use the digest module".into(),
                )),
            }
        }
        Auth::Oauth1(c) => oauth1::sign(c, req, env),
        Auth::Oauth2(c) => match util::non_empty(&c.access_token) {
            Some(token) => Ok(oauth2::apply_token(c, token)),
            None => Err(AuthError::Unsupported(
                "OAuth 2.0 needs an access token: get a new access token first".into(),
            )),
        },
        Auth::Hawk(c) => hawk::sign(c, req, env),
        Auth::Awsv4(c) => awsv4::sign(c, req, env.now),
        Auth::Ntlm(_) => Err(AuthError::Unsupported(
            "NTLM is a connection handshake: use ntlm::negotiate and ntlm::authenticate".into(),
        )),
        Auth::Akamai(c) => akamai::sign(c, req, env),
        Auth::Asap(c) => Ok(AuthOutput::header(
            "Authorization",
            format!("Bearer {}", asap_token(c, env)?),
        )),
    }
}

/// ASAP helper exposed for reuse (also used by `sign`).
///
/// Claims: `iss`, `sub` (default `iss`), `aud`, `iat` (now), `exp` (now + `expires_in`, default
/// 3600), `jti` (random UUID v4), then `additional_claims` merged on top. Header: `alg`, `typ`,
/// `kid` = `key_id`.
pub fn asap_token(cfg: &AsapAuth, env: &mut AuthEnv) -> Result<String, AuthError> {
    use serde_json::{Map, Value};
    let alg = jwt::algorithm(&cfg.algorithm, "RS256")?;
    if alg.starts_with("HS") {
        return Err(util::invalid(
            "algorithm",
            "ASAP uses asymmetric keys (RS*, PS* or ES*)",
        ));
    }
    for (field, value) in [
        ("issuer", &cfg.issuer),
        ("audience", &cfg.audience),
        ("key_id", &cfg.key_id),
    ] {
        if value.trim().is_empty() {
            return Err(util::invalid(field, "is required for ASAP"));
        }
    }
    let extra = jwt::json_object(
        cfg.additional_claims.as_deref().unwrap_or_default(),
        "additional_claims",
    )?;
    let iat = env.now;
    let exp = iat + cfg.expires_in.filter(|e| *e > 0).unwrap_or(3600);
    let mut claims = Map::new();
    claims.insert("iss".into(), Value::String(cfg.issuer.clone()));
    let sub = util::non_empty(&cfg.subject).unwrap_or(&cfg.issuer);
    claims.insert("sub".into(), Value::String(sub.to_string()));
    claims.insert("aud".into(), Value::String(cfg.audience.clone()));
    claims.insert("iat".into(), Value::from(iat));
    claims.insert("exp".into(), Value::from(exp));
    claims.insert("jti".into(), Value::String(util::uuid_v4(env)));
    claims.extend(extra);
    let mut header = Map::new();
    header.insert("kid".into(), Value::String(cfg.key_id.clone()));
    jwt::encode(
        &alg,
        header,
        &claims,
        jwt::Key::Pem(&cfg.private_key, "private_key"),
        env,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::test_support::*;
    use base64::Engine as _;
    use ilixpatch_core::auth::*;

    fn run(auth: &Auth) -> Result<AuthOutput, AuthError> {
        let mut r = stream();
        let mut env = AuthEnv {
            now: 1_700_000_000,
            random: &mut r,
        };
        sign(
            auth,
            &parts("GET", "https://example.com/a?b=c", &[], b""),
            &mut env,
        )
    }

    #[test]
    fn simple_types() {
        assert!(matches!(
            run(&Auth::Inherit),
            Err(AuthError::Unsupported(_))
        ));
        assert_eq!(run(&Auth::NoAuth).unwrap(), AuthOutput::default());
        let k = |add_to| {
            Auth::ApiKey(ApiKeyAuth {
                key: "X-Key".into(),
                value: "v 1".into(),
                add_to,
            })
        };
        assert_eq!(
            run(&k(AddTo::Header)).unwrap().headers,
            h(&[("X-Key", "v 1")])
        );
        assert_eq!(run(&k(AddTo::Query)).unwrap().query, h(&[("X-Key", "v 1")]));
        assert_eq!(
            run(&Auth::Bearer(BearerAuth {
                token: "abc".into()
            }))
            .unwrap()
            .headers,
            h(&[("Authorization", "Bearer abc")])
        );
        assert_eq!(
            run(&Auth::Bearer(BearerAuth { token: " ".into() })).unwrap(),
            AuthOutput::default()
        );
        // RFC 7617 §2: Aladdin / open sesame.
        let b = Auth::Basic(BasicAuth {
            username: "Aladdin".into(),
            password: "open sesame".into(),
        });
        assert_eq!(
            run(&b).unwrap().headers,
            h(&[("Authorization", "Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ==")])
        );
        // RFC 7617 §2.1 UTF-8 example: test / 123£.
        let b = Auth::Basic(BasicAuth {
            username: "test".into(),
            password: "123\u{a3}".into(),
        });
        assert_eq!(
            run(&b).unwrap().headers,
            h(&[("Authorization", "Basic dGVzdDoxMjPCow==")])
        );
        assert!(matches!(
            run(&Auth::Ntlm(NtlmAuth::default())),
            Err(AuthError::Unsupported(_))
        ));
        assert!(matches!(
            run(&Auth::Oauth2(OAuth2Auth::default())),
            Err(AuthError::Unsupported(_))
        ));
        let o = Auth::Oauth2(OAuth2Auth {
            access_token: Some("tok".into()),
            ..Default::default()
        });
        assert_eq!(
            run(&o).unwrap().headers,
            h(&[("Authorization", "Bearer tok")])
        );
        assert!(matches!(
            run(&Auth::Digest(DigestAuth::default())),
            Err(AuthError::Unsupported(_))
        ));
        let d = Auth::Digest(DigestAuth {
            username: "u".into(),
            realm: Some("r".into()),
            nonce: Some("n".into()),
            ..Default::default()
        });
        assert!(
            run(&d).unwrap().headers[0]
                .1
                .starts_with("Digest username=\"u\"")
        );
    }

    #[test]
    fn asap() {
        let cfg = AsapAuth {
            algorithm: String::new(),
            issuer: "svc-a".into(),
            subject: None,
            audience: "svc-b".into(),
            key_id: "svc-a/key1".into(),
            private_key: RSA_PKCS8.into(),
            additional_claims: Some(r#"{"scope":"read"}"#.into()),
            expires_in: Some(60),
        };
        let out = run(&Auth::Asap(cfg.clone())).unwrap();
        let token = out.headers[0]
            .1
            .strip_prefix("Bearer ")
            .unwrap()
            .to_string();
        let parts: Vec<&str> = token.split('.').collect();
        let dec = |p: &str| -> serde_json::Value {
            serde_json::from_slice(
                &base64::engine::general_purpose::URL_SAFE_NO_PAD
                    .decode(p)
                    .unwrap(),
            )
            .unwrap()
        };
        let header = dec(parts[0]);
        assert_eq!(
            (
                header["alg"].as_str(),
                header["kid"].as_str(),
                header["typ"].as_str()
            ),
            (Some("RS256"), Some("svc-a/key1"), Some("JWT"))
        );
        let c = dec(parts[1]);
        assert_eq!(c["iss"], "svc-a");
        assert_eq!(c["sub"], "svc-a");
        assert_eq!(c["aud"], "svc-b");
        assert_eq!(c["iat"], 1_700_000_000u64);
        assert_eq!(c["exp"], 1_700_000_060u64);
        assert_eq!(c["scope"], "read");
        // Random UUID v4.
        let jti = c["jti"].as_str().unwrap();
        assert_eq!(
            (jti.len(), &jti[14..15], &jti[8..9]),
            (36, "4", "-"),
            "{jti}"
        );

        let field = |cfg: AsapAuth| match run(&Auth::Asap(cfg)) {
            Err(AuthError::Invalid { field, .. }) => field,
            other => panic!("{other:?}"),
        };
        assert_eq!(
            field(AsapAuth {
                issuer: "".into(),
                ..cfg.clone()
            }),
            "issuer"
        );
        assert_eq!(
            field(AsapAuth {
                private_key: "x".into(),
                ..cfg.clone()
            }),
            "private_key"
        );
        assert_eq!(
            field(AsapAuth {
                additional_claims: Some("{".into()),
                ..cfg.clone()
            }),
            "additional_claims"
        );
        assert_eq!(
            field(AsapAuth {
                algorithm: "HS256".into(),
                ..cfg
            }),
            "algorithm"
        );
    }
}
