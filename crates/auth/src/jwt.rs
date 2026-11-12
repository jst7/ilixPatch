//! JWT (JWS compact serialization) signing for "JWT Bearer" and ASAP.

use crate::util::{self, HashAlg, PrivateKey, b64url, invalid};
use crate::{AuthEnv, AuthError, AuthOutput};
use ilixpatch_core::auth::{AddTo, JwtAuth};
use serde_json::{Map, Value};

pub(crate) const ALGORITHMS: &[&str] = &[
    "HS256", "HS384", "HS512", "RS256", "RS384", "RS512", "PS256", "PS384", "PS512", "ES256",
    "ES384", "ES512",
];

pub(crate) enum Key<'a> {
    /// HS*: raw secret bytes.
    Secret(&'a [u8]),
    /// RS*/PS*/ES*: PEM private key and the model field it came from (for errors).
    Pem(&'a str, &'a str),
}

/// Parse an optional JSON-object field; empty → `{}`.
pub(crate) fn json_object(text: &str, field: &str) -> Result<Map<String, Value>, AuthError> {
    if text.trim().is_empty() {
        return Ok(Map::new());
    }
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(m)) => Ok(m),
        Ok(other) => {
            let kind = match other {
                Value::Array(_) => "an array",
                Value::String(_) => "a string",
                Value::Number(_) => "a number",
                Value::Bool(_) => "a boolean",
                _ => "null",
            };
            Err(invalid(
                field,
                format!("must be a JSON object ({{…}}), got {kind}"),
            ))
        }
        Err(e) => Err(invalid(field, format!("invalid JSON: {e}"))),
    }
}

/// Normalised algorithm name or `Invalid { field: "algorithm" }`.
pub(crate) fn algorithm(alg: &str, default: &str) -> Result<String, AuthError> {
    let a = alg.trim().to_ascii_uppercase();
    let a = if a.is_empty() { default.to_string() } else { a };
    if ALGORITHMS.contains(&a.as_str()) {
        Ok(a)
    } else {
        Err(invalid(
            "algorithm",
            format!(
                "unsupported JWT algorithm \"{alg}\" (use one of {})",
                ALGORITHMS.join(", ")
            ),
        ))
    }
}

fn hash_for(alg: &str) -> HashAlg {
    match &alg[2..] {
        "384" => HashAlg::Sha384,
        "512" => HashAlg::Sha512,
        _ => HashAlg::Sha256,
    }
}

/// Build and sign a JWT. `alg` must come from [`algorithm`]. `alg` and `typ` are set in the
/// header (`typ` may be overridden by `extra_header`, `alg` may not).
pub(crate) fn encode(
    alg: &str,
    extra_header: Map<String, Value>,
    claims: &Map<String, Value>,
    key: Key,
    env: &mut AuthEnv,
) -> Result<String, AuthError> {
    let mut header = Map::new();
    header.insert("alg".into(), Value::String(alg.into()));
    header.insert("typ".into(), Value::String("JWT".into()));
    for (k, v) in extra_header {
        if k != "alg" {
            header.insert(k, v);
        }
    }
    let header_json = serde_json::to_vec(&Value::Object(header)).expect("serialisable");
    let claims_json = serde_json::to_vec(claims).expect("serialisable");
    let signing_input = format!("{}.{}", b64url(&header_json), b64url(&claims_json));
    let sig = sign(alg, signing_input.as_bytes(), key, env)?;
    Ok(format!("{signing_input}.{}", b64url(&sig)))
}

fn sign(alg: &str, msg: &[u8], key: Key, env: &mut AuthEnv) -> Result<Vec<u8>, AuthError> {
    let h = hash_for(alg);
    match (&alg[..2], key) {
        ("HS", Key::Secret(secret)) => Ok(util::hmac(h, secret, msg)),
        ("HS", Key::Pem(..)) => Err(invalid("secret", format!("{alg} needs a shared secret"))),
        (_, Key::Secret(_)) => Err(invalid(
            "private_key",
            format!("{alg} needs a PEM private key"),
        )),
        (family, Key::Pem(pem, field)) => {
            let key = util::parse_private_key(pem, field)?;
            match (family, key) {
                ("RS", PrivateKey::Rsa(k)) => Ok(util::rsa_pkcs1v15_sign(&k, h, msg, env)),
                ("PS", PrivateKey::Rsa(k)) => Ok(util::rsa_pss_sign(&k, h, msg, env)),
                ("ES", k) => es_sign(alg, k, msg, field),
                (_, _) => Err(invalid(
                    field,
                    format!("{alg} needs an RSA private key, got an EC key"),
                )),
            }
        }
    }
}

