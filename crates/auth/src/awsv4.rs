//! AWS Signature Version 4 (headers or presigned query).

use crate::util::{self, HashAlg, hex, invalid, non_empty, pct};
use crate::{AuthError, AuthOutput, HttpParts};
use ilixpatch_core::auth::{AddTo, AwsV4Auth};

const ALGORITHM: &str = "AWS4-HMAC-SHA256";
/// Headers never signed (changed by proxies/clients), as in the `aws4` library Postman uses.
const UNSIGNABLE: &[&str] = &[
    "authorization",
    "connection",
    "x-amzn-trace-id",
    "user-agent",
    "expect",
    "presigned-expires",
    "range",
    "host",
    "content-length",
];
const PRESIGN_EXPIRES: &str = "86400";

/// Canonical URI: normalised path, each segment URI-encoded twice (once for S3, not normalised).
fn canonical_uri(path: &str, s3: bool) -> String {
    let segments: Vec<&str> = path.split('/').collect();
    let mut out: Vec<String> = Vec::new();
    if s3 {
        out = segments.iter().map(|s| pct(&util::pct_decode(s))).collect();
        let p = out.join("/");
        return if p.is_empty() { "/".into() } else { p };
    }
    let trailing = path.len() > 1 && path.ends_with('/');
    for seg in segments {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            s => out.push(pct(&pct(&util::pct_decode(s)))),
        }
    }
    let mut p = format!("/{}", out.join("/"));
    if trailing && p != "/" {
        p.push('/');
    }
    p
}

fn canonical_query(pairs: &[(String, String)]) -> String {
    let mut enc: Vec<(String, String)> = pairs.iter().map(|(k, v)| (pct(k), pct(v))).collect();
    enc.sort();
    enc.iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&")
}

