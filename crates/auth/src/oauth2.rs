//! OAuth 2.0 (RFC 6749) helpers + PKCE (RFC 7636). No network: the caller sends the requests.

use crate::util::{self, b64, b64url, invalid, non_empty};
use crate::{AuthEnv, AuthError, AuthOutput};
use ilixpatch_core::auth::{AddTo, ClientAuthentication, OAuth2Auth, OAuth2Grant};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// A token obtained from an authorization server.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Token {
    pub access_token: String,
    /// Usually "Bearer".
    pub token_type: Option<String>,
    pub refresh_token: Option<String>,
    pub id_token: Option<String>,
    pub scope: Option<String>,
    /// Unix seconds, from `expires_in` relative to `AuthEnv.now`.
    pub expires_at: Option<u64>,
    /// The full JSON response, for display.
    pub raw: serde_json::Value,
}

/// PKCE pair (verifier, challenge) for the configured method (S256 default).
/// A configured `code_verifier` is used as is; otherwise 32 random bytes → 43 base64url chars.
pub fn pkce(cfg: &OAuth2Auth, env: &mut AuthEnv) -> (String, String) {
    let verifier = match non_empty(&cfg.code_verifier) {
        Some(v) => v.to_string(),
        None => b64url(&util::random_bytes(env, 32)),
    };
    let challenge = pkce_challenge(cfg, &verifier);
    (verifier, challenge)
}

fn pkce_method(cfg: &OAuth2Auth) -> &'static str {
    match non_empty(&cfg.code_challenge_method) {
        Some(m) if m.eq_ignore_ascii_case("plain") => "plain",
        _ => "S256",
    }
}

fn pkce_challenge(cfg: &OAuth2Auth, verifier: &str) -> String {
    match pkce_method(cfg) {
        "plain" => verifier.to_string(),
        _ => b64url(&util::hash(util::HashAlg::Sha256, verifier.as_bytes())),
    }
}

fn push_opt(params: &mut Vec<(String, String)>, name: &str, value: &Option<String>) {
    if let Some(v) = non_empty(value) {
        params.push((name.into(), v.into()));
    }
}

fn push_extra(params: &mut Vec<(String, String)>, cfg: &OAuth2Auth) {
    for (k, v) in &cfg.extra_params {
        if !k.trim().is_empty() {
            params.push((k.clone(), v.clone()));
        }
    }
}

/// Browser URL for authorization_code / authorization_code_pkce / implicit.
pub fn authorize_url(
    cfg: &OAuth2Auth,
    redirect_uri: &str,
    state: &str,
    code_challenge: Option<&str>,
) -> Result<String, AuthError> {
    let response_type = match cfg.grant_type {
        OAuth2Grant::Implicit => "token",
        OAuth2Grant::AuthorizationCode | OAuth2Grant::AuthorizationCodePkce => "code",
        g => {
            return Err(AuthError::Unsupported(format!(
                "the {g:?} grant has no browser authorization step"
            )));
        }
    };
    let auth_url = non_empty(&cfg.auth_url)
        .ok_or_else(|| invalid("auth_url", "the authorization URL is required"))?;
    let mut url = url::Url::parse(auth_url)
        .map_err(|e| invalid("auth_url", format!("not a valid URL ({e})")))?;
    if cfg.client_id.trim().is_empty() {
        return Err(invalid("client_id", "is required"));
    }
    let mut params: Vec<(String, String)> = vec![
        ("response_type".into(), response_type.into()),
        ("client_id".into(), cfg.client_id.trim().into()),
    ];
    if !redirect_uri.is_empty() {
        params.push(("redirect_uri".into(), redirect_uri.into()));
    }
    push_opt(&mut params, "scope", &cfg.scope);
    if !state.is_empty() {
        params.push(("state".into(), state.into()));
    }
    if let Some(c) = code_challenge.filter(|c| !c.is_empty()) {
        params.push(("code_challenge".into(), c.into()));
        params.push(("code_challenge_method".into(), pkce_method(cfg).into()));
    }
    push_opt(&mut params, "audience", &cfg.audience);
    push_opt(&mut params, "resource", &cfg.resource);
    push_extra(&mut params, cfg);
    let added = util::encode_pairs(&params);
    let query = match url.query().filter(|q| !q.is_empty()) {
        Some(q) => format!("{q}&{added}"),
        None => added,
    };
    url.set_query(Some(&query));
    Ok(url.to_string())
}