/// ECDSA with deterministic nonces (RFC 6979); JWS raw `r || s` signature.
fn es_sign(alg: &str, key: PrivateKey, msg: &[u8], field: &str) -> Result<Vec<u8>, AuthError> {
    use p256::ecdsa::signature::Signer;
    match (alg, key) {
        ("ES256", PrivateKey::P256(k)) => {
            let sig: p256::ecdsa::Signature = p256::ecdsa::SigningKey::from(k).sign(msg);
            Ok(sig.to_bytes().to_vec())
        }
        ("ES384", PrivateKey::P384(k)) => {
            let sig: p384::ecdsa::Signature = p384::ecdsa::SigningKey::from(k).sign(msg);
            Ok(sig.to_bytes().to_vec())
        }
        ("ES512", PrivateKey::P521(k)) => {
            let sk = p521::ecdsa::SigningKey::from_bytes(&k.to_bytes())
                .map_err(|_| invalid(field, "invalid P-521 private key"))?;
            let sig: p521::ecdsa::Signature = sk.sign(msg);
            Ok(sig.to_bytes().to_vec())
        }
        (_, PrivateKey::Rsa(_)) => Err(invalid(
            field,
            format!("{alg} needs an EC private key, got an RSA key"),
        )),
        _ => {
            let curve = match alg {
                "ES256" => "P-256",
                "ES384" => "P-384",
                _ => "P-521",
            };
            Err(invalid(
                field,
                format!("{alg} needs an EC {curve} private key (different curve given)"),
            ))
        }
    }
}

