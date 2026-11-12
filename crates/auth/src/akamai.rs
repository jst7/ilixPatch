//! Akamai EdgeGrid (`EG1-HMAC-SHA256`).

use crate::util::{self, HashAlg, b64, invalid, non_empty};
use crate::{AuthEnv, AuthError, AuthOutput, HttpParts};
use ilixpatch_core::auth::AkamaiAuth;

const DEFAULT_MAX_BODY: u64 = 131_072;

/// `yyyyMMddTHH:mm:ss+0000`.
fn timestamp(now: u64) -> String {
    let (y, mo, d, h, mi, s) = util::utc(now);
    format!("{y:04}{mo:02}{d:02}T{h:02}:{mi:02}:{s:02}+0000")
}

pub(crate) fn sign(
    cfg: &AkamaiAuth,
    req: &HttpParts,
    env: &mut AuthEnv,
) -> Result<AuthOutput, AuthError> {
    for (field, v) in [
        ("client_token", &cfg.client_token),
        ("client_secret", &cfg.client_secret),
        ("access_token", &cfg.access_token),
    ] {
        if v.trim().is_empty() {
            return Err(invalid(field, "is required for Akamai EdgeGrid"));
        }
    }
    let u = util::parse_url(req.url)?;
    let ts = non_empty(&cfg.timestamp)
        .map(str::to_string)
        .unwrap_or_else(|| timestamp(env.now));
    let nonce = non_empty(&cfg.nonce)
        .map(str::to_string)
        .unwrap_or_else(|| util::uuid_v4(env));
    let auth_header = format!(
        "EG1-HMAC-SHA256 client_token={};access_token={};timestamp={ts};nonce={nonce};",
        cfg.client_token.trim(),
        cfg.access_token.trim()
    );

    // Configured headers, in order, when present: `name:value` with whitespace collapsed.
    let canonical_headers = cfg
        .headers_to_sign
        .as_deref()
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .filter_map(|n| {
            util::header(req, n).map(|v| {
                format!(
                    "{}:{}",
                    n.to_ascii_lowercase(),
                    v.split_whitespace().collect::<Vec<_>>().join(" ")
                )
            })
        })
        .collect::<Vec<_>>()
        .join("\t");

    let content_hash = if req.method.eq_ignore_ascii_case("POST") && !req.body.is_empty() {
        let max = cfg
            .max_body_size
            .filter(|m| *m > 0)
            .unwrap_or(DEFAULT_MAX_BODY) as usize;
        b64(&util::hash(
            HashAlg::Sha256,
            &req.body[..req.body.len().min(max)],
        ))
    } else {
        String::new()
    };

    let data = [
        req.method.to_ascii_uppercase(),
        u.scheme().to_string(),
        util::host_header(&u),
        util::request_target(&u),
        canonical_headers,
        content_hash,
        auth_header.clone(),
    ]
    .join("\t");
    let signing_key = b64(&util::hmac(
        HashAlg::Sha256,
        cfg.client_secret.trim().as_bytes(),
        ts.as_bytes(),
    ));
    let signature = b64(&util::hmac(
        HashAlg::Sha256,
        signing_key.as_bytes(),
        data.as_bytes(),
    ));
    Ok(AuthOutput::header(
        "Authorization",
        format!("{auth_header}signature={signature}"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::test_support::*;

    // Akamai edgegrid-python `akamai/edgegrid/test/testdata.json` credentials.
    fn cfg() -> AkamaiAuth {
        AkamaiAuth {
            client_token: "akab-client-token-xxx-xxxxxxxxxxxxxxxx".into(),
            client_secret: "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=".into(),
            access_token: "akab-access-token-xxx-xxxxxxxxxxxxxxxx".into(),
            timestamp: Some("20140321T19:34:21+0000".into()),
            nonce: Some("nonce-xx-xxxx-xxxx-xxxx-xxxxxxxxxxxx".into()),
            headers_to_sign: Some("X-Test1,X-Test2,X-Test3".into()),
            max_body_size: Some(2048),
            ..Default::default()
        }
    }
    const BASE: &str = "https://akaa-baseurl-xxxxxxxxxxx-xxxxxxxxxxxxx.luna.akamaiapis.net";
    const PREFIX: &str = "EG1-HMAC-SHA256 client_token=akab-client-token-xxx-xxxxxxxxxxxxxxxx;access_token=akab-access-token-xxx-xxxxxxxxxxxxxxxx;timestamp=20140321T19:34:21+0000;nonce=nonce-xx-xxxx-xxxx-xxxx-xxxxxxxxxxxx;signature=";

    fn sig(method: &str, path: &str, headers: &[(String, String)], body: &[u8]) -> String {
        let url = format!("{BASE}{path}");
        let mut r = counter();
        let mut env = AuthEnv {
            now: 0,
            random: &mut r,
        };
        let out = sign(&cfg(), &parts(method, &url, headers, body), &mut env).unwrap();
        out.headers[0].1.strip_prefix(PREFIX).unwrap().to_string()
    }

    /// Independent re-statement of the EdgeGrid algorithm (Akamai's "Authorization header" spec).
    fn reference(method: &str, path: &str, canon_headers: &str, content_hash: &str) -> String {
        let auth = &PREFIX[..PREFIX.len() - "signature=".len()];
        let host = BASE.trim_start_matches("https://");
        let data =
            format!("{method}\thttps\t{host}\t{path}\t{canon_headers}\t{content_hash}\t{auth}");
        let key = b64(&util::hmac(
            HashAlg::Sha256,
            b"xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=",
            b"20140321T19:34:21+0000",
        ));
        b64(&util::hmac(
            HashAlg::Sha256,
            key.as_bytes(),
            data.as_bytes(),
        ))
    }

    #[test]
    fn edgegrid_testcases() {
        // edgegrid-python `akamai/edgegrid/test/testcases.json` (expected signatures).
        let d = |n: usize| "d".repeat(n).into_bytes();
        type Case<'a> = (&'a str, &'a str, Vec<(String, String)>, Vec<u8>, &'a str);
        let cases: Vec<Case> = vec![
            (
                "GET",
                "/",
                vec![],
                vec![],
                "tL+y4hxyHxgWVD30X3pWnGKHcPzmrIF+LThiAOhMxYU=",
            ),
            (
                "GET",
                "/testapi/v1/t1?p1=1&p2=2",
                vec![],
                vec![],
                "hKDH1UlnQySSHjvIcZpDMbQHihTQ0XyVAKZaApabdeA=",
            ),
            (
                "POST",
                "/testapi/v1/t3",
                vec![],
                b"datadatadatadatadatadatadatadata".to_vec(),
                "hXm4iCxtpN22m4cbZb4lVLW5rhX8Ca82vCFqXzSTPe4=",
            ),
            (
                "POST",
                "/testapi/v1/t3",
                vec![],
                d(2049),
                "6Q6PiTipLae6n4GsSIDTCJ54bEbHUBp+4MUXrbQCBoY=",
            ),
            (
                "POST",
                "/testapi/v1/t3",
                vec![],
                d(2048),
                "6Q6PiTipLae6n4GsSIDTCJ54bEbHUBp+4MUXrbQCBoY=",
            ),
            (
                "POST",
                "/testapi/v1/t6",
                vec![],
                vec![],
                "1gEDxeQGD5GovIkJJGcBaKnZ+VaPtrc4qBUHixjsPCQ=",
            ),
            (
                "GET",
                "/testapi/v1/t4",
                h(&[("X-Test1", "test-simple-header")]),
                vec![],
                "8F9AybcRw+PLxnvT+H0JRkjROrrUgsxJTnRXMzqvcwY=",
            ),
            (
                "GET",
                "/testapi/v1/t4",
                h(&[("X-Test1", "\"     test-header-with-spaces     \"")]),
                vec![],
                "ucq2AbjCNtobHfCTuS38fdkl5UDdWHZhQX46fYR8CqI=",
            ),
            (
                "GET",
                "/testapi/v1/t4",
                h(&[("X-Test1", "     first-thing      second-thing")]),
                vec![],
                "WtnneL539UadAAOJwnsXvPqT4Kt6z7HMgBEwAFpt3+c=",
            ),
            (
                "GET",
                "/testapi/v1/t4",
                h(&[("X-Test2", "t2"), ("X-Test1", "t1"), ("X-Test3", "t3")]),
                vec![],
                "Wus73Nx8jOYM+kkBFF2q8D1EATRIMr0WLWwpLBgkBqY=",
            ),
            (
                "GET",
                "/testapi/v1/t5",
                h(&[
                    ("X-Test2", "t2"),
                    ("X-Test1", "t1"),
                    ("X-Test3", "t3"),
                    ("X-Extra", "this won't be included"),
                ]),
                vec![],
                "Knd/jc0A5Ghhizjayr0AUUvl2MZjBpS3FDSzvtq4Ixc=",
            ),
            (
                "GET",
                "/testapi/v1/configs/111?from=12345&limit=200000",
                vec![],
                vec![],
                "XM+hsuSs6nuy/5eDRty1IjtVCAdr8xPFRAZ/b8RXDm8=",
            ),
            (
                "GET",
                "/testapi/v1/configs/111;222;333?from=12345&limit=200000",
                vec![],
                vec![],
                "pmQF7Is2+O4r/mMojPR4yeF58BrempNNoBX5/DT0Fxs=",
            ),
        ];
        for (method, path, headers, body, expected) in cases {
            assert_eq!(
                sig(method, path, &headers, &body),
                expected,
                "{method} {path}"
            );
        }
    }

    #[test]
    fn headers_and_body_limit() {
        let headers = h(&[
            ("X-Test1", "  test-simple-header  "),
            ("X-Test2", "a   b"),
            ("X-Other", "ignored"),
        ]);
        assert_eq!(
            sig("GET", "/x", &headers, b""),
            reference("GET", "/x", "x-test1:test-simple-header\tx-test2:a b", "")
        );
        // Bodies over max_body_size are hashed up to the limit; non-POST bodies are not hashed.
        let big = vec![b'd'; 4096];
        let hash = b64(&util::hash(HashAlg::Sha256, &big[..2048]));
        assert_eq!(
            sig("POST", "/y", &[], &big),
            reference("POST", "/y", "", &hash)
        );
        assert_eq!(sig("PUT", "/y", &[], &big), reference("PUT", "/y", "", ""));
    }

    #[test]
    fn defaults_and_errors() {
        assert_eq!(timestamp(1_395_430_461), "20140321T19:34:21+0000");
        let c = AkamaiAuth {
            timestamp: None,
            nonce: None,
            ..cfg()
        };
        let mut r = counter();
        let mut env = AuthEnv {
            now: 1_395_430_461,
            random: &mut r,
        };
        let out = sign(&c, &parts("GET", "https://h/", &[], b""), &mut env).unwrap();
        assert!(out.headers[0].1.contains(
            "timestamp=20140321T19:34:21+0000;nonce=00010203-0405-4607-8809-0a0b0c0d0e0f;"
        ));
        let e = sign(
            &AkamaiAuth {
                client_secret: "".into(),
                ..cfg()
            },
            &parts("GET", "https://h/", &[], b""),
            &mut env,
        );
        assert!(matches!(e, Err(AuthError::Invalid { field, .. }) if field == "client_secret"));
    }
}
