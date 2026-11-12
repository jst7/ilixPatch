//! The access token to use when sending a request with OAuth 2.0 auth.

use std::sync::Arc;

use ilixpatch_core::auth::{OAuth2Auth, OAuth2Grant};

use crate::{Flows, StoredToken, TokenStore, now};

pub struct TokenProvider {
    pub store: Arc<TokenStore>,
    pub flows: Arc<Flows>,
    /// Single-flight for refresh/fetch (many virtual users, one token request).
    fetching: tokio::sync::Mutex<()>,
}

impl TokenProvider {
    pub fn new(store: Arc<TokenStore>, flows: Arc<Flows>) -> Self {
        Self {
            store,
            flows,
            fetching: tokio::sync::Mutex::new(()),
        }
    }

    /// Get a token with the configured grant, store it and select it.
    pub async fn fetch_and_store(&self, cfg: &OAuth2Auth) -> Result<StoredToken, String> {
        let token = self.flows.get_token(cfg).await?;
        self.store
            .add(StoredToken::from_token(&token_name(cfg), token, now()))
    }

    /// Refresh stored token `id` in place (same id, stays selected).
    pub async fn refresh_stored(&self, id: &str, cfg: &OAuth2Auth) -> Result<StoredToken, String> {
        let old = self
            .store
            .get(id)?
            .ok_or_else(|| format!("unknown token {id}"))?;
        let refresh = old
            .refresh_token
            .clone()
            .filter(|r| !r.is_empty())
            .ok_or("this token has no refresh token; get a new one")?;
        let t = self.flows.refresh(cfg, &refresh).await?;
        let mut fresh = StoredToken::from_token(&old.token_name, t, now());
        fresh.id = old.id;
        self.store.replace(fresh)
    }

    /// Valid token for `token_name` right now, if any: the current one, else the newest valid.
    fn valid(&self, name: &str) -> Result<Option<StoredToken>, String> {
        let t = now();
        if let Some(c) = self.store.current(name)?.filter(|c| c.is_valid(t)) {
            return Ok(Some(c));
        }
        Ok(self
            .store
            .list(Some(name))?
            .into_iter()
            .find(|c| c.is_valid(t)))
    }

    /// The access token for a request whose OAuth 2.0 config is `cfg` (variables resolved):
    /// a literal `access_token` → the selected/newest valid stored token → refreshed
    /// (`auto_refresh`) → fetched (`auto_fetch`, client_credentials/password) → error.
    pub async fn access_token(&self, cfg: &OAuth2Auth) -> Result<String, String> {
        if let Some(t) = cfg.access_token.as_deref().filter(|t| !t.trim().is_empty()) {
            return Ok(t.to_string());
        }
        let name = token_name(cfg);
        if let Some(t) = self.valid(&name)? {
            return Ok(t.access_token);
        }
        let _g = self.fetching.lock().await;
        // Someone else may have refreshed/fetched while we waited.
        if let Some(t) = self.valid(&name)? {
            return Ok(t.access_token);
        }
        let mut last_err = None;
        if cfg.auto_refresh
            && let Some(cur) = self.store.current(&name)?
            && cur.refresh_token.as_deref().is_some_and(|r| !r.is_empty())
        {
            match self.refresh_stored(&cur.id, cfg).await {
                Ok(t) => return Ok(t.access_token),
                Err(e) => last_err = Some(e),
            }
        }
        let automatic = matches!(
            cfg.grant_type,
            OAuth2Grant::ClientCredentials | OAuth2Grant::Password
        );
        if cfg.auto_fetch && automatic {
            return self.fetch_and_store(cfg).await.map(|t| t.access_token);
        }
        Err(match last_err {
            Some(e) => {
                format!("OAuth 2.0 token for '{name}' expired and couldn't be refreshed: {e}")
            }
            None => format!("No OAuth 2.0 token for '{name}': click Get New Access Token"),
        })
    }
}

impl ilixpatch_runner::OAuth2Provider for TokenProvider {
    fn access_token<'a>(
        &'a self,
        cfg: &'a OAuth2Auth,
    ) -> ilixpatch_runner::BoxFuture<'a, Result<String, String>> {
        Box::pin(TokenProvider::access_token(self, cfg))
    }
}

/// Tokens are stored under `token_name` (or "default" when empty).
pub(crate) fn token_name(cfg: &OAuth2Auth) -> String {
    let n = cfg.token_name.trim();
    if n.is_empty() {
        "default".into()
    } else {
        n.to_string()
    }
}
