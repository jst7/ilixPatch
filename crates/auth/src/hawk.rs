//! Hawk HTTP authentication (hawk.1 header scheme).

use crate::util::{self, HashAlg, b64, invalid, non_empty, quote};
use crate::{AuthEnv, AuthError, AuthOutput, HttpParts};
use ilixpatch_core::auth::HawkAuth;

fn algorithm(cfg: &HawkAuth) -> Result<HashAlg, AuthError> {
    match non_empty(&cfg.algorithm)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        None | Some("sha256") | Some("sha-256") => Ok(HashAlg::Sha256),
        Some("sha1") | Some("sha-1") => Ok(HashAlg::Sha1),
        Some(other) => Err(invalid(
            "algorithm",
            format!("unsupported Hawk algorithm \"{other}\" (sha256 or sha1)"),
        )),
    }
}

/// `hawk.1.payload\n<content-type>\n<payload>\n`, base64 hash.
pub(crate) fn payload_hash(alg: HashAlg, content_type: &str, payload: &[u8]) -> String {
    let mut data = format!("hawk.1.payload\n{}\n", util::media_type(content_type)).into_bytes();
    data.extend_from_slice(payload);
    data.push(b'\n');
    b64(&util::hash(alg, &data))
}

pub(crate) struct Artifacts<'a> {
    pub ts: &'a str,
    pub nonce: &'a str,
    pub method: &'a str,
    pub resource: &'a str,
    pub host: &'a str,
    pub port: u16,
    pub hash: Option<&'a str>,
    pub ext: Option<&'a str>,
    pub app: Option<&'a str>,
    pub dlg: Option<&'a str>,
}

pub(crate) fn mac(alg: HashAlg, key: &str, a: &Artifacts) -> String {
    let mut s = format!(
        "hawk.1.header\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n",
        a.ts,
        a.nonce,
        a.method.to_ascii_uppercase(),
        a.resource,
        a.host.to_ascii_lowercase(),
        a.port,
        a.hash.unwrap_or_default(),
        a.ext
            .unwrap_or_default()
            .replace('\\', "\\\\")
            .replace('\n', "\\n"),
    );
    if let Some(app) = a.app {
        s.push_str(&format!("{app}\n{}\n", a.dlg.unwrap_or_default()));
    }
    b64(&util::hmac(alg, key.as_bytes(), s.as_bytes()))
}

