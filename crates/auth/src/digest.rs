//! HTTP Digest access authentication (RFC 7616, RFC 2617).

use crate::AuthError;
use crate::util::{hex, invalid, quote};
use ilixpatch_core::auth::DigestAuth;
use sha2::Digest as _;

/// Parsed `WWW-Authenticate: Digest …` challenge.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Challenge {
    pub realm: String,
    pub nonce: String,
    pub opaque: Option<String>,
    pub algorithm: Option<String>,
    /// Offered qop values ("auth", "auth-int").
    pub qop: Vec<String>,
    pub stale: bool,
}

/// One auth challenge: scheme, params, and an optional token68.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RawChallenge {
    pub scheme: String,
    pub params: Vec<(String, String)>,
    pub token68: Option<String>,
}

/// Parse a `WWW-Authenticate` value that may hold several challenges
/// (`Basic realm="a", Digest realm="b", nonce="c"`). Parameter names are lowercased.
pub(crate) fn parse_challenges(value: &str) -> Vec<RawChallenge> {
    let s: Vec<char> = value.chars().collect();
    let mut i = 0;
    let mut out: Vec<RawChallenge> = Vec::new();
    // True right after a scheme name (before any comma/param): a bare token here is a token68.
    let mut just_scheme = false;
    while i < s.len() {
        if s[i].is_whitespace() {
            i += 1;
            continue;
        }
        if s[i] == ',' {
            just_scheme = false;
            i += 1;
            continue;
        }
        let start = i;
        while i < s.len() && !s[i].is_whitespace() && s[i] != '=' && s[i] != ',' {
            i += 1;
        }
        let token: String = s[start..i].iter().collect();
        if token.is_empty() {
            // A stray '='.
            i += 1;
            continue;
        }
        while i < s.len() && s[i].is_whitespace() {
            i += 1;
        }
        if i < s.len() && s[i] == '=' {
            // name=value, or a token68 with '=' padding (`NTLM abc==`).
            let mut j = i;
            while j < s.len() && s[j] == '=' {
                j += 1;
            }
            let mut k = j;
            while k < s.len() && s[k].is_whitespace() {
                k += 1;
            }
            if just_scheme && (k >= s.len() || s[k] == ',') {
                if let Some(c) = out.last_mut() {
                    c.token68 = Some(s[start..j].iter().collect());
                }
                just_scheme = false;
                i = j;
                continue;
            }
            just_scheme = false;
            i += 1;
            while i < s.len() && s[i].is_whitespace() {
                i += 1;
            }
            let mut val = String::new();
            if i < s.len() && s[i] == '"' {
                i += 1;
                while i < s.len() && s[i] != '"' {
                    if s[i] == '\\' && i + 1 < s.len() {
                        i += 1;
                    }
                    val.push(s[i]);
                    i += 1;
                }
                i += 1; // closing quote
            } else {
                while i < s.len() && s[i] != ',' {
                    val.push(s[i]);
                    i += 1;
                }
                val = val.trim().to_string();
            }
            if let Some(c) = out.last_mut() {
                c.params.push((token.to_ascii_lowercase(), val));
            }
        } else if just_scheme {
            if let Some(c) = out.last_mut() {
                c.token68 = Some(token);
            }
            just_scheme = false;
        } else {
            out.push(RawChallenge {
                scheme: token,
                params: Vec::new(),
                token68: None,
            });
            just_scheme = true;
        }
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Algo {
    Md5,
    Sha256,
    Sha512_256,
}

/// (hash, -sess) for a Digest algorithm name.
fn algo(name: &str) -> Option<(Algo, bool)> {
    let n = name.trim().to_ascii_uppercase();
    let (base, sess) = match n.strip_suffix("-SESS") {
        Some(b) => (b.to_string(), true),
        None => (n, false),
    };
    let a = match base.as_str() {
        "" | "MD5" => Algo::Md5,
        "SHA-256" => Algo::Sha256,
        "SHA-512-256" => Algo::Sha512_256,
        _ => return None,
    };
    Some((a, sess))
}

fn h(a: Algo, data: &[u8]) -> String {
    match a {
        Algo::Md5 => hex(&md5::Md5::digest(data)),
        Algo::Sha256 => hex(&sha2::Sha256::digest(data)),
        Algo::Sha512_256 => hex(&sha2::Sha512_256::digest(data)),
    }
}

/// Find and parse the Digest challenge among (possibly several) WWW-Authenticate values.
/// When several Digest challenges are offered, the strongest supported algorithm wins
/// (SHA-512-256 > SHA-256 > MD5).
pub fn parse_challenge(www_authenticate: &[String]) -> Option<Challenge> {
    let mut best: Option<(u8, Challenge)> = None;
    for value in www_authenticate {
        for raw in parse_challenges(value) {
            if !raw.scheme.eq_ignore_ascii_case("digest") {
                continue;
            }
            let mut c = Challenge::default();
            for (k, v) in raw.params {
                match k.as_str() {
                    "realm" => c.realm = v,
                    "nonce" => c.nonce = v,
                    "opaque" => c.opaque = Some(v),
                    "algorithm" => c.algorithm = Some(v),
                    "qop" => {
                        c.qop = v
                            .split(',')
                            .map(|q| q.trim().to_ascii_lowercase())
                            .filter(|q| !q.is_empty())
                            .collect()
                    }
                    "stale" => c.stale = v.eq_ignore_ascii_case("true"),
                    _ => {}
                }
            }
            let rank = match algo(c.algorithm.as_deref().unwrap_or("MD5")) {
                Some((Algo::Sha512_256, _)) => 3,
                Some((Algo::Sha256, _)) => 2,
                Some((Algo::Md5, _)) => 1,
                None => 0,
            };
            if best.as_ref().is_none_or(|(r, _)| rank > *r) {
                best = Some((rank, c));
            }
        }
    }
    best.map(|(_, c)| c)
}

/// Value of the `Authorization` header answering `challenge`.
/// `nc` = nonce count (1 for the first use), `cnonce` = client nonce.
pub fn authorization(
    cfg: &DigestAuth,
    challenge: &Challenge,
    method: &str,
    uri: &str,
    body: &[u8],
    nc: u32,
    cnonce: &str,
) -> Result<String, AuthError> {
    if cfg.username.is_empty() {
        return Err(invalid("username", "is required for Digest auth"));
    }
    if challenge.nonce.is_empty() {
        return Err(invalid("nonce", "the Digest challenge has no nonce"));
    }
    let alg_name = challenge
        .algorithm
        .clone()
        .or_else(|| cfg.algorithm.clone().filter(|a| !a.trim().is_empty()))
        .unwrap_or_else(|| "MD5".into());
    let (a, sess) = algo(&alg_name).ok_or_else(|| {
        invalid("algorithm", format!("unsupported Digest algorithm \"{alg_name}\" (MD5, SHA-256, SHA-512-256, with or without -sess)"))
    })?;

    // qop: the configured one if the server offers it, else "auth", else "auth-int".
    let qop: Option<String> = if challenge.qop.is_empty() {
        None
    } else {
        let wanted = cfg
            .qop
            .as_deref()
            .map(|q| q.trim().to_ascii_lowercase())
            .filter(|q| !q.is_empty());
        let pick = wanted
            .filter(|q| challenge.qop.contains(q))
            .or_else(|| challenge.qop.iter().find(|q| *q == "auth").cloned())
            .or_else(|| challenge.qop.iter().find(|q| *q == "auth-int").cloned());
        match pick {
            Some(q) => Some(q),
            None => {
                return Err(invalid(
                    "qop",
                    format!(
                        "unsupported qop offered by the server: {}",
                        challenge.qop.join(", ")
                    ),
                ));
            }
        }
    };
    let nc_s = format!("{nc:08x}");
    if (qop.is_some() || sess) && cnonce.is_empty() {
        return Err(invalid("client_nonce", "a client nonce is required"));
    }

    let mut ha1 = h(
        a,
        format!("{}:{}:{}", cfg.username, challenge.realm, cfg.password).as_bytes(),
    );
    if sess {
        ha1 = h(a, format!("{ha1}:{}:{cnonce}", challenge.nonce).as_bytes());
    }
    let ha2 = if qop.as_deref() == Some("auth-int") {
        h(a, format!("{method}:{uri}:{}", h(a, body)).as_bytes())
    } else {
        h(a, format!("{method}:{uri}").as_bytes())
    };
    let response = match &qop {
        Some(q) => h(
            a,
            format!("{ha1}:{}:{nc_s}:{cnonce}:{q}:{ha2}", challenge.nonce).as_bytes(),
        ),
        None => h(a, format!("{ha1}:{}:{ha2}", challenge.nonce).as_bytes()),
    };

    let mut parts = vec![
        format!("username={}", quote(&cfg.username)),
        format!("realm={}", quote(&challenge.realm)),
        format!("nonce={}", quote(&challenge.nonce)),
        format!("uri={}", quote(uri)),
        format!("algorithm={alg_name}"),
    ];
    if let Some(q) = &qop {
        parts.push(format!("qop={q}"));
        parts.push(format!("nc={nc_s}"));
        parts.push(format!("cnonce={}", quote(cnonce)));
    }
    parts.push(format!("response={}", quote(&response)));
    if let Some(o) = &challenge.opaque {
        parts.push(format!("opaque={}", quote(o)));
    }
    Ok(format!("Digest {}", parts.join(", ")))
}

/// When realm+nonce are configured, the header can be sent up-front (no 401 round trip).
pub fn preemptive(
    cfg: &DigestAuth,
    method: &str,
    uri: &str,
    body: &[u8],
    cnonce: &str,
) -> Option<Result<String, AuthError>> {
    let realm = cfg.realm.as_deref().filter(|r| !r.is_empty())?;
    let nonce = cfg.nonce.as_deref().filter(|n| !n.is_empty())?;
    let challenge = Challenge {
        realm: realm.into(),
        nonce: nonce.into(),
        opaque: cfg.opaque.clone().filter(|o| !o.is_empty()),
        algorithm: cfg.algorithm.clone().filter(|a| !a.trim().is_empty()),
        qop: cfg
            .qop
            .iter()
            .map(|q| q.trim().to_ascii_lowercase())
            .filter(|q| !q.is_empty())
            .collect(),
        stale: false,
    };
    let nc = match cfg
        .nonce_count
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(s) => match u32::from_str_radix(s, 16) {
            Ok(n) => n,
            Err(_) => {
                return Some(Err(invalid(
                    "nonce_count",
                    "must be a hexadecimal number like 00000001",
                )));
            }
        },
        None => 1,
    };
    let cnonce = cfg
        .client_nonce
        .as_deref()
        .filter(|c| !c.is_empty())
        .unwrap_or(cnonce);
    Some(authorization(
        cfg, &challenge, method, uri, body, nc, cnonce,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn param(header: &str, name: &str) -> String {
        let raw = parse_challenges(header);
        raw[0]
            .params
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
            .unwrap_or_else(|| panic!("no {name} in {header}"))
    }

    #[test]
    fn rfc2617_example() {
        // RFC 2617 §3.5.
        let www = vec![
            r#"Digest realm="testrealm@host.com", qop="auth,auth-int", nonce="dcd98b7102dd2f0e8b11d0f600bfb0c093", opaque="5ccc069c403ebaf9f0171e9517f40e41""#.to_string(),
        ];
        let c = parse_challenge(&www).unwrap();
        assert_eq!(c.realm, "testrealm@host.com");
        assert_eq!(c.qop, vec!["auth", "auth-int"]);
        assert_eq!(
            c.opaque.as_deref(),
            Some("5ccc069c403ebaf9f0171e9517f40e41")
        );
        let cfg = DigestAuth {
            username: "Mufasa".into(),
            password: "Circle Of Life".into(),
            ..Default::default()
        };
        let v = authorization(&cfg, &c, "GET", "/dir/index.html", b"", 1, "0a4f113b").unwrap();
        assert_eq!(param(&v, "response"), "6629fae49393a05397450978507c4ef1");
        assert!(
            v.contains("nc=00000001")
                && v.contains("qop=auth,")
                && v.contains("cnonce=\"0a4f113b\""),
            "{v}"
        );
        assert!(
            v.contains("opaque=\"5ccc069c403ebaf9f0171e9517f40e41\""),
            "{v}"
        );
        assert!(
            v.starts_with("Digest username=\"Mufasa\", realm=\"testrealm@host.com\""),
            "{v}"
        );
    }

    #[test]
    fn rfc7616_examples() {
        // RFC 7616 §3.9.1: the server offers SHA-256 and MD5 challenges.
        let www = vec![
            r#"Digest realm="http-auth@example.org", qop="auth, auth-int", algorithm=SHA-256, nonce="7ypf/xlj9XXwfDPEoM4URrv/xwf94BcCAzFZH4GiTo0v", opaque="FQhe/qaU925kfnzjCev0ciny7QMkPqMAFRtzCUYo5tdS""#.to_string(),
            r#"Digest realm="http-auth@example.org", qop="auth, auth-int", algorithm=MD5, nonce="7ypf/xlj9XXwfDPEoM4URrv/xwf94BcCAzFZH4GiTo0v", opaque="FQhe/qaU925kfnzjCev0ciny7QMkPqMAFRtzCUYo5tdS""#.to_string(),
        ];
        let c = parse_challenge(&www).unwrap();
        assert_eq!(c.algorithm.as_deref(), Some("SHA-256"));
        let cfg = DigestAuth {
            username: "Mufasa".into(),
            password: "Circle of Life".into(),
            ..Default::default()
        };
        let cnonce = "f2/wE4q74E6zIJEtWaHKaf5wv/H5QzzpXusqGemxURZJ";
        let v = authorization(&cfg, &c, "GET", "/dir/index.html", b"", 1, cnonce).unwrap();
        assert_eq!(
            param(&v, "response"),
            "753927fa0e85d155564e2e272a28d1802ca10daf4496794697cf8db5856cb6c1"
        );
        assert!(v.contains("algorithm=SHA-256"));

        let md5 = Challenge {
            algorithm: Some("MD5".into()),
            ..c
        };
        let v = authorization(&cfg, &md5, "GET", "/dir/index.html", b"", 1, cnonce).unwrap();
        assert_eq!(param(&v, "response"), "8ca523f5e9506fed4657c9700eebdbec");
    }

    #[test]
    fn parser_robustness() {
        // Several schemes in one header, unquoted params, odd casing, token68 schemes.
        let www = vec![
            r#"NTLM, Negotiate, Basic realm="x, y", DIGEST Realm=r1, NONCE="n\"1", QOP=auth, Algorithm=md5-sess, stale=TRUE"#.to_string(),
        ];
        let c = parse_challenge(&www).unwrap();
        assert_eq!(c.realm, "r1");
        assert_eq!(c.nonce, "n\"1");
        assert_eq!(c.qop, vec!["auth"]);
        assert_eq!(c.algorithm.as_deref(), Some("md5-sess"));
        assert!(c.stale);
        let raw = parse_challenges("NTLM TlRMTVNTUAACAAAA==, Basic realm=\"a\"");
        assert_eq!(raw[0].scheme, "NTLM");
        assert_eq!(raw[0].token68.as_deref(), Some("TlRMTVNTUAACAAAA=="));
        assert_eq!(raw[1].scheme, "Basic");
        assert_eq!(raw[1].params, vec![("realm".to_string(), "a".to_string())]);
        assert_eq!(
            parse_challenges("Negotiate abc=")[0].token68.as_deref(),
            Some("abc=")
        );
        assert!(parse_challenge(&["Basic realm=\"a\"".into()]).is_none());
    }

    #[test]
    fn variants() {
        let cfg = DigestAuth {
            username: "u".into(),
            password: "p".into(),
            ..Default::default()
        };
        let base = Challenge {
            realm: "r".into(),
            nonce: "n".into(),
            ..Default::default()
        };
        // No qop (RFC 2069 style): response = H(HA1:nonce:HA2).
        let v = authorization(&cfg, &base, "GET", "/", b"", 1, "c").unwrap();
        let ha1 = h(Algo::Md5, b"u:r:p");
        let ha2 = h(Algo::Md5, b"GET:/");
        assert_eq!(
            param(&v, "response"),
            h(Algo::Md5, format!("{ha1}:n:{ha2}").as_bytes())
        );
        assert!(!v.contains("nc="));
        // auth-int with -sess, SHA-512-256.
        let c = Challenge {
            qop: vec!["auth-int".into()],
            algorithm: Some("SHA-512-256-sess".into()),
            ..base.clone()
        };
        let v = authorization(&cfg, &c, "POST", "/x", b"body", 255, "cn").unwrap();
        let a = Algo::Sha512_256;
        let ha1 = h(a, format!("{}:n:cn", h(a, b"u:r:p")).as_bytes());
        let ha2 = h(a, format!("POST:/x:{}", h(a, b"body")).as_bytes());
        assert_eq!(
            param(&v, "response"),
            h(a, format!("{ha1}:n:000000ff:cn:auth-int:{ha2}").as_bytes())
        );
        // Configured qop preference.
        let c = Challenge {
            qop: vec!["auth".into(), "auth-int".into()],
            ..base.clone()
        };
        let cfg2 = DigestAuth {
            qop: Some("auth-int".into()),
            ..cfg.clone()
        };
        assert!(
            authorization(&cfg2, &c, "GET", "/", b"", 1, "c")
                .unwrap()
                .contains("qop=auth-int")
        );
        // Errors.
        let bad = Challenge {
            algorithm: Some("SHA-1".into()),
            ..base.clone()
        };
        assert!(
            matches!(authorization(&cfg, &bad, "GET", "/", b"", 1, "c"), Err(AuthError::Invalid { field, .. }) if field == "algorithm")
        );
        let nouser = DigestAuth::default();
        assert!(
            matches!(authorization(&nouser, &base, "GET", "/", b"", 1, "c"), Err(AuthError::Invalid { field, .. }) if field == "username")
        );
    }

    #[test]
    fn preemptive_uses_config() {
        let cfg = DigestAuth {
            username: "Mufasa".into(),
            password: "Circle Of Life".into(),
            realm: Some("testrealm@host.com".into()),
            nonce: Some("dcd98b7102dd2f0e8b11d0f600bfb0c093".into()),
            qop: Some("auth".into()),
            nonce_count: Some("00000001".into()),
            client_nonce: Some("0a4f113b".into()),
            opaque: Some("5ccc069c403ebaf9f0171e9517f40e41".into()),
            ..Default::default()
        };
        let v = preemptive(&cfg, "GET", "/dir/index.html", b"", "ignored")
            .unwrap()
            .unwrap();
        assert_eq!(param(&v, "response"), "6629fae49393a05397450978507c4ef1");
        assert!(
            preemptive(
                &DigestAuth {
                    nonce: None,
                    ..cfg.clone()
                },
                "GET",
                "/",
                b"",
                "c"
            )
            .is_none()
        );
        let bad = DigestAuth {
            nonce_count: Some("zz".into()),
            ..cfg
        };
        assert!(preemptive(&bad, "GET", "/", b"", "c").unwrap().is_err());
    }
}
