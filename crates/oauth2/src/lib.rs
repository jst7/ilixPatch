//! OAuth 2.0 for ilixPatch (D39): where tokens come from and where they live.
//!
//! * [`TokenStore`]: per-user JSON file in the app data dir (never in a repo).
//! * [`Flows`]: obtains tokens. client_credentials / password POST to the token
//!   endpoint; authorization_code (+PKCE) / implicit open the system browser and
//!   wait on a loopback (`127.0.0.1`) redirect. Refresh with the refresh grant.
//! * [`TokenProvider`]: the access token for a send: stored → refreshed →
//!   fetched automatically (client_credentials/password) → error.
//!
//! Protocol details (URLs, form bodies, response parsing, PKCE) come from the
//! pure `ilixpatch_auth::oauth2` module; this crate only does the I/O.

mod flows;
mod loopback;
mod provider;
mod store;

use std::time::{SystemTime, UNIX_EPOCH};

use ilixpatch_core::Scope;
use ilixpatch_core::auth::OAuth2Auth;

pub use flows::{ClientFor, Flows, Opener};
pub use provider::TokenProvider;
pub use store::{EXPIRY_SKEW, StoredToken, TOKENS_FILE, TokenStore};

/// Product name shown on the loopback "you can close this window" page.
pub const APP_NAME: &str = "ilixPatch";

pub(crate) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Cryptographically random bytes (OS RNG).
pub(crate) fn fill_random(buf: &mut [u8]) {
    // `state`, PKCE verifiers and token ids are security-sensitive: never fall back
    // to predictable values. getrandom only fails on broken/unsupported platforms.
    getrandom::fill(buf).expect("the operating system's random number generator is unavailable");
}

/// 128-bit random id, hex.
pub(crate) fn random_id() -> String {
    let mut b = [0u8; 16];
    fill_random(&mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Resolve `{{variables}}` in every string field of `cfg`.
pub fn resolve_config(cfg: &OAuth2Auth, scope: &Scope) -> Result<OAuth2Auth, String> {
    fn walk(v: &mut serde_json::Value, scope: &Scope) -> Result<(), String> {
        match v {
            serde_json::Value::String(s) => {
                *s = scope.interpolate(s).map_err(|e| e.to_string())?;
            }
            serde_json::Value::Array(a) => {
                for x in a {
                    walk(x, scope)?;
                }
            }
            serde_json::Value::Object(o) => {
                for (_, x) in o.iter_mut() {
                    walk(x, scope)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    let mut v = serde_json::to_value(cfg).map_err(|e| e.to_string())?;
    walk(&mut v, scope)?;
    serde_json::from_value(v).map_err(|e| e.to_string())
}