/// What to send to the token endpoint.
#[derive(Debug, Clone, PartialEq)]
pub enum Grant<'a> {
    /// authorization_code (+ PKCE verifier when used).
    Code {
        code: &'a str,
        redirect_uri: &'a str,
        code_verifier: Option<&'a str>,
    },
    Password,
    ClientCredentials,
    Refresh {
        refresh_token: &'a str,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct TokenRequest {
    pub url: String,
    pub headers: Vec<(String, String)>,
    /// application/x-www-form-urlencoded body.
    pub form: Vec<(String, String)>,
}

/// Token endpoint request. Headers: `Accept: application/json` and, with client authentication
/// "basic" and a client secret, `Authorization: Basic base64(client_id:client_secret)` (like
/// Postman, not form-encoded). Without a secret, `client_id` goes in the body.
pub fn token_request(cfg: &OAuth2Auth, grant: Grant) -> Result<TokenRequest, AuthError> {
    let url = non_empty(&cfg.access_token_url)
        .ok_or_else(|| invalid("access_token_url", "the access token URL is required"))?;
    url::Url::parse(url)
        .map_err(|e| invalid("access_token_url", format!("not a valid URL ({e})")))?;
    let mut form: Vec<(String, String)> = Vec::new();
    let is_refresh = matches!(grant, Grant::Refresh { .. });
    match grant {
        Grant::Code {
            code,
            redirect_uri,
            code_verifier,
        } => {
            if code.is_empty() {
                return Err(invalid("code", "the authorization code is empty"));
            }
            form.push(("grant_type".into(), "authorization_code".into()));
            form.push(("code".into(), code.into()));
            if !redirect_uri.is_empty() {
                form.push(("redirect_uri".into(), redirect_uri.into()));
            }
            if let Some(v) = code_verifier.filter(|v| !v.is_empty()) {
                form.push(("code_verifier".into(), v.into()));
            }
        }
        Grant::Password => {
            let user = non_empty(&cfg.username)
                .ok_or_else(|| invalid("username", "is required for the password grant"))?;
            form.push(("grant_type".into(), "password".into()));
            form.push(("username".into(), user.into()));
            form.push(("password".into(), cfg.password.clone().unwrap_or_default()));
            push_opt(&mut form, "scope", &cfg.scope);
        }
        Grant::ClientCredentials => {
            form.push(("grant_type".into(), "client_credentials".into()));
            push_opt(&mut form, "scope", &cfg.scope);
        }
        Grant::Refresh { refresh_token } => {
            if refresh_token.is_empty() {
                return Err(invalid("refresh_token", "no refresh token available"));
            }
            form.push(("grant_type".into(), "refresh_token".into()));
            form.push(("refresh_token".into(), refresh_token.into()));
        }
    }
    if !is_refresh {
        push_opt(&mut form, "audience", &cfg.audience);
        push_opt(&mut form, "resource", &cfg.resource);
    }
    push_extra(&mut form, cfg);

    let mut headers = vec![("Accept".to_string(), "application/json".to_string())];
    let client_id = cfg.client_id.trim();
    let secret = cfg.client_secret.as_deref().unwrap_or_default();
    match cfg.client_authentication {
        ClientAuthentication::Basic if !secret.is_empty() => {
            headers.push((
                "Authorization".into(),
                format!("Basic {}", b64(format!("{client_id}:{secret}").as_bytes())),
            ));
        }
        _ => {
            if !client_id.is_empty() {
                form.push(("client_id".into(), client_id.into()));
            }
            if !secret.is_empty() {
                form.push(("client_secret".into(), secret.into()));
            }
        }
    }
    Ok(TokenRequest {
        url: url.to_string(),
        headers,
        form,
    })
}