/// "JWT Bearer" auth type.
pub(crate) fn sign_auth(cfg: &JwtAuth, env: &mut AuthEnv) -> Result<AuthOutput, AuthError> {
    let alg = algorithm(&cfg.algorithm, "HS256")?;
    let claims = json_object(&cfg.payload, "payload")?;
    let extra = json_object(cfg.header.as_deref().unwrap_or_default(), "header")?;
    let secret_bytes;
    let key = if alg.starts_with("HS") {
        let secret = cfg.secret.as_deref().unwrap_or_default();
        if secret.is_empty() {
            return Err(invalid("secret", format!("{alg} needs a secret")));
        }
        secret_bytes = if cfg.secret_base64 {
            util::b64_decode_lenient(secret).ok_or_else(|| {
                invalid(
                    "secret",
                    "not valid base64 (\"Secret base64 encoded\" is on)",
                )
            })?
        } else {
            secret.as_bytes().to_vec()
        };
        Key::Secret(&secret_bytes)
    } else {
        Key::Pem(
            cfg.private_key.as_deref().unwrap_or_default(),
            "private_key",
        )
    };
    let token = encode(&alg, extra, &claims, key, env)?;
    Ok(match cfg.add_to {
        AddTo::Query => {
            let name = util::non_empty(&cfg.query_param).unwrap_or("token");
            AuthOutput {
                query: vec![(name.to_string(), token)],
                ..Default::default()
            }
        }
        _ => {
            let prefix = cfg
                .header_prefix
                .as_deref()
                .map(str::trim)
                .unwrap_or("Bearer");
            let value = if prefix.is_empty() {
                token
            } else {
                format!("{prefix} {token}")
            };
            AuthOutput::header("Authorization", value)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::test_support::*;
    use base64::Engine as _;

    fn env_sign(cfg: &JwtAuth) -> Result<AuthOutput, AuthError> {
        let mut r = stream();
        let mut env = AuthEnv {
            now: 0,
            random: &mut r,
        };
        sign_auth(cfg, &mut env)
    }

    fn decode_part(p: &str) -> Value {
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(p)
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[test]
    fn hs256_rfc7515_style() {
        // jwt.io default example: HS256 over {"alg":"HS256","typ":"JWT"} with secret
        // "your-256-bit-secret". Header key order differs from jwt.io (serde_json map order),
        // so check the HMAC over our signing input instead of a fixed string.
        let cfg = JwtAuth {
            algorithm: "HS256".into(),
            secret: Some("your-256-bit-secret".into()),
            payload: r#"{"sub":"1234567890","name":"John Doe","iat":1516239022}"#.into(),
            ..Default::default()
        };
        let out = env_sign(&cfg).unwrap();
        let (name, value) = &out.headers[0];
        assert_eq!(name, "Authorization");
        let token = value.strip_prefix("Bearer ").unwrap();
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(decode_part(parts[0])["alg"], "HS256");
        assert_eq!(decode_part(parts[0])["typ"], "JWT");
        assert_eq!(decode_part(parts[1])["name"], "John Doe");
        let mac = util::hmac(
            HashAlg::Sha256,
            b"your-256-bit-secret",
            format!("{}.{}", parts[0], parts[1]).as_bytes(),
        );
        assert_eq!(parts[2], b64url(&mac));
    }

    #[test]
    fn hs_known_answer() {
        // Fixed signing input → known HMAC-SHA256 (RFC 4231-independent check via jwt.io):
        // header {"alg":"HS256","typ":"JWT"}, payload {"sub":"1234567890","name":"John Doe","iat":1516239022}
        let input = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ";
        let mac = util::hmac(HashAlg::Sha256, b"your-256-bit-secret", input.as_bytes());
        assert_eq!(b64url(&mac), "SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c");
    }

    #[test]
    fn base64_secret_query_and_prefix() {
        let cfg = JwtAuth {
            algorithm: "hs512".into(),
            secret: Some("c2VjcmV0".into()), // "secret"
            secret_base64: true,
            payload: "{}".into(),
            header: Some(r#"{"kid":"k1"}"#.into()),
            add_to: AddTo::Query,
            ..Default::default()
        };
        let out = env_sign(&cfg).unwrap();
        let (name, token) = &out.query[0];
        assert_eq!(name, "token");
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(decode_part(parts[0])["kid"], "k1");
        let mac = util::hmac(
            HashAlg::Sha512,
            b"secret",
            format!("{}.{}", parts[0], parts[1]).as_bytes(),
        );
        assert_eq!(parts[2], b64url(&mac));

        let cfg = JwtAuth {
            add_to: AddTo::Header,
            header_prefix: Some("JWT".into()),
            ..cfg
        };
        assert!(env_sign(&cfg).unwrap().headers[0].1.starts_with("JWT ey"));
    }

    #[test]
    fn rs_ps_es_signatures_verify() {
        use rsa::signature::Verifier;
        let cases = [
            ("RS256", RSA_PKCS8),
            ("RS512", RSA_PKCS1),
            ("PS256", RSA_PKCS8),
            ("PS384", RSA_PKCS1),
            ("ES256", EC_P256_SEC1),
            ("ES256", EC_P256_PKCS8),
            ("ES384", EC_P384_PKCS8),
            ("ES512", EC_P521_SEC1),
        ];
        for (alg, pem) in cases {
            let cfg = JwtAuth {
                algorithm: alg.into(),
                private_key: Some(pem.into()),
                payload: r#"{"sub":"x"}"#.into(),
                ..Default::default()
            };
            let out = env_sign(&cfg).unwrap_or_else(|e| panic!("{alg}: {e}"));
            let token = out.headers[0]
                .1
                .strip_prefix("Bearer ")
                .unwrap()
                .to_string();
            let (input, sig) = token.rsplit_once('.').unwrap();
            let sig = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(sig)
                .unwrap();
            let key = util::parse_private_key(pem, "k").ok().unwrap();
            let ok = match key {
                PrivateKey::Rsa(k) => {
                    let public = k.to_public_key();
                    if alg.starts_with("RS") {
                        let s = rsa::pkcs1v15::Signature::try_from(sig.as_slice()).unwrap();
                        match alg {
                            "RS256" => rsa::pkcs1v15::VerifyingKey::<sha2::Sha256>::new(public)
                                .verify(input.as_bytes(), &s),
                            _ => rsa::pkcs1v15::VerifyingKey::<sha2::Sha512>::new(public)
                                .verify(input.as_bytes(), &s),
                        }
                        .is_ok()
                    } else {
                        let s = rsa::pss::Signature::try_from(sig.as_slice()).unwrap();
                        match alg {
                            "PS256" => rsa::pss::VerifyingKey::<sha2::Sha256>::new(public)
                                .verify(input.as_bytes(), &s),
                            _ => rsa::pss::VerifyingKey::<sha2::Sha384>::new(public)
                                .verify(input.as_bytes(), &s),
                        }
                        .is_ok()
                    }
                }
                PrivateKey::P256(k) => {
                    let s = p256::ecdsa::Signature::from_slice(&sig).unwrap();
                    p256::ecdsa::VerifyingKey::from(k.public_key())
                        .verify(input.as_bytes(), &s)
                        .is_ok()
                }
                PrivateKey::P384(k) => {
                    let s = p384::ecdsa::Signature::from_slice(&sig).unwrap();
                    p384::ecdsa::VerifyingKey::from(k.public_key())
                        .verify(input.as_bytes(), &s)
                        .is_ok()
                }
                PrivateKey::P521(k) => {
                    assert_eq!(sig.len(), 132);
                    let s = p521::ecdsa::Signature::from_slice(&sig).unwrap();
                    let vk = p521::ecdsa::VerifyingKey::from_affine(*k.public_key().as_affine())
                        .unwrap();
                    vk.verify(input.as_bytes(), &s).is_ok()
                }
            };
            assert!(ok, "{alg} signature does not verify");
        }
    }

    #[test]
    fn errors_name_the_field() {
        let field = |cfg: JwtAuth| match env_sign(&cfg) {
            Err(AuthError::Invalid { field, .. }) => field,
            other => panic!("{other:?}"),
        };
        let base = JwtAuth {
            algorithm: "HS256".into(),
            secret: Some("s".into()),
            ..Default::default()
        };
        assert_eq!(
            field(JwtAuth {
                secret: None,
                ..base.clone()
            }),
            "secret"
        );
        assert_eq!(
            field(JwtAuth {
                payload: "{nope".into(),
                ..base.clone()
            }),
            "payload"
        );
        assert_eq!(
            field(JwtAuth {
                payload: "[1]".into(),
                ..base.clone()
            }),
            "payload"
        );
        assert_eq!(
            field(JwtAuth {
                header: Some("1".into()),
                ..base.clone()
            }),
            "header"
        );
        assert_eq!(
            field(JwtAuth {
                algorithm: "none".into(),
                ..base.clone()
            }),
            "algorithm"
        );
        assert_eq!(
            field(JwtAuth {
                secret_base64: true,
                secret: Some("!!!".into()),
                ..base.clone()
            }),
            "secret"
        );
        assert_eq!(
            field(JwtAuth {
                algorithm: "RS256".into(),
                ..base.clone()
            }),
            "private_key"
        );
        assert_eq!(
            field(JwtAuth {
                algorithm: "RS256".into(),
                private_key: Some("garbage".into()),
                ..base.clone()
            }),
            "private_key"
        );
        assert_eq!(
            field(JwtAuth {
                algorithm: "RS256".into(),
                private_key: Some(EC_P256_PKCS8.into()),
                ..base.clone()
            }),
            "private_key"
        );
        assert_eq!(
            field(JwtAuth {
                algorithm: "ES384".into(),
                private_key: Some(EC_P256_PKCS8.into()),
                ..base.clone()
            }),
            "private_key"
        );
        let msg = env_sign(&JwtAuth {
            payload: "[1]".into(),
            ..base
        })
        .unwrap_err()
        .to_string();
        assert!(msg.contains("JSON object"), "{msg}");
    }
}
