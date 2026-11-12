//! Applying a request's authorization (D39) right before sending.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use ilixpatch_auth::{AuthOutput, HttpParts, oauth2, sign};
use ilixpatch_core::auth::{Auth, OAuth2Auth};
use ilixpatch_core::{Body, Request};
use ilixpatch_engine::{
    ChallengeAuth, RequestError, RequestErrorKind, body_bytes, full_url, with_auth_env,
};

/// A boxed, `Send` future (no `futures` dependency needed).
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Supplies OAuth 2.0 access tokens (token store, refresh, automatic fetch for
/// client_credentials/password). Implemented by the app and the CLI.
/// `cfg` is already variable-resolved. Errors are shown to the user as is.
pub trait OAuth2Provider: Send + Sync {
    fn access_token<'a>(&'a self, cfg: &'a OAuth2Auth) -> BoxFuture<'a, Result<String, String>>;
}

/// A shareable provider for run/load configurations.
#[derive(Clone)]
pub struct OAuth2Handle(pub Arc<dyn OAuth2Provider>);

impl std::fmt::Debug for OAuth2Handle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OAuth2Handle")
    }
}

/// Display name of an auth type (as in Postman), for messages.
pub fn auth_type_name(auth: &Auth) -> &'static str {
    match auth {
        Auth::Inherit => "Inherit",
        Auth::NoAuth => "No Auth",
        Auth::ApiKey(_) => "API Key",
        Auth::Bearer(_) => "Bearer Token",
        Auth::Jwt(_) => "JWT Bearer",
        Auth::Basic(_) => "Basic Auth",
        Auth::Digest(_) => "Digest Auth",
        Auth::Oauth1(_) => "OAuth 1.0",
        Auth::Oauth2(_) => "OAuth 2.0",
        Auth::Hawk(_) => "Hawk Authentication",
        Auth::Awsv4(_) => "AWS Signature",
        Auth::Ntlm(_) => "NTLM Authentication",
        Auth::Akamai(_) => "Akamai EdgeGrid",
        Auth::Asap(_) => "ASAP (Atlassian)",
    }
}

fn has_header(req: &Request, name: &str) -> bool {
    req.headers.keys().any(|k| k.eq_ignore_ascii_case(name))
}

fn set_header(req: &mut Request, name: &str, value: String) {
    req.headers.retain(|k, _| !k.eq_ignore_ascii_case(name));
    req.headers.insert(name.to_string(), value);
}

fn warn_override(req: &Request, auth: &Auth, logs: &mut Vec<String>) {
    if has_header(req, "authorization") {
        logs.push(format!(
            "[warn] {} replaces the Authorization header set in Headers",
            auth_type_name(auth)
        ));
    }
}

/// Merge signer output into the request: headers replace case-insensitively,
/// query parameters are appended to the URL, the body is replaced.
pub fn merge_output(
    req: &mut Request,
    out: AuthOutput,
    auth: &Auth,
    logs: &mut Vec<String>,
) -> Result<(), Box<RequestError>> {
    if out
        .headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("authorization"))
    {
        warn_override(req, auth, logs);
    }
    for (k, v) in out.headers {
        set_header(req, &k, v);
    }
    if !out.query.is_empty() {
        let url = full_url(req).map_err(|e| invalid(auth, &req.url, &e.to_string()))?;
        let mut url = url
            .parse::<ilixpatch_engine::Url>()
            .map_err(|e| invalid(auth, &req.url, &e.to_string()))?;
        url.query_pairs_mut()
            .extend_pairs(out.query.iter().map(|(k, v)| (k.as_str(), v.as_str())));
        req.url = url.to_string();
        req.query.clear();
    }
    if let Some((bytes, content_type)) = out.body {
        req.body = Some(Body::Text {
            content: String::from_utf8_lossy(&bytes).into_owned(),
        });
        set_header(req, "Content-Type", content_type);
    }
    Ok(())
}

fn invalid(auth: &Auth, url: &str, msg: &str) -> Box<RequestError> {
    let message = format!("{}: {msg}", auth_type_name(auth));
    Box::new(RequestError::new(RequestErrorKind::Other, message.clone(), message).with_url(url))
}