fn as_string(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn error_message(obj: &Map<String, Value>) -> Option<String> {
    let err = as_string(obj.get("error"))?;
    Some(match as_string(obj.get("error_description")) {
        Some(d) => format!("{err}: {d}"),
        None => err,
    })
}

fn token_from(obj: &Map<String, Value>, now: u64, field: &str) -> Result<Token, AuthError> {
    let access_token = as_string(obj.get("access_token"))
        .ok_or_else(|| invalid(field, "the response has no access_token"))?;
    let expires_at = obj.get("expires_in").and_then(|v| match v {
        Value::Number(n) => n.as_u64().or_else(|| n.as_f64().map(|f| f as u64)),
        Value::String(s) => s.trim().parse::<u64>().ok(),
        _ => None,
    });
    Ok(Token {
        access_token,
        token_type: as_string(obj.get("token_type")),
        refresh_token: as_string(obj.get("refresh_token")),
        id_token: as_string(obj.get("id_token")),
        scope: as_string(obj.get("scope")),
        expires_at: expires_at.map(|e| now + e),
        raw: Value::Object(obj.clone()),
    })
}

fn form_object(pairs: Vec<(String, String)>) -> Map<String, Value> {
    pairs
        .into_iter()
        .map(|(k, v)| (k, Value::String(v)))
        .collect()
}

/// Parse a token endpoint response (JSON, or form-encoded like GitHub's legacy one).
/// Error responses (`error`, `error_description`) → AuthError::Invalid with a readable message.
pub fn parse_token_response(
    status: u16,
    content_type: Option<&str>,
    body: &[u8],
    now: u64,
) -> Result<Token, AuthError> {
    const FIELD: &str = "token_response";
    let text = String::from_utf8_lossy(body);
    let trimmed = text.trim();
    let ct = content_type.map(util::media_type).unwrap_or_default();
    let obj: Option<Map<String, Value>> = if ct.contains("json") || trimmed.starts_with('{') {
        match serde_json::from_str::<Value>(trimmed) {
            Ok(Value::Object(m)) => Some(m),
            _ => None,
        }
    } else if ct == "application/x-www-form-urlencoded"
        || (trimmed.contains('=') && !trimmed.contains(char::is_whitespace))
    {
        Some(form_object(util::parse_form(trimmed)))
    } else {
        None
    };
    if let Some(msg) = obj.as_ref().and_then(error_message) {
        return Err(invalid(FIELD, msg));
    }
    if !(200..300).contains(&status) {
        let snippet: String = trimmed.chars().take(300).collect();
        let msg = if snippet.is_empty() {
            format!("the token endpoint returned HTTP {status} with an empty body")
        } else {
            format!("the token endpoint returned HTTP {status}: {snippet}")
        };
        return Err(invalid(FIELD, msg));
    }
    match obj {
        Some(o) => token_from(&o, now, FIELD),
        None => Err(invalid(
            FIELD,
            "the token endpoint response is neither JSON nor form-encoded",
        )),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RedirectResult {
    Code(String),
    Token(Token),
}

/// Parse the redirect URL received on the callback (code+state in the query, or the implicit
/// token in the fragment). Checks `state`.
pub fn parse_redirect(
    cfg: &OAuth2Auth,
    redirect_url: &str,
    expected_state: &str,
    now: u64,
) -> Result<RedirectResult, AuthError> {
    let url = url::Url::parse(redirect_url.trim())
        .map_err(|e| invalid("callback_url", format!("not a valid redirect URL ({e})")))?;
    let mut params = util::parse_form(url.query().unwrap_or_default());
    params.extend(util::parse_form(url.fragment().unwrap_or_default()));
    let obj = form_object(params);
    if let Some(msg) = error_message(&obj) {
        return Err(invalid(
            "authorization",
            format!("the authorization server returned an error: {msg}"),
        ));
    }
    if !expected_state.is_empty() {
        let got = as_string(obj.get("state")).unwrap_or_default();
        if got != expected_state {
            return Err(invalid(
                "state",
                "the state in the redirect does not match the one sent (possible CSRF); try again",
            ));
        }
    }
    let code = as_string(obj.get("code"));
    let has_token = obj.contains_key("access_token");
    match (cfg.grant_type, code, has_token) {
        (OAuth2Grant::Implicit, _, true) | (_, None, true) => Ok(RedirectResult::Token(
            token_from(&obj, now, "callback_url")?,
        )),
        (_, Some(c), _) => Ok(RedirectResult::Code(c)),
        _ => Err(invalid(
            "callback_url",
            "the redirect URL has no code or access_token",
        )),
    }
}

/// Headers/query to use an access token according to `add_to` / `header_prefix`.
pub fn apply_token(cfg: &OAuth2Auth, access_token: &str) -> AuthOutput {
    match cfg.add_to {
        AddTo::Query => AuthOutput {
            query: vec![("access_token".into(), access_token.into())],
            ..Default::default()
        },
        _ => {
            let prefix = cfg
                .header_prefix
                .as_deref()
                .map(str::trim)
                .unwrap_or("Bearer");
            let value = if prefix.is_empty() {
                access_token.to_string()
            } else {
                format!("{prefix} {access_token}")
            };
            AuthOutput {
                headers: vec![("Authorization".into(), value)],
                ..Default::default()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::test_support::*;

    fn cfg() -> OAuth2Auth {
        OAuth2Auth {
            grant_type: OAuth2Grant::AuthorizationCodePkce,
            client_id: "my client".into(),
            client_secret: Some("s3cret".into()),
            auth_url: Some("https://auth.example.com/authorize?tenant=a".into()),
            access_token_url: Some("https://auth.example.com/token".into()),
            scope: Some("read write".into()),
            audience: Some("api".into()),
            ..Default::default()
        }
    }

    #[test]
    fn pkce_rfc7636_appendix_b() {
        let c = OAuth2Auth {
            code_verifier: Some("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk".into()),
            ..Default::default()
        };
        let mut r = counter();
        let mut env = AuthEnv {
            now: 0,
            random: &mut r,
        };
        assert_eq!(
            pkce(&c, &mut env).1,
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let plain = OAuth2Auth {
            code_challenge_method: Some("plain".into()),
            ..c
        };
        let (v, ch) = pkce(&plain, &mut env);
        assert_eq!(v, ch);
        let (v, _) = pkce(&OAuth2Auth::default(), &mut env);
        assert!((43..=128).contains(&v.len()));
    }

    #[test]
    fn authorize() {
        let u = authorize_url(&cfg(), "http://127.0.0.1:5555/cb", "xyz", Some("CH")).unwrap();
        assert_eq!(
            u,
            "https://auth.example.com/authorize?tenant=a&response_type=code&client_id=my%20client&redirect_uri=http%3A%2F%2F127.0.0.1%3A5555%2Fcb&scope=read%20write&state=xyz&code_challenge=CH&code_challenge_method=S256&audience=api"
        );
        let imp = OAuth2Auth {
            grant_type: OAuth2Grant::Implicit,
            ..cfg()
        };
        assert!(
            authorize_url(&imp, "", "", None)
                .unwrap()
                .contains("response_type=token")
        );
        assert!(
            matches!(authorize_url(&OAuth2Auth { auth_url: None, ..cfg() }, "", "", None), Err(AuthError::Invalid { field, .. }) if field == "auth_url")
        );
        assert!(matches!(
            authorize_url(
                &OAuth2Auth {
                    grant_type: OAuth2Grant::Password,
                    ..cfg()
                },
                "",
                "",
                None
            ),
            Err(AuthError::Unsupported(_))
        ));
    }

    #[test]
    fn token_requests() {
        let r = token_request(
            &cfg(),
            Grant::Code {
                code: "c1",
                redirect_uri: "http://cb",
                code_verifier: Some("v"),
            },
        )
        .unwrap();
        assert_eq!(r.url, "https://auth.example.com/token");
        assert!(r.headers.contains(&(
            "Authorization".into(),
            format!("Basic {}", b64(b"my client:s3cret"))
        )));
        assert_eq!(
            r.form,
            h(&[
                ("grant_type", "authorization_code"),
                ("code", "c1"),
                ("redirect_uri", "http://cb"),
                ("code_verifier", "v"),
                ("audience", "api")
            ])
        );
        let body = OAuth2Auth {
            client_authentication: ClientAuthentication::Body,
            ..cfg()
        };
        let r = token_request(&body, Grant::ClientCredentials).unwrap();
        assert_eq!(r.headers.len(), 1);
        assert_eq!(
            r.form,
            h(&[
                ("grant_type", "client_credentials"),
                ("scope", "read write"),
                ("audience", "api"),
                ("client_id", "my client"),
                ("client_secret", "s3cret")
            ])
        );
        let mut pw = OAuth2Auth {
            username: Some("bob".into()),
            password: Some("pw".into()),
            ..cfg()
        };
        pw.extra_params.insert("x".into(), "1".into());
        let r = token_request(&pw, Grant::Password).unwrap();
        assert_eq!(
            &r.form[..4],
            &h(&[
                ("grant_type", "password"),
                ("username", "bob"),
                ("password", "pw"),
                ("scope", "read write")
            ])[..]
        );
        assert!(r.form.contains(&("x".into(), "1".into())));
        let r = token_request(
            &cfg(),
            Grant::Refresh {
                refresh_token: "rt",
            },
        )
        .unwrap();
        assert_eq!(
            r.form,
            h(&[("grant_type", "refresh_token"), ("refresh_token", "rt")])
        );
        assert!(
            matches!(token_request(&cfg(), Grant::Password), Err(AuthError::Invalid { field, .. }) if field == "username")
        );
        assert!(
            matches!(token_request(&OAuth2Auth::default(), Grant::ClientCredentials), Err(AuthError::Invalid { field, .. }) if field == "access_token_url")
        );
    }

    #[test]
    fn token_responses() {
        let t = parse_token_response(
            200,
            Some("application/json; charset=utf-8"),
            br#"{"access_token":"at","token_type":"Bearer","expires_in":3600,"refresh_token":"rt","scope":"a b"}"#,
            1000,
        )
        .unwrap();
        assert_eq!(
            (
                t.access_token.as_str(),
                t.expires_at,
                t.refresh_token.as_deref()
            ),
            ("at", Some(4600), Some("rt"))
        );
        assert_eq!(t.raw["scope"], "a b");
        // GitHub legacy form-encoded response.
        let t = parse_token_response(
            200,
            Some("application/x-www-form-urlencoded"),
            b"access_token=gho_1&scope=repo&token_type=bearer",
            0,
        )
        .unwrap();
        assert_eq!(
            (t.access_token.as_str(), t.scope.as_deref()),
            ("gho_1", Some("repo"))
        );
        // Errors.
        let e = parse_token_response(
            400,
            Some("application/json"),
            br#"{"error":"invalid_grant","error_description":"Bad code"}"#,
            0,
        )
        .unwrap_err();
        assert_eq!(
            e,
            AuthError::Invalid {
                field: "token_response".into(),
                message: "invalid_grant: Bad code".into()
            }
        );
        let e = parse_token_response(200, None, b"error=bad_verification_code", 0).unwrap_err();
        assert!(e.to_string().contains("bad_verification_code"));
        let e = parse_token_response(502, Some("text/html"), b"<html>Bad gateway</html>", 0)
            .unwrap_err()
            .to_string();
        assert!(e.contains("HTTP 502") && e.contains("Bad gateway"), "{e}");
        assert!(parse_token_response(200, Some("application/json"), b"{}", 0).is_err());
    }

    #[test]
    fn redirects() {
        let c = cfg();
        assert_eq!(
            parse_redirect(&c, "http://127.0.0.1:5555/cb?code=abc&state=xyz", "xyz", 0).unwrap(),
            RedirectResult::Code("abc".into())
        );
        assert!(
            matches!(parse_redirect(&c, "http://127.0.0.1/cb?code=abc&state=evil", "xyz", 0), Err(AuthError::Invalid { field, .. }) if field == "state")
        );
        let e = parse_redirect(
            &c,
            "http://127.0.0.1/cb?error=access_denied&error_description=User+said+no&state=xyz",
            "xyz",
            0,
        )
        .unwrap_err();
        assert!(e.to_string().contains("access_denied: User said no"), "{e}");
        let imp = OAuth2Auth {
            grant_type: OAuth2Grant::Implicit,
            ..c.clone()
        };
        match parse_redirect(
            &imp,
            "http://127.0.0.1/cb#access_token=tok&token_type=bearer&expires_in=60&state=xyz",
            "xyz",
            10,
        )
        .unwrap()
        {
            RedirectResult::Token(t) => {
                assert_eq!((t.access_token.as_str(), t.expires_at), ("tok", Some(70)))
            }
            other => panic!("{other:?}"),
        }
        assert!(parse_redirect(&c, "http://127.0.0.1/cb?state=xyz", "xyz", 0).is_err());
    }

    #[test]
    fn apply() {
        assert_eq!(
            apply_token(&cfg(), "t").headers,
            h(&[("Authorization", "Bearer t")])
        );
        let c = OAuth2Auth {
            header_prefix: Some("Token".into()),
            ..cfg()
        };
        assert_eq!(
            apply_token(&c, "t").headers,
            h(&[("Authorization", "Token t")])
        );
        let c = OAuth2Auth {
            add_to: AddTo::Query,
            ..cfg()
        };
        assert_eq!(apply_token(&c, "t").query, h(&[("access_token", "t")]));
    }
}
