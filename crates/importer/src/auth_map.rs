//! Postman `auth` blocks → ilixPatch [`Auth`] (D39).
//!
//! Postman stores parameters as `[{key, value}]` (v2.1) or `{key: value}`
//! (v2.0). OAuth 2.0 configuration is imported, tokens are not (they are
//! per-user and live in the user's token store).

use ilixpatch_core::auth::*;
use serde_json::Value;

use crate::parse::{str_at, text};

/// Raw parameter of an auth block.
fn param_value<'a>(auth: &'a Value, typ: &str, key: &str) -> Option<&'a Value> {
    match auth.get(typ)? {
        Value::Array(items) => items
            .iter()
            .find(|i| str_at(i, "key").as_deref() == Some(key))
            .and_then(|i| i.get("value")),
        Value::Object(o) => o.get(key),
        _ => None,
    }
}

struct P<'a> {
    auth: &'a Value,
    typ: &'a str,
}

impl P<'_> {
    fn s(&self, key: &str) -> String {
        self.opt(key).unwrap_or_default()
    }
    fn opt(&self, key: &str) -> Option<String> {
        param_value(self.auth, self.typ, key)
            .and_then(text)
            .filter(|s| !s.is_empty())
    }
    fn b(&self, key: &str) -> bool {
        match param_value(self.auth, self.typ, key) {
            Some(Value::Bool(b)) => *b,
            Some(Value::String(s)) => s == "true",
            _ => false,
        }
    }
    fn raw(&self, key: &str) -> Option<&Value> {
        param_value(self.auth, self.typ, key)
    }
}