pub(crate) fn sign(
    cfg: &HawkAuth,
    req: &HttpParts,
    env: &mut AuthEnv,
) -> Result<AuthOutput, AuthError> {
    if cfg.id.trim().is_empty() {
        return Err(invalid("id", "the Hawk Auth ID is required"));
    }
    if cfg.key.is_empty() {
        return Err(invalid("key", "the Hawk Auth Key is required"));
    }
    let alg = algorithm(cfg)?;
    let u = util::parse_url(req.url)?;
    let ts = non_empty(&cfg.timestamp)
        .map(str::to_string)
        .unwrap_or_else(|| env.now.to_string());
    let nonce = non_empty(&cfg.nonce)
        .map(str::to_string)
        .unwrap_or_else(|| util::random_alnum(env, 6));
    let hash = cfg.include_payload_hash.then(|| {
        payload_hash(
            alg,
            util::header(req, "content-type").unwrap_or_default(),
            req.body,
        )
    });
    let ext = non_empty(&cfg.ext);
    let app = non_empty(&cfg.app);
    let dlg = app.and(non_empty(&cfg.dlg));
    let resource = util::request_target(&u);
    let host = u.host_str().unwrap_or_default().to_string();
    let port = u.port_or_known_default().unwrap_or(80);
    let a = Artifacts {
        ts: &ts,
        nonce: &nonce,
        method: req.method,
        resource: &resource,
        host: &host,
        port,
        hash: hash.as_deref(),
        ext,
        app,
        dlg,
    };
    let m = mac(alg, &cfg.key, &a);
    let mut parts = vec![
        format!("id={}", quote(cfg.id.trim())),
        format!("ts={}", quote(&ts)),
        format!("nonce={}", quote(&nonce)),
    ];
    if let Some(h) = &hash {
        parts.push(format!("hash={}", quote(h)));
    }
    if let Some(e) = ext {
        parts.push(format!("ext={}", quote(e)));
    }
    parts.push(format!("mac={}", quote(&m)));
    if let Some(app) = app {
        parts.push(format!("app={}", quote(app)));
        if let Some(d) = dlg {
            parts.push(format!("dlg={}", quote(d)));
        }
    }
    Ok(AuthOutput::header(
        "Authorization",
        format!("Hawk {}", parts.join(", ")),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::test_support::*;

    fn readme_cfg() -> HawkAuth {
        // Hawk README "Usage example" credentials.
        HawkAuth {
            id: "dh37fgj492je".into(),
            key: "werxhqb98rpaxn39848xrunpaw3489ruxnpa98w4rxn".into(),
            algorithm: Some("sha256".into()),
            nonce: Some("j4h3g2".into()),
            ext: Some("some-app-ext-data".into()),
            timestamp: Some("1353832234".into()),
            ..Default::default()
        }
    }

    fn run(
        cfg: &HawkAuth,
        method: &str,
        url: &str,
        headers: &[(String, String)],
        body: &[u8],
    ) -> Result<AuthOutput, AuthError> {
        let mut r = counter();
        let mut env = AuthEnv {
            now: 0,
            random: &mut r,
        };
        sign(cfg, &parts(method, url, headers, body), &mut env)
    }

    #[test]
    fn readme_example() {
        let out = run(
            &readme_cfg(),
            "GET",
            "http://example.com:8000/resource/1?b=1&a=2",
            &[],
            b"",
        )
        .unwrap();
        assert_eq!(
            out.headers[0].1,
            r#"Hawk id="dh37fgj492je", ts="1353832234", nonce="j4h3g2", ext="some-app-ext-data", mac="6R4rV5iE+NPoym+WwjeHzjAGXUtLNIxmo1vpMofpLAE=""#
        );
    }

    #[test]
    fn readme_payload_example() {
        // Hawk README "Payload Validation": POST "Thank you for flying Hawk" as text/plain.
        assert_eq!(
            payload_hash(HashAlg::Sha256, "text/plain", b"Thank you for flying Hawk"),
            "Yi9LfIIFRtBEPt74PVmbTF/xVAwPn7ub15ePICfgnuY="
        );
        let cfg = HawkAuth {
            include_payload_hash: true,
            ..readme_cfg()
        };
        let headers = h(&[("Content-Type", "text/plain")]);
        let out = run(
            &cfg,
            "POST",
            "http://example.com:8000/resource/1?b=1&a=2",
            &headers,
            b"Thank you for flying Hawk",
        )
        .unwrap();
        let v = &out.headers[0].1;
        assert!(
            v.contains(r#"hash="Yi9LfIIFRtBEPt74PVmbTF/xVAwPn7ub15ePICfgnuY=""#),
            "{v}"
        );
        assert!(
            v.contains(r#"mac="aSe1DERmZuRl3pI36/9BdZmnErTw3sNzOOAUlfeKjVw=""#),
            "{v}"
        );
    }

    #[test]
    fn app_dlg_sha1_and_errors() {
        let cfg = HawkAuth {
            algorithm: Some("sha1".into()),
            app: Some("ap".into()),
            dlg: Some("dl".into()),
            ext: None,
            ..readme_cfg()
        };
        let v = run(&cfg, "GET", "https://example.com/x", &[], b"")
            .unwrap()
            .headers[0]
            .1
            .clone();
        let a = Artifacts {
            ts: "1353832234",
            nonce: "j4h3g2",
            method: "GET",
            resource: "/x",
            host: "example.com",
            port: 443,
            hash: None,
            ext: None,
            app: Some("ap"),
            dlg: Some("dl"),
        };
        assert!(
            v.ends_with(&format!(
                "mac=\"{}\", app=\"ap\", dlg=\"dl\"",
                mac(HashAlg::Sha1, &cfg.key, &a)
            )),
            "{v}"
        );
        assert!(
            matches!(run(&HawkAuth { id: "".into(), ..readme_cfg() }, "GET", "https://e.com/", &[], b""), Err(AuthError::Invalid { field, .. }) if field == "id")
        );
        assert!(
            matches!(run(&HawkAuth { key: "".into(), ..readme_cfg() }, "GET", "https://e.com/", &[], b""), Err(AuthError::Invalid { field, .. }) if field == "key")
        );
        assert!(
            matches!(run(&HawkAuth { algorithm: Some("md5".into()), ..readme_cfg() }, "GET", "https://e.com/", &[], b""), Err(AuthError::Invalid { field, .. }) if field == "algorithm")
        );
    }
}