/// Apply `req.auth` (already resolved and inherited). Single-shot types are
/// signed here; Digest/NTLM are returned as a [`ChallengeAuth`] for the
/// engine. `Inherit` is treated as no auth.
pub async fn apply_auth(
    mut req: Request,
    oauth2_provider: Option<&dyn OAuth2Provider>,
    logs: &mut Vec<String>,
) -> Result<(Request, Option<ChallengeAuth>), Box<RequestError>> {
    let auth = req.auth.clone();
    match &auth {
        Auth::Inherit | Auth::NoAuth => Ok((req, None)),
        Auth::Digest(cfg) => {
            warn_override(&req, &auth, logs);
            Ok((req, Some(ChallengeAuth::Digest(cfg.clone()))))
        }
        Auth::Ntlm(cfg) => {
            warn_override(&req, &auth, logs);
            Ok((req, Some(ChallengeAuth::Ntlm(cfg.clone()))))
        }
        Auth::Oauth2(cfg) => {
            let token = match cfg.access_token.as_deref().map(str::trim) {
                Some(t) if !t.is_empty() => t.to_string(),
                _ => match oauth2_provider {
                    Some(p) => p
                        .access_token(cfg)
                        .await
                        .map_err(|e| invalid(&auth, &req.url, &e))?,
                    None => {
                        return Err(invalid(
                            &auth,
                            &req.url,
                            "OAuth 2.0 needs a token: get one in the app (Authorization tab) or set access_token",
                        ));
                    }
                },
            };
            let out = oauth2::apply_token(cfg, &token);
            merge_output(&mut req, out, &auth, logs)?;
            Ok((req, None))
        }
        _ => {
            let url = full_url(&req).map_err(|e| invalid(&auth, &req.url, &e.to_string()))?;
            let body = body_bytes(&req);
            let mut headers: Vec<(String, String)> = req
                .headers
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            // The engine adds this one for JSON bodies; signers must see it.
            if matches!(req.body, Some(Body::Json { .. })) && !has_header(&req, "content-type") {
                headers.push(("Content-Type".into(), "application/json".into()));
            }
            let parts = HttpParts {
                method: req.method.as_str(),
                url: &url,
                headers: &headers,
                body: &body,
            };
            let out = with_auth_env(|env| sign(&auth, &parts, env))
                .map_err(|e| invalid(&auth, &req.url, &e.to_string()))?;
            merge_output(&mut req, out, &auth, logs)?;
            Ok((req, None))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilixpatch_core::auth::BearerAuth;

    fn req(url: &str) -> Request {
        toml::from_str(&format!("name = \"x\"\nurl = \"{url}\"\n")).unwrap()
    }

    #[test]
    fn merge_headers_query_body() {
        let mut r = req("https://h/p?a=1");
        r.query.insert("b".into(), "2".into());
        r.headers.insert("authorization".into(), "manual".into());
        let auth = Auth::Bearer(BearerAuth { token: "t".into() });
        let mut logs = Vec::new();
        merge_output(
            &mut r,
            AuthOutput {
                headers: vec![("Authorization".into(), "Bearer t".into())],
                query: vec![("key".into(), "a b".into())],
                body: Some((b"x=1".to_vec(), "application/x-www-form-urlencoded".into())),
            },
            &auth,
            &mut logs,
        )
        .unwrap();
        assert_eq!(r.headers.len(), 2);
        assert_eq!(r.headers["Authorization"], "Bearer t");
        assert_eq!(
            r.headers["Content-Type"],
            "application/x-www-form-urlencoded"
        );
        assert_eq!(r.url, "https://h/p?a=1&b=2&key=a+b");
        assert!(r.query.is_empty());
        assert!(matches!(&r.body, Some(Body::Text { content }) if content == "x=1"));
        assert_eq!(logs.len(), 1);
        assert!(logs[0].starts_with("[warn] Bearer Token replaces"));
    }

    #[tokio::test]
    async fn oauth2_without_token_or_provider() {
        let mut r = req("https://h/");
        r.auth = Auth::Oauth2(OAuth2Auth {
            token_name: "t".into(),
            ..Default::default()
        });
        let err = apply_auth(r.clone(), None, &mut Vec::new())
            .await
            .unwrap_err();
        assert!(
            err.message
                .starts_with("OAuth 2.0: OAuth 2.0 needs a token"),
            "{}",
            err.message
        );
        assert_eq!(err.kind, RequestErrorKind::Other);

        struct Fixed;
        impl OAuth2Provider for Fixed {
            fn access_token<'a>(
                &'a self,
                cfg: &'a OAuth2Auth,
            ) -> BoxFuture<'a, Result<String, String>> {
                Box::pin(async move { Err(format!("no token named {}", cfg.token_name)) })
            }
        }
        let err = apply_auth(r, Some(&Fixed), &mut Vec::new())
            .await
            .unwrap_err();
        assert_eq!(err.message, "OAuth 2.0: no token named t");
    }

    #[tokio::test]
    async fn inherit_is_no_auth_and_challenges_are_deferred() {
        let r = req("https://h/");
        let (out, ch) = apply_auth(r.clone(), None, &mut Vec::new()).await.unwrap();
        assert_eq!(out, r);
        assert!(ch.is_none());
        let mut d = r.clone();
        d.auth = Auth::Digest(Default::default());
        let (_, ch) = apply_auth(d, None, &mut Vec::new()).await.unwrap();
        assert!(matches!(ch, Some(ChallengeAuth::Digest(_))));
    }
}