/// Map a Postman auth block. Warnings are `(code, message)`.
pub(crate) fn map_auth(auth: &Value, warn: &mut dyn FnMut(&str, String)) -> Auth {
    let typ = str_at(auth, "type").unwrap_or_default();
    let p = P { auth, typ: &typ };
    match typ.as_str() {
        "" | "inherit" => Auth::Inherit,
        "noauth" => Auth::NoAuth,
        "apikey" => Auth::ApiKey(ApiKeyAuth {
            key: p.s("key"),
            value: p.s("value"),
            add_to: if p.opt("in").as_deref() == Some("query") {
                AddTo::Query
            } else {
                AddTo::Header
            },
        }),
        "bearer" => Auth::Bearer(BearerAuth {
            token: p.s("token"),
        }),
        "basic" => Auth::Basic(BasicAuth {
            username: p.s("username"),
            password: p.s("password"),
        }),
        "digest" => Auth::Digest(DigestAuth {
            username: p.s("username"),
            password: p.s("password"),
            realm: p.opt("realm"),
            nonce: p.opt("nonce"),
            algorithm: p.opt("algorithm"),
            qop: p.opt("qop"),
            nonce_count: p.opt("nonceCount"),
            client_nonce: p.opt("clientNonce"),
            opaque: p.opt("opaque"),
            disable_retry: p.b("disableRetryRequest"),
        }),
        "oauth1" => Auth::Oauth1(OAuth1Auth {
            signature_method: p
                .opt("signatureMethod")
                .unwrap_or_else(|| "HMAC-SHA1".into()),
            consumer_key: p.s("consumerKey"),
            consumer_secret: p.s("consumerSecret"),
            token: p.opt("token"),
            token_secret: p.opt("tokenSecret"),
            private_key: p.opt("privateKey"),
            callback_url: p.opt("callback"),
            verifier: p.opt("verifier"),
            timestamp: p.opt("timestamp"),
            nonce: p.opt("nonce"),
            version: p.opt("version"),
            realm: p.opt("realm"),
            add_to: if p.b("addParamsToHeader") {
                AddTo::Header
            } else {
                AddTo::Query
            },
            include_body_hash: p.b("includeBodyHash"),
            add_empty_params: p.b("addEmptyParamsToSign"),
        }),
        "oauth2" => {
            if p.opt("accessToken").is_some() {
                warn(
                    "oauth2_token_not_imported",
                    "OAuth 2.0 settings were imported but not the access token: get a new token in the Authorization tab".into(),
                );
            }
            let grant_type = match p.opt("grant_type").as_deref() {
                Some("authorization_code_with_pkce") => OAuth2Grant::AuthorizationCodePkce,
                Some("implicit") => OAuth2Grant::Implicit,
                Some("password_credentials") | Some("password") => OAuth2Grant::Password,
                Some("client_credentials") => OAuth2Grant::ClientCredentials,
                _ => OAuth2Grant::AuthorizationCode,
            };
            let mut extra_params = indexmap::IndexMap::new();
            for key in ["authRequestParams", "tokenRequestParams"] {
                if let Some(Value::Array(items)) = p.raw(key) {
                    for i in items {
                        let enabled = i.get("enabled").and_then(Value::as_bool).unwrap_or(true);
                        if let (true, Some(k)) = (enabled, str_at(i, "key")) {
                            extra_params.insert(k, str_at(i, "value").unwrap_or_default());
                        }
                    }
                }
            }
            Auth::Oauth2(OAuth2Auth {
                grant_type,
                token_name: p.opt("tokenName").unwrap_or_else(|| "default".into()),
                auth_url: p.opt("authUrl"),
                access_token_url: p.opt("accessTokenUrl"),
                client_id: p.s("clientId"),
                client_secret: p.opt("clientSecret"),
                scope: p.opt("scope"),
                state: p.opt("state"),
                callback_url: p.opt("redirect_uri"),
                use_local_callback: true,
                code_challenge_method: p.opt("challengeAlgorithm"),
                code_verifier: p.opt("code_verifier"),
                username: p.opt("username"),
                password: p.opt("password"),
                client_authentication: if p.opt("client_authentication").as_deref() == Some("body")
                {
                    ClientAuthentication::Body
                } else {
                    ClientAuthentication::Basic
                },
                audience: p.opt("audience"),
                resource: p.opt("resource"),
                extra_params,
                add_to: if p.opt("addTokenTo").as_deref() == Some("queryParams") {
                    AddTo::Query
                } else {
                    AddTo::Header
                },
                header_prefix: p.opt("headerPrefix"),
                auto_refresh: true,
                auto_fetch: true,
                access_token: None,
            })
        }
        "hawk" => Auth::Hawk(HawkAuth {
            id: p.s("authId"),
            key: p.s("authKey"),
            algorithm: p.opt("algorithm"),
            user: p.opt("user"),
            nonce: p.opt("nonce"),
            ext: p.opt("extraData"),
            app: p.opt("app"),
            dlg: p.opt("delegation"),
            timestamp: p.opt("timestamp"),
            include_payload_hash: p.b("includePayloadHash"),
        }),
        "awsv4" => Auth::Awsv4(AwsV4Auth {
            access_key: p.s("accessKey"),
            secret_key: p.s("secretKey"),
            session_token: p.opt("sessionToken"),
            region: p.s("region"),
            service: p.s("service"),
            add_to: if p.b("addAuthDataToQuery") {
                AddTo::Query
            } else {
                AddTo::Header
            },
        }),
        "ntlm" => Auth::Ntlm(NtlmAuth {
            username: p.s("username"),
            password: p.s("password"),
            domain: p.opt("domain"),
            workstation: p.opt("workstation"),
            disable_retry: p.b("disableRetryRequest"),
        }),
        "edgegrid" => Auth::Akamai(AkamaiAuth {
            client_token: p.s("clientToken"),
            client_secret: p.s("clientSecret"),
            access_token: p.s("accessToken"),
            base_url: p.opt("baseURL"),
            nonce: p.opt("nonce"),
            timestamp: p.opt("timestamp"),
            headers_to_sign: p.opt("headersToSign"),
            max_body_size: p.opt("maxBodySize").and_then(|s| s.parse().ok()),
        }),
        "jwt" => Auth::Jwt(JwtAuth {
            algorithm: p.opt("algorithm").unwrap_or_else(|| "HS256".into()),
            secret: p.opt("secret"),
            secret_base64: p.b("isSecretBase64Encoded"),
            private_key: p.opt("privateKey"),
            payload: p.opt("payload").unwrap_or_else(|| "{}".into()),
            header: p.opt("header"),
            add_to: if p.opt("addTokenTo").as_deref() == Some("queryParam") {
                AddTo::Query
            } else {
                AddTo::Header
            },
            header_prefix: p.opt("headerPrefix"),
            query_param: p.opt("queryParamKey"),
        }),
        "asap" => Auth::Asap(AsapAuth {
            algorithm: p.opt("alg").unwrap_or_else(|| "RS256".into()),
            issuer: p.s("iss"),
            subject: p.opt("sub"),
            audience: p.s("aud"),
            key_id: p.s("kid"),
            private_key: p.s("privateKey"),
            additional_claims: p.opt("claims"),
            expires_in: p.opt("exp").and_then(|s| parse_seconds(&s)),
        }),
        other => {
            warn(
                "auth_unsupported",
                format!("{other} auth is not supported and was not imported"),
            );
            Auth::Inherit
        }
    }
}

/// `3600`, `"3600"`, or Postman's `"1h"` / `"30m"` / `"45s"` → seconds.
fn parse_seconds(s: &str) -> Option<u64> {
    let s = s.trim();
    let (num, mult) = match s.chars().last()? {
        'h' => (&s[..s.len() - 1], 3600),
        'm' => (&s[..s.len() - 1], 60),
        's' => (&s[..s.len() - 1], 1),
        _ => (s, 1),
    };
    num.trim().parse::<u64>().ok().map(|n| n * mult)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seconds() {
        assert_eq!(parse_seconds("1h"), Some(3600));
        assert_eq!(parse_seconds("90"), Some(90));
        assert_eq!(parse_seconds("x"), None);
    }
}
