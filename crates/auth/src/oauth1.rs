//! OAuth 1.0a request signing (RFC 5849, OAuth Request Body Hash extension).

use crate::util::{self, HashAlg, PrivateKey, b64, invalid, non_empty, pct};
use crate::{AuthEnv, AuthError, AuthOutput, HttpParts};
use ilixpatch_core::auth::{AddTo, OAuth1Auth};

enum Method {
    Hmac(HashAlg),
    Rsa(HashAlg),
    Plaintext,
}

fn method(name: &str) -> Result<(Method, &'static str), AuthError> {
    Ok(match name.trim().to_ascii_uppercase().as_str() {
        "" | "HMAC-SHA1" => (Method::Hmac(HashAlg::Sha1), "HMAC-SHA1"),
        "HMAC-SHA256" => (Method::Hmac(HashAlg::Sha256), "HMAC-SHA256"),
        "HMAC-SHA512" => (Method::Hmac(HashAlg::Sha512), "HMAC-SHA512"),
        "RSA-SHA1" => (Method::Rsa(HashAlg::Sha1), "RSA-SHA1"),
        "RSA-SHA256" => (Method::Rsa(HashAlg::Sha256), "RSA-SHA256"),
        "RSA-SHA512" => (Method::Rsa(HashAlg::Sha512), "RSA-SHA512"),
        "PLAINTEXT" => (Method::Plaintext, "PLAINTEXT"),
        _ => {
            return Err(invalid(
                "signature_method",
                format!(
                    "unsupported signature method \"{name}\" (HMAC-SHA1/256/512, RSA-SHA1/256/512, PLAINTEXT)"
                ),
            ));
        }
    })
}

/// RFC 5849 §3.4.1.2 base string URI: lowercase scheme/host, default port omitted, no query.
fn base_uri(u: &url::Url) -> String {
    format!("{}://{}{}", u.scheme(), util::host_header(u), u.path())
}

/// RFC 5849 §3.4.1.3.2: encode, sort by name then value, join.
fn normalize(params: &[(String, String)]) -> String {
    let mut enc: Vec<(String, String)> = params.iter().map(|(k, v)| (pct(k), pct(v))).collect();
    enc.sort();
    enc.iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&")
}

pub(crate) fn base_string(method: &str, u: &url::Url, params: &[(String, String)]) -> String {
    format!(
        "{}&{}&{}",
        method.to_ascii_uppercase(),
        pct(&base_uri(u)),
        pct(&normalize(params))
    )
}

