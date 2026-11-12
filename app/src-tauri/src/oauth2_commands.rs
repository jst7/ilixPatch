//! OAuth 2.0 commands (D39): get/refresh tokens through the browser or the
//! token endpoint, and manage the per-user token store.

use std::sync::Arc;

use ilixpatch_core::Scope;
use ilixpatch_core::auth::OAuth2Auth;
use ilixpatch_oauth2::{Flows, Opener, StoredToken, TokenProvider, TokenStore, resolve_config};
use tauri::{App, Manager, State};

use crate::SendVariables;

type CmdResult<T> = Result<T, String>;

/// Shared by the commands and by sends/runs (token provider).
pub struct OAuth2State {
    pub provider: Arc<TokenProvider>,
}

impl OAuth2State {
    /// For run/load configurations.
    pub fn handle(&self) -> ilixpatch_runner::OAuth2Handle {
        ilixpatch_runner::OAuth2Handle(self.provider.clone())
    }
}

/// Token store in `<app data dir>/oauth2-tokens.json`; the browser opens via the opener plugin.
pub fn init(app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    let dir = app.path().app_data_dir()?;
    let opener: Opener = Arc::new(|url: &str| {
        tauri_plugin_opener::open_url(url, None::<&str>).map_err(|e| e.to_string())
    });
    // Token requests use the same network settings as sends (proxy, CAs, client
    // certificates), read at request time so settings changes apply immediately.
    let handle = app.handle().clone();
    let client_for: ilixpatch_oauth2::ClientFor = Arc::new(move |url: &str| {
        handle
            .state::<crate::network_commands::SharedEngine>()
            .get()
            .client_for_url(url)
            .map_err(|e| e.to_string())
    });
    let provider = TokenProvider::new(
        Arc::new(TokenStore::new(dir)),
        Arc::new(Flows::new(Some(opener)).with_client_for(client_for)),
    );
    app.manage(OAuth2State {
        provider: Arc::new(provider),
    });
    Ok(())
}

/// globals < collection < environment, like a send.
fn scope(v: &SendVariables) -> Scope {
    Scope::new()
        .push(&v.globals)
        .push(&v.collection)
        .push(&v.environment)
}

#[tauri::command]
pub async fn oauth2_get_token(
    state: State<'_, OAuth2State>,
    config: OAuth2Auth,
    variables: SendVariables,
) -> CmdResult<StoredToken> {
    let cfg = resolve_config(&config, &scope(&variables))?;
    state.provider.fetch_and_store(&cfg).await
}

#[tauri::command]
pub fn oauth2_cancel_flow(state: State<'_, OAuth2State>) -> CmdResult<()> {
    state.provider.flows.cancel();
    Ok(())
}

#[tauri::command]
pub async fn oauth2_refresh_token(
    state: State<'_, OAuth2State>,
    id: String,
    config: OAuth2Auth,
    variables: SendVariables,
) -> CmdResult<StoredToken> {
    let cfg = resolve_config(&config, &scope(&variables))?;
    state.provider.refresh_stored(&id, &cfg).await
}

#[tauri::command(async)]
pub fn oauth2_list_tokens(
    state: State<'_, OAuth2State>,
    token_name: Option<String>,
) -> CmdResult<Vec<StoredToken>> {
    state.provider.store.list(token_name.as_deref())
}

#[tauri::command(async)]
pub fn oauth2_delete_tokens(state: State<'_, OAuth2State>, ids: Vec<String>) -> CmdResult<()> {
    state.provider.store.delete(&ids)
}

#[tauri::command(async)]
pub fn oauth2_select_token(
    state: State<'_, OAuth2State>,
    token_name: String,
    id: String,
) -> CmdResult<()> {
    state.provider.store.select(&token_name, &id)
}
