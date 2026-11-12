//! Authorization model (D39) — every Postman auth type.
//!
//! CONTRACT (fixed; mirrored by `Auth` in app/src/lib/api.ts). Stored as `[auth]` in a request
//! `.toml`, in `collection.toml`, or in a folder's `folder.toml`. Every string field may contain
//! `{{variables}}`; they are resolved together with the rest of the request.
//!
//! Inheritance: a request/folder with `Inherit` (or no `[auth]`) uses its parent folder's auth,
//! up to the collection; a collection with no auth = `NoAuth`. `NoAuth` stops inheritance.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Auth {
    /// Use the parent folder/collection auth (default for requests and folders).
    #[default]
    Inherit,
    NoAuth,
    ApiKey(ApiKeyAuth),
    Bearer(BearerAuth),
    Jwt(JwtAuth),
    Basic(BasicAuth),
    Digest(DigestAuth),
    Oauth1(OAuth1Auth),
    Oauth2(OAuth2Auth),
    Hawk(HawkAuth),
    Awsv4(AwsV4Auth),
    Ntlm(NtlmAuth),
    Akamai(AkamaiAuth),
    Asap(AsapAuth),
}

impl Auth {
    pub fn is_inherit(&self) -> bool {
        matches!(self, Auth::Inherit)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AddTo {
    #[default]
    Header,
    Query,
    /// OAuth 1.0 only: form body (application/x-www-form-urlencoded).
    Body,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ApiKeyAuth {
    pub key: String,
    pub value: String,
    #[serde(default, rename = "in")]
    pub add_to: AddTo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct BearerAuth {
    pub token: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct BasicAuth {
    pub username: String,
    #[serde(default)]
    pub password: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct JwtAuth {
    /// HS256/384/512, RS256/384/512, PS256/384/512, ES256/384/512.
    pub algorithm: String,
    /// HS*: shared secret.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret: Option<String>,
    #[serde(default)]
    pub secret_base64: bool,
    /// RS*/PS*/ES*: PEM private key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_key: Option<String>,
    /// JSON object (claims).
    #[serde(default)]
    pub payload: String,
    /// Extra JWT header fields, JSON object (e.g. {"kid": "…"}).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
    #[serde(default)]
    pub add_to: AddTo,
    /// Header prefix, default "Bearer".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header_prefix: Option<String>,
    /// Query parameter name, default "token".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query_param: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct DigestAuth {
    pub username: String,
    #[serde(default)]
    pub password: String,
    /// Normally learned from the 401 challenge; set to pre-compute (no extra round trip).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub realm: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
    /// MD5 (default), MD5-sess, SHA-256, SHA-256-sess, SHA-512-256, SHA-512-256-sess.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub algorithm: Option<String>,
    /// "auth", "auth-int" or none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qop: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nonce_count: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_nonce: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opaque: Option<String>,
    /// Postman "Disable retrying the request": don't answer the 401 challenge automatically.
    #[serde(default)]
    pub disable_retry: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct OAuth1Auth {
    /// HMAC-SHA1 (default), HMAC-SHA256, HMAC-SHA512, RSA-SHA1, RSA-SHA256, RSA-SHA512, PLAINTEXT.
    pub signature_method: String,
    pub consumer_key: String,
    #[serde(default)]
    pub consumer_secret: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_secret: Option<String>,
    /// RSA-*: PEM private key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub callback_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verifier: Option<String>,
    /// Fixed timestamp / nonce (tests); generated when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub realm: Option<String>,
    #[serde(default)]
    pub add_to: AddTo,
    /// Add oauth_body_hash (non-form bodies).
    #[serde(default)]
    pub include_body_hash: bool,
    /// Include empty parameters in the signature.
    #[serde(default)]
    pub add_empty_params: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OAuth2Grant {
    #[default]
    AuthorizationCode,
    AuthorizationCodePkce,
    Implicit,
    Password,
    ClientCredentials,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ClientAuthentication {
    /// Send client_id/secret as HTTP Basic (default, like Postman).
    #[default]
    Basic,
    /// Send them in the token request body.
    Body,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct OAuth2Auth {
    pub grant_type: OAuth2Grant,
    /// Name under which obtained tokens are stored (per user, never in the repo).
    pub token_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_token_url: Option<String>,
    #[serde(default)]
    pub client_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    /// Redirect URI. When `use_local_callback` is true the app listens on a loopback port instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub callback_url: Option<String>,
    #[serde(default = "default_true")]
    pub use_local_callback: bool,
    /// PKCE: "S256" (default) or "plain".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_challenge_method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_verifier: Option<String>,
    /// Password grant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(default)]
    pub client_authentication: ClientAuthentication,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audience: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
    /// Extra parameters for the authorization and token requests.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub extra_params: IndexMap<String, String>,
    #[serde(default)]
    pub add_to: AddTo,
    /// Header prefix, default "Bearer".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header_prefix: Option<String>,
    /// Refresh automatically when expired and a refresh token exists.
    #[serde(default = "default_true")]
    pub auto_refresh: bool,
    /// Fetch automatically when there's no valid token (client_credentials/password only).
    #[serde(default = "default_true")]
    pub auto_fetch: bool,
    /// Literal access token to use instead of the token store (e.g. pasted, or `{{token}}`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct HawkAuth {
    pub id: String,
    pub key: String,
    /// "sha256" (default) or "sha1".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub algorithm: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ext: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dlg: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    #[serde(default)]
    pub include_payload_hash: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AwsV4Auth {
    pub access_key: String,
    pub secret_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_token: Option<String>,
    pub region: String,
    pub service: String,
    /// Sign in the query string (presigned URL) instead of headers.
    #[serde(default)]
    pub add_to: AddTo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct NtlmAuth {
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workstation: Option<String>,
    #[serde(default)]
    pub disable_retry: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AkamaiAuth {
    pub client_token: String,
    pub client_secret: String,
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    /// Comma-separated header names to include in the signature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers_to_sign: Option<String>,
    /// Max body bytes to hash (default 131072).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_body_size: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AsapAuth {
    /// RS256 (default), RS384, RS512, PS256, PS384, PS512, ES256, ES384, ES512.
    pub algorithm: String,
    pub issuer: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    pub audience: String,
    pub key_id: String,
    pub private_key: String,
    /// Extra claims, JSON object.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub additional_claims: Option<String>,
    /// Token lifetime in seconds (default 3600).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_in: Option<u64>,
}

/// The auth that applies to the request file `request_path` of `collection`
/// (D39 inheritance): the request's own auth, else the nearest folder's, else
/// the collection's. `Inherit` at the top means `NoAuth`. A path that is not
/// a request of the collection gets the collection's auth.
pub fn effective_auth(
    collection: &crate::model::Collection,
    request_path: &std::path::Path,
) -> Auth {
    use crate::model::Folder;
    /// Folders from the root down to the one containing the request.
    fn find<'a>(
        folder: &'a Folder,
        path: &std::path::Path,
        chain: &mut Vec<&'a Folder>,
    ) -> Option<&'a Auth> {
        chain.push(folder);
        if let Some(r) = folder.requests.iter().find(|r| r.path == path) {
            return Some(&r.request.auth);
        }
        for f in &folder.folders {
            if let Some(a) = find(f, path, chain) {
                return Some(a);
            }
        }
        chain.pop();
        None
    }
    let mut chain = Vec::new();
    let own = find(&collection.root, request_path, &mut chain);
    std::iter::once(own)
        .flatten()
        // Skip the root folder: its auth is the collection's.
        .chain(chain.iter().skip(1).rev().map(|f| &f.auth))
        .chain(std::iter::once(&collection.auth))
        .find(|a| !a.is_inherit())
        .cloned()
        .unwrap_or(Auth::NoAuth)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toml_round_trip_tagged() {
        let a = Auth::ApiKey(ApiKeyAuth {
            key: "X-Key".into(),
            value: "{{k}}".into(),
            add_to: AddTo::Query,
        });
        let s = toml::to_string(&a).unwrap();
        assert!(
            s.contains("type = \"api_key\"") && s.contains("in = \"query\""),
            "{s}"
        );
        assert_eq!(toml::from_str::<Auth>(&s).unwrap(), a);
        let o: Auth = toml::from_str(
            "type = \"oauth2\"\ngrant_type = \"client_credentials\"\ntoken_name = \"t\"",
        )
        .unwrap();
        match o {
            Auth::Oauth2(c) => assert!(c.auto_refresh && c.use_local_callback && c.auto_fetch),
            _ => panic!(),
        }
        assert_eq!(
            toml::from_str::<Auth>("type = \"no_auth\"").unwrap(),
            Auth::NoAuth
        );
    }

    #[test]
    fn inheritance() {
        use crate::model::{Collection, Folder, Request, RequestFile};
        use std::path::PathBuf;
        let req = |p: &str, auth: Auth| RequestFile {
            path: PathBuf::from(p),
            request: Request {
                name: p.into(),
                method: Default::default(),
                url: String::new(),
                headers: Default::default(),
                query: Default::default(),
                body: None,
                scripts: Default::default(),
                settings: Default::default(),
                auth,
            },
        };
        let bearer = |t: &str| Auth::Bearer(BearerAuth { token: t.into() });
        let c = Collection {
            auth: bearer("collection"),
            root: Folder {
                requests: vec![req("/c/root.toml", Auth::Inherit)],
                folders: vec![Folder {
                    auth: bearer("outer"),
                    requests: vec![req("/c/a/own.toml", bearer("own"))],
                    folders: vec![
                        Folder {
                            auth: Auth::Inherit,
                            requests: vec![req("/c/a/b/x.toml", Auth::Inherit)],
                            ..Default::default()
                        },
                        Folder {
                            auth: Auth::NoAuth,
                            requests: vec![req("/c/a/n/y.toml", Auth::Inherit)],
                            ..Default::default()
                        },
                    ],
                    ..Default::default()
                }],
                ..Default::default()
            },
            ..Default::default()
        };
        let eff = |p: &str| effective_auth(&c, std::path::Path::new(p));
        assert_eq!(eff("/c/root.toml"), bearer("collection"));
        assert_eq!(eff("/c/a/own.toml"), bearer("own"));
        assert_eq!(eff("/c/a/b/x.toml"), bearer("outer"));
        assert_eq!(eff("/c/a/n/y.toml"), Auth::NoAuth);
        assert_eq!(eff("/elsewhere.toml"), bearer("collection"));
        let none = Collection::default();
        assert_eq!(
            effective_auth(&none, std::path::Path::new("/x")),
            Auth::NoAuth
        );
    }
}