pub(crate) fn sign(
    cfg: &OAuth1Auth,
    req: &HttpParts,
    env: &mut AuthEnv,
) -> Result<AuthOutput, AuthError> {
    let (m, method_name) = method(&cfg.signature_method)?;
    if cfg.consumer_key.trim().is_empty() {
        return Err(invalid("consumer_key", "is required for OAuth 1.0"));
    }
    let u = util::parse_url(req.url)?;
    let form_body = util::is_form(req);
    if cfg.add_to == AddTo::Body && !form_body && !req.body.is_empty() {
        return Err(invalid(
            "add_to",
            "OAuth parameters can only be added to an x-www-form-urlencoded body",
        ));
    }

    let timestamp = non_empty(&cfg.timestamp)
        .map(str::to_string)
        .unwrap_or_else(|| env.now.to_string());
    let nonce = non_empty(&cfg.nonce)
        .map(str::to_string)
        .unwrap_or_else(|| util::random_alnum(env, 16));
    let mut oauth: Vec<(String, String)> =
        vec![("oauth_consumer_key".into(), cfg.consumer_key.trim().into())];
    let optional = |name: &str, v: &Option<String>, out: &mut Vec<(String, String)>| {
        let v = v.as_deref().unwrap_or_default();
        if !v.is_empty() || cfg.add_empty_params {
            out.push((name.into(), v.into()));
        }
    };
    optional("oauth_token", &cfg.token, &mut oauth);
    oauth.push(("oauth_signature_method".into(), method_name.into()));
    oauth.push(("oauth_timestamp".into(), timestamp));
    oauth.push(("oauth_nonce".into(), nonce));
    match cfg.version.as_deref() {
        None => oauth.push(("oauth_version".into(), "1.0".into())),
        Some(v) if !v.trim().is_empty() => oauth.push(("oauth_version".into(), v.trim().into())),
        Some(_) => {}
    }
    optional("oauth_callback", &cfg.callback_url, &mut oauth);
    optional("oauth_verifier", &cfg.verifier, &mut oauth);
    if cfg.include_body_hash && !form_body && cfg.add_to != AddTo::Body {
        let alg = match m {
            Method::Hmac(a) | Method::Rsa(a) => a,
            Method::Plaintext => HashAlg::Sha1,
        };
        oauth.push(("oauth_body_hash".into(), b64(&util::hash(alg, req.body))));
    }

    let mut params = util::parse_form(u.query().unwrap_or_default());
    if form_body {
        params.extend(util::parse_form(&String::from_utf8_lossy(req.body)));
    }
    params.extend(oauth.iter().cloned());
    let base = base_string(req.method, &u, &params);

    let key = format!(
        "{}&{}",
        pct(&cfg.consumer_secret),
        pct(cfg.token_secret.as_deref().unwrap_or_default())
    );
    let signature = match m {
        Method::Hmac(a) => b64(&util::hmac(a, key.as_bytes(), base.as_bytes())),
        Method::Plaintext => key,
        Method::Rsa(a) => match util::parse_private_key(
            cfg.private_key.as_deref().unwrap_or_default(),
            "private_key",
        )? {
            PrivateKey::Rsa(k) => b64(&util::rsa_pkcs1v15_sign(&k, a, base.as_bytes(), env)),
            _ => return Err(invalid("private_key", "RSA-SHA* needs an RSA private key")),
        },
    };
    oauth.push(("oauth_signature".into(), signature));

    Ok(match cfg.add_to {
        AddTo::Header => {
            let mut parts = Vec::new();
            if let Some(realm) = non_empty(&cfg.realm) {
                parts.push(format!("realm={}", util::quote(realm)));
            }
            parts.extend(
                oauth
                    .iter()
                    .map(|(k, v)| format!("{}=\"{}\"", pct(k), pct(v))),
            );
            AuthOutput {
                headers: vec![(
                    "Authorization".into(),
                    format!("OAuth {}", parts.join(", ")),
                )],
                ..Default::default()
            }
        }
        AddTo::Query => AuthOutput {
            query: oauth,
            ..Default::default()
        },
        AddTo::Body => {
            let mut body = req.body.to_vec();
            if !body.is_empty() {
                body.push(b'&');
            }
            body.extend_from_slice(util::encode_pairs(&oauth).as_bytes());
            AuthOutput {
                body: Some((body, "application/x-www-form-urlencoded".into())),
                ..Default::default()
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::test_support::*;

    #[test]
    fn rfc5849_3_4_1_base_string() {
        // RFC 5849 §3.4.1.1 example (oauth_version omitted, as in the RFC).
        let u =
            url::Url::parse("http://example.com/request?b5=%3D%253D&a3=a&c%40=&a2=r%20b").unwrap();
        let mut params = util::parse_form(u.query().unwrap());
        params.extend(util::parse_form("c2&a3=2+q"));
        for (k, v) in [
            ("oauth_consumer_key", "9djdj82h48djs9d2"),
            ("oauth_token", "kkk9d7dh3k39sjv7"),
            ("oauth_signature_method", "HMAC-SHA1"),
            ("oauth_timestamp", "137131201"),
            ("oauth_nonce", "7d8f3e4a"),
        ] {
            params.push((k.into(), v.into()));
        }
        assert_eq!(
            base_string("POST", &u, &params),
            "POST&http%3A%2F%2Fexample.com%2Frequest&a2%3Dr%2520b%26a3%3D2%2520q%26a3%3Da%26b5%3D%253D%25253D%26c%2540%3D%26c2%3D%26oauth_consumer_key%3D9djdj82h48djs9d2%26oauth_nonce%3D7d8f3e4a%26oauth_signature_method%3DHMAC-SHA1%26oauth_timestamp%3D137131201%26oauth_token%3Dkkk9d7dh3k39sjv7"
        );
        // Base string URI normalisation (RFC 5849 §3.4.1.2).
        assert_eq!(
            base_uri(&url::Url::parse("HTTP://EXAMPLE.COM:80/r%20v/X?id=123").unwrap()),
            "http://example.com/r%20v/X"
        );
        assert_eq!(
            base_uri(&url::Url::parse("https://www.example.net:8080/?q=1").unwrap()),
            "https://www.example.net:8080/"
        );
    }

    fn twitter() -> OAuth1Auth {
        // Twitter (X) developer docs, "Creating a signature".
        OAuth1Auth {
            signature_method: "HMAC-SHA1".into(),
            consumer_key: "xvz1evFS4wEEPTGEFPHBog".into(),
            consumer_secret: "kAcSOqF21Fu85e7zjz7ZN2U4ZRhfV3WpwPAoE3Z7kBw".into(),
            token: Some("370773112-GmHxMAgYyLbNEtIKZeRNFsMKPR9EyMZeS9weJAEb".into()),
            token_secret: Some("LswwdoUaIvS8ltyTt5jkRh4J50vUPVVHtR2YPi5kE".into()),
            timestamp: Some("1318622958".into()),
            nonce: Some("kYjzVBB8Y0ZFabxSWbWovY3uYSQ2pTgmZeNu2VS4cg".into()),
            ..Default::default()
        }
    }

    fn run(
        cfg: &OAuth1Auth,
        method: &str,
        url: &str,
        headers: &[(String, String)],
        body: &[u8],
    ) -> Result<AuthOutput, AuthError> {
        let mut r = stream();
        let mut env = AuthEnv {
            now: 0,
            random: &mut r,
        };
        sign(cfg, &parts(method, url, headers, body), &mut env)
    }

    #[test]
    fn twitter_example() {
        let headers = h(&[("Content-Type", "application/x-www-form-urlencoded")]);
        let body = b"status=Hello%20Ladies%20%2b%20Gentlemen%2c%20a%20signed%20OAuth%20request%21";
        let out = run(
            &twitter(),
            "POST",
            "https://api.twitter.com/1.1/statuses/update.json?include_entities=true",
            &headers,
            body,
        )
        .unwrap();
        let v = &out.headers[0].1;
        assert!(
            v.starts_with("OAuth oauth_consumer_key=\"xvz1evFS4wEEPTGEFPHBog\""),
            "{v}"
        );
        assert!(
            v.contains("oauth_signature=\"hCtSmYh%2BiHYCEqBWrE7C7hYmtUk%3D\""),
            "{v}"
        );
        assert!(v.contains("oauth_version=\"1.0\""));
    }

    #[test]
    fn modes_and_options() {
        // Query: decoded values, oauth_signature included.
        let cfg = OAuth1Auth {
            add_to: AddTo::Query,
            realm: Some("R".into()),
            ..twitter()
        };
        let out = run(&cfg, "GET", "https://api.example.com/x", &[], b"").unwrap();
        assert!(out.headers.is_empty());
        assert!(out.query.iter().any(|(k, _)| k == "oauth_signature"));
        // Body: appended to the form body.
        let cfg = OAuth1Auth {
            add_to: AddTo::Body,
            ..twitter()
        };
        let headers = h(&[("Content-Type", "application/x-www-form-urlencoded")]);
        let out = run(&cfg, "POST", "https://api.example.com/x", &headers, b"a=1").unwrap();
        let (body, ct) = out.body.unwrap();
        assert!(
            String::from_utf8(body)
                .unwrap()
                .starts_with("a=1&oauth_consumer_key=")
        );
        assert_eq!(ct, "application/x-www-form-urlencoded");
        let json = h(&[("Content-Type", "application/json")]);
        assert!(
            matches!(run(&cfg, "POST", "https://e.com/", &json, b"{}"), Err(AuthError::Invalid { field, .. }) if field == "add_to")
        );
        // Realm in the header; body hash for non-form bodies; empty params.
        let cfg = OAuth1Auth {
            realm: Some("Example".into()),
            include_body_hash: true,
            token: None,
            add_empty_params: true,
            ..twitter()
        };
        let v = run(&cfg, "POST", "https://e.com/", &json, b"Hello World!")
            .unwrap()
            .headers[0]
            .1
            .clone();
        assert!(v.starts_with("OAuth realm=\"Example\", "), "{v}");
        // SHA-1 of "Hello World!" (OAuth body hash spec example).
        assert!(
            v.contains("oauth_body_hash=\"Lve95gjOVATpfV8EL5X4nxwjKHE%3D\""),
            "{v}"
        );
        assert!(v.contains("oauth_token=\"\""), "{v}");
        // PLAINTEXT: signature = encoded secrets.
        let cfg = OAuth1Auth {
            signature_method: "PLAINTEXT".into(),
            consumer_secret: "a b".into(),
            token_secret: Some("c".into()),
            ..twitter()
        };
        let v = run(&cfg, "GET", "https://e.com/", &[], b"")
            .unwrap()
            .headers[0]
            .1
            .clone();
        assert!(v.contains("oauth_signature=\"a%2520b%26c\""), "{v}");
    }

    #[test]
    fn rsa_and_errors() {
        use rsa::signature::Verifier;
        let cfg = OAuth1Auth {
            signature_method: "RSA-SHA256".into(),
            private_key: Some(RSA_PKCS8.into()),
            add_to: AddTo::Query,
            ..twitter()
        };
        let out = run(&cfg, "GET", "https://e.com/p?x=1", &[], b"").unwrap();
        let sig = out
            .query
            .iter()
            .find(|(k, _)| k == "oauth_signature")
            .unwrap()
            .1
            .clone();
        let sig = util::b64_decode_lenient(&sig).unwrap();
        let u = url::Url::parse("https://e.com/p?x=1").unwrap();
        let mut params = vec![("x".to_string(), "1".to_string())];
        params.extend(
            out.query
                .iter()
                .filter(|(k, _)| k != "oauth_signature")
                .cloned(),
        );
        let base = base_string("GET", &u, &params);
        let PrivateKey::Rsa(k) = util::parse_private_key(RSA_PKCS8, "k").ok().unwrap() else {
            panic!()
        };
        let vk = rsa::pkcs1v15::VerifyingKey::<sha2::Sha256>::new(k.to_public_key());
        assert!(
            vk.verify(
                base.as_bytes(),
                &rsa::pkcs1v15::Signature::try_from(sig.as_slice()).unwrap()
            )
            .is_ok()
        );

        let field = |cfg: OAuth1Auth| match run(&cfg, "GET", "https://e.com/", &[], b"") {
            Err(AuthError::Invalid { field, .. }) => field,
            other => panic!("{other:?}"),
        };
        assert_eq!(
            field(OAuth1Auth {
                signature_method: "MD5".into(),
                ..twitter()
            }),
            "signature_method"
        );
        assert_eq!(
            field(OAuth1Auth {
                consumer_key: "".into(),
                ..twitter()
            }),
            "consumer_key"
        );
        assert_eq!(
            field(OAuth1Auth {
                signature_method: "RSA-SHA1".into(),
                ..twitter()
            }),
            "private_key"
        );
    }
}
