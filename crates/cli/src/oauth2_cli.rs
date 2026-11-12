//! OAuth 2.0 tokens for `run` / `load` (D39).
//!
//! Reads the same per-user token store as the desktop app
//! (`$ILIXPATCH_DATA_DIR`, else `<OS data dir>/dev.ilixpatch.app`, file
//! `oauth2-tokens.json`), refreshes expired tokens, and fetches
//! client_credentials / password tokens on its own. Browser grants
//! (authorization code, PKCE, implicit) need a token obtained in the app first.

use std::path::PathBuf;
use std::sync::Arc;

use ilixpatch_oauth2::{Flows, TokenProvider, TokenStore};
use ilixpatch_runner::OAuth2Handle;

/// The app data dir shared with the desktop app.
pub fn data_dir() -> Option<PathBuf> {
    match std::env::var_os("ILIXPATCH_DATA_DIR") {
        Some(dir) if !dir.is_empty() => Some(PathBuf::from(dir)),
        _ => dirs::data_dir().map(|d| d.join(crate::APP_IDENTIFIER)),
    }
}

pub fn provider(engine: ilixpatch_engine::Engine) -> Option<OAuth2Handle> {
    let store = TokenStore::new(data_dir()?);
    let client_for: ilixpatch_oauth2::ClientFor =
        Arc::new(move |url: &str| engine.client_for_url(url).map_err(|e| e.to_string()));
    let flows = Flows::new(None).with_client_for(client_for);
    let provider = TokenProvider::new(Arc::new(store), Arc::new(flows));
    Some(OAuth2Handle(Arc::new(provider)))
}