fn trim_value(v: &str) -> String {
    v.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn amz_date(now: u64) -> (String, String) {
    let (y, mo, d, h, mi, s) = util::utc(now);
    let date = format!("{y:04}{mo:02}{d:02}");
    (format!("{date}T{h:02}{mi:02}{s:02}Z"), date)
}

pub(crate) fn sign(cfg: &AwsV4Auth, req: &HttpParts, now: u64) -> Result<AuthOutput, AuthError> {
    if cfg.access_key.trim().is_empty() {
        return Err(invalid("access_key", "the AWS access key is required"));
    }
    if cfg.secret_key.is_empty() {
        return Err(invalid("secret_key", "the AWS secret key is required"));
    }
    let service = cfg.service.trim();
    if service.is_empty() {
        return Err(invalid(
            "service",
            "the AWS service name is required (e.g. execute-api, s3)",
        ));
    }
    let region = if cfg.region.trim().is_empty() {
        "us-east-1"
    } else {
        cfg.region.trim()
    };
    let s3 = service == "s3";
    let u = util::parse_url(req.url)?;
    let (datetime, date) = amz_date(now);
    let scope = format!("{date}/{region}/{service}/aws4_request");
    let token = non_empty(&cfg.session_token);
    let presign = cfg.add_to == AddTo::Query;

    let mut query = util::parse_form(u.query().unwrap_or_default());
    let mut headers: Vec<(String, String)> = vec![("host".into(), util::host_header(&u))];
    let mut out = AuthOutput::default();
    let payload_hash = if presign && s3 {
        "UNSIGNED-PAYLOAD".to_string()
    } else {
        hex(&util::hash(HashAlg::Sha256, req.body))
    };

    if presign {
        let mut added = vec![
            ("X-Amz-Algorithm".to_string(), ALGORITHM.to_string()),
            (
                "X-Amz-Credential".into(),
                format!("{}/{scope}", cfg.access_key.trim()),
            ),
            ("X-Amz-Date".into(), datetime.clone()),
            ("X-Amz-Expires".into(), PRESIGN_EXPIRES.into()),
            ("X-Amz-SignedHeaders".into(), "host".into()),
        ];
        if let Some(t) = token {
            added.push(("X-Amz-Security-Token".into(), t.into()));
        }
        query.extend(added.iter().cloned());
        out.query = added;
    } else {
        for (k, v) in req.headers {
            let name = k.to_ascii_lowercase();
            if UNSIGNABLE.contains(&name.as_str())
                || name == "x-amz-date"
                || name == "x-amz-security-token"
                || name == "x-amz-content-sha256"
            {
                continue;
            }
            match headers.iter_mut().find(|(n, _)| *n == name) {
                Some(existing) => existing.1 = format!("{},{}", existing.1, trim_value(v)),
                None => headers.push((name, trim_value(v))),
            }
        }
        headers.push(("x-amz-date".into(), datetime.clone()));
        out.headers.push(("X-Amz-Date".into(), datetime.clone()));
        if s3 {
            headers.push(("x-amz-content-sha256".into(), payload_hash.clone()));
            out.headers
                .push(("X-Amz-Content-Sha256".into(), payload_hash.clone()));
        }
        if let Some(t) = token {
            headers.push(("x-amz-security-token".into(), t.into()));
            out.headers.push(("X-Amz-Security-Token".into(), t.into()));
        }
    }
    headers.sort();
    let signed_headers = headers
        .iter()
        .map(|(k, _)| k.as_str())
        .collect::<Vec<_>>()
        .join(";");
    let canonical_headers: String = headers.iter().map(|(k, v)| format!("{k}:{v}\n")).collect();
    let canonical_request = format!(
        "{}\n{}\n{}\n{canonical_headers}\n{signed_headers}\n{payload_hash}",
        req.method.to_ascii_uppercase(),
        canonical_uri(u.path(), s3),
        canonical_query(&query),
    );
    let string_to_sign = format!(
        "{ALGORITHM}\n{datetime}\n{scope}\n{}",
        hex(&util::hash(HashAlg::Sha256, canonical_request.as_bytes()))
    );
    let mut key = util::hmac(
        HashAlg::Sha256,
        format!("AWS4{}", cfg.secret_key).as_bytes(),
        date.as_bytes(),
    );
    for part in [region, service, "aws4_request"] {
        key = util::hmac(HashAlg::Sha256, &key, part.as_bytes());
    }
    let signature = hex(&util::hmac(
        HashAlg::Sha256,
        &key,
        string_to_sign.as_bytes(),
    ));

    if presign {
        out.query.push(("X-Amz-Signature".into(), signature));
    } else {
        out.headers.insert(
            0,
            (
                "Authorization".into(),
                format!("{ALGORITHM} Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={signature}", cfg.access_key.trim()),
            ),
        );
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::test_support::*;

    // AWS Signature Version 4 test suite (aws-sig-v4-test-suite): 20150830T123600Z,
    // us-east-1, service "service", AKIDEXAMPLE / wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY.
    const NOW: u64 = 1_440_938_160;

    fn cfg() -> AwsV4Auth {
        AwsV4Auth {
            access_key: "AKIDEXAMPLE".into(),
            secret_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".into(),
            region: "us-east-1".into(),
            service: "service".into(),
            ..Default::default()
        }
    }

    fn auth(out: &AuthOutput) -> &str {
        &out.headers
            .iter()
            .find(|(k, _)| k == "Authorization")
            .unwrap()
            .1
    }

    #[test]
    fn get_vanilla() {
        let out = sign(
            &cfg(),
            &parts("GET", "https://example.amazonaws.com/", &[], b""),
            NOW,
        )
        .unwrap();
        assert_eq!(
            auth(&out),
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/service/aws4_request, SignedHeaders=host;x-amz-date, Signature=5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31"
        );
        assert!(
            out.headers
                .contains(&("X-Amz-Date".into(), "20150830T123600Z".into()))
        );
    }

    #[test]
    fn get_vanilla_query_order_key_case() {
        let out = sign(
            &cfg(),
            &parts(
                "GET",
                "https://example.amazonaws.com/?Param2=value2&Param1=value1",
                &[],
                b"",
            ),
            NOW,
        )
        .unwrap();
        assert!(
            auth(&out).ends_with(
                "Signature=b97d918cfa904a5beff61c982a1b6f458b799221646efd99d3219ec94cdf2500"
            ),
            "{}",
            auth(&out)
        );
    }

    #[test]
    fn post_x_www_form_urlencoded() {
        let headers = h(&[("Content-Type", "application/x-www-form-urlencoded")]);
        let out = sign(
            &cfg(),
            &parts(
                "POST",
                "https://example.amazonaws.com/",
                &headers,
                b"Param1=value1",
            ),
            NOW,
        )
        .unwrap();
        assert!(auth(&out).contains("SignedHeaders=content-type;host;x-amz-date"));
        assert!(
            auth(&out).ends_with(
                "Signature=ff11897932ad3f4e8b18135d722051e5ac45fc38421b1da7b9d196a0fe09473a"
            ),
            "{}",
            auth(&out)
        );
    }

    #[test]
    fn uri_normalisation() {
        assert_eq!(canonical_uri("/example//./a/../b/", false), "/example/b/");
        assert_eq!(canonical_uri("/a%20b", false), "/a%2520b");
        assert_eq!(canonical_uri("/a%20b//c", true), "/a%20b//c");
        assert_eq!(canonical_uri("", false), "/");
    }

    #[test]
    fn s3_token_presign_and_errors() {
        let c = AwsV4Auth {
            service: "s3".into(),
            session_token: Some("TOKEN".into()),
            ..cfg()
        };
        let out = sign(
            &c,
            &parts("PUT", "https://bucket.s3.amazonaws.com/k", &[], b""),
            NOW,
        )
        .unwrap();
        assert!(
            out.headers
                .iter()
                .any(|(k, v)| k == "X-Amz-Content-Sha256" && v.starts_with("e3b0c442"))
        );
        assert!(
            out.headers
                .contains(&("X-Amz-Security-Token".into(), "TOKEN".into()))
        );
        assert!(
            auth(&out).contains(
                "SignedHeaders=host;x-amz-content-sha256;x-amz-date;x-amz-security-token"
            )
        );

        let p = AwsV4Auth {
            add_to: AddTo::Query,
            ..cfg()
        };
        let out = sign(
            &p,
            &parts("GET", "https://example.amazonaws.com/?a=1", &[], b""),
            NOW,
        )
        .unwrap();
        assert!(out.headers.is_empty());
        let names: Vec<&str> = out.query.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            names,
            [
                "X-Amz-Algorithm",
                "X-Amz-Credential",
                "X-Amz-Date",
                "X-Amz-Expires",
                "X-Amz-SignedHeaders",
                "X-Amz-Signature"
            ]
        );

        let field = |c: AwsV4Auth| match sign(&c, &parts("GET", "https://e.com/", &[], b""), NOW) {
            Err(AuthError::Invalid { field, .. }) => field,
            other => panic!("{other:?}"),
        };
        assert_eq!(
            field(AwsV4Auth {
                access_key: "".into(),
                ..cfg()
            }),
            "access_key"
        );
        assert_eq!(
            field(AwsV4Auth {
                secret_key: "".into(),
                ..cfg()
            }),
            "secret_key"
        );
        assert_eq!(
            field(AwsV4Auth {
                service: "".into(),
                ..cfg()
            }),
            "service"
        );
    }
}
