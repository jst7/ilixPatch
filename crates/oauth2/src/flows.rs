//! Obtaining and refreshing tokens.

use std::sync::Arc;
use std::time::Duration;

use ilixpatch_auth::AuthEnv;
use ilixpatch_auth::oauth2::{self, Grant, RedirectResult, Token};
use ilixpatch_core::auth::{OAuth2Auth, OAuth2Grant};
use tokio::sync::Notify;

use crate::loopback::{Loopback, parse_local_url};
use crate::{fill_random, now};

/// Opens a URL in the user's browser (injectable for tests and the CLI).
pub type Opener = Arc<dyn Fn(&str) -> Result<(), String> + Send + Sync>;

/// How long the browser step may take.
const BROWSER_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// Returns the HTTP client to use for a token endpoint URL (e.g. one honouring the
/// app's network settings: proxy, extra CAs, client certificate for that host).
pub type ClientFor = Arc<dyn Fn(&str) -> Result<reqwest::Client, String> + Send + Sync>;

pub struct Flows {
    http: reqwest::Client,
    /// When set, used instead of `http` for every token request (looked up per request,
    /// so network-settings changes apply immediately).
    client_for: Option<ClientFor>,
    opener: Option<Opener>,
    /// D46: network console.
    observer: Option<ilixpatch_engine::NetObserver>,
    cancel: Notify,
    browser_timeout: Duration,
}

fn auth_err(e: ilixpatch_auth::AuthError) -> String {
    e.to_string()
}

impl Flows {
    /// `opener` is required for browser grants; without it they fail with a clear error.
    pub fn new(opener: Option<Opener>) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .unwrap_or_default(),
            client_for: None,
            opener,
            observer: None,
            cancel: Notify::new(),
            browser_timeout: BROWSER_TIMEOUT,
        }
    }

    /// Use another HTTP client (e.g. one honouring the network settings).
    pub fn with_client(mut self, client: reqwest::Client) -> Self {
        self.http = client;
        self
    }

    /// Pick the HTTP client per token request (see [`ClientFor`]).
    pub fn with_client_for(mut self, client_for: ClientFor) -> Self {
        self.client_for = Some(client_for);
        self
    }

    /// D46: report token requests to the network console.
    pub fn with_observer(mut self, observer: ilixpatch_engine::NetObserver) -> Self {
        self.observer = Some(observer);
        self
    }

    pub fn with_browser_timeout(mut self, timeout: Duration) -> Self {
        self.browser_timeout = timeout;
        self
    }

    /// Abort a browser flow waiting for its redirect.
    pub fn cancel(&self) {
        self.cancel.notify_waiters();
    }

    /// POST to the token endpoint.
    async fn token_endpoint(&self, cfg: &OAuth2Auth, grant: Grant<'_>) -> Result<Token, String> {
        let req = oauth2::token_request(cfg, grant).map_err(auth_err)?;
        let http = match &self.client_for {
            Some(f) => f(&req.url)?,
            None => self.http.clone(),
        };
        let mut builder = http
            .post(&req.url)
            .timeout(Duration::from_secs(60))
            .form(&req.form);
        if !req
            .headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("accept"))
        {
            builder = builder.header("Accept", "application/json");
        }
        for (k, v) in &req.headers {
            builder = builder.header(k, v);
        }
        use ilixpatch_engine::netlog::{self, BodyPreview, NetExchange};
        let request = builder
            .build()
            .map_err(|e| format!("token request to {} failed: {e}", req.url))?;
        // D46: one console entry per token request.
        let mut logged = self.observer.as_ref().map(|obs| {
            let (h, b) = netlog::capture_request(&request, None);
            let trace = ilixpatch_engine::Trace::new("OAuth 2.0 · token");
            (
                obs,
                NetExchange {
                    trace: Some(trace.id),
                    label: trace.label.clone(),
                    phase: Some("oauth2-token".into()),
                    timestamp_ms: netlog::now_ms(),
                    method: request.method().to_string(),
                    url: request.url().to_string(),
                    request_headers: h,
                    request_body: b,
                    status: None,
                    status_text: None,
                    response_headers: Vec::new(),
                    response_body: None,
                    http_version: None,
                    remote_addr: None,
                    proxy: None,
                    elapsed_ms: 0.0,
                    timings: None,
                    error: None,
                },
            )
        });
        let started = std::time::Instant::now();
        let (sent, conn_timing) = ilixpatch_engine::timing::timed(http.execute(request)).await;
        if let Some((_, ex)) = logged.as_mut() {
            ex.timings = Some(netlog::Timings::from_conn(
                conn_timing,
                started.elapsed().as_secs_f64() * 1000.0,
            ));
        }
        let resp = match sent {
            Ok(r) => r,
            Err(e) => {
                if let Some((obs, mut ex)) = logged.take() {
                    ex.elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
                    ex.error = Some(e.to_string());
                    obs(ex);
                }
                return Err(format!("token request to {} failed: {e}", req.url));
            }
        };
        let status = resp.status().as_u16();
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        if let Some((_, ex)) = logged.as_mut() {
            ex.status = Some(status);
            ex.status_text = resp.status().canonical_reason().map(str::to_string);
            ex.response_headers = netlog::response_headers(&resp);
            ex.remote_addr = resp.remote_addr().map(|a| a.to_string());
            ex.http_version = Some(format!("{:?}", resp.version()));
        }
        let body = resp.bytes().await.map_err(|e| e.to_string());
        if let Some((obs, mut ex)) = logged {
            ex.elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            match &body {
                Ok(b) => ex.response_body = Some(BodyPreview::from_bytes(b, b.len() as u64)),
                Err(e) => ex.error = Some(e.clone()),
            }
            obs(ex);
        }
        let body = body?;
        oauth2::parse_token_response(status, content_type.as_deref(), &body, now())
            .map_err(auth_err)
    }

    /// Run the configured grant (variables already resolved).
    pub async fn get_token(&self, cfg: &OAuth2Auth) -> Result<Token, String> {
        match cfg.grant_type {
            OAuth2Grant::ClientCredentials => {
                self.token_endpoint(cfg, Grant::ClientCredentials).await
            }
            OAuth2Grant::Password => self.token_endpoint(cfg, Grant::Password).await,
            OAuth2Grant::AuthorizationCode
            | OAuth2Grant::AuthorizationCodePkce
            | OAuth2Grant::Implicit => self.browser_flow(cfg).await,
        }
    }

    /// Refresh grant; keeps the old refresh token when the response omits it.
    pub async fn refresh(&self, cfg: &OAuth2Auth, refresh_token: &str) -> Result<Token, String> {
        let mut t = self
            .token_endpoint(cfg, Grant::Refresh { refresh_token })
            .await?;
        if t.refresh_token.as_deref().is_none_or(str::is_empty) {
            t.refresh_token = Some(refresh_token.to_string());
        }
        Ok(t)
    }

    async fn browser_flow(&self, cfg: &OAuth2Auth) -> Result<Token, String> {
        let callback = cfg
            .callback_url
            .as_deref()
            .map(str::trim)
            .filter(|c| !c.is_empty());
        if !cfg.use_local_callback && callback.and_then(parse_local_url).is_none() {
            return Err("Use a localhost callback URL (http://127.0.0.1:<port>/…) or enable the local callback".into());
        }
        let opener = self
            .opener
            .as_ref()
            .ok_or("this grant needs a browser; get the token in the app first")?;
        let loopback = Loopback::bind(callback).await?;
        let redirect_uri = loopback.redirect_uri.clone();

        let state = match cfg.state.as_deref().filter(|s| !s.is_empty()) {
            Some(s) => s.to_string(),
            None => crate::random_id(),
        };
        // (Scoped: AuthEnv isn't Send and must not live across an await.)
        let pkce = (cfg.grant_type == OAuth2Grant::AuthorizationCodePkce).then(|| {
            let mut rng = |b: &mut [u8]| fill_random(b);
            let mut env = AuthEnv {
                now: now(),
                random: &mut rng,
            };
            oauth2::pkce(cfg, &mut env)
        });
        let url = oauth2::authorize_url(
            cfg,
            &redirect_uri,
            &state,
            pkce.as_ref().map(|(_, c)| c.as_str()),
        )
        .map_err(auth_err)?;
        opener(&url)
            .map_err(|e| format!("couldn't open the browser ({e}); open this URL: {url}"))?;

        let redirect = loopback.wait(self.browser_timeout, &self.cancel).await?;
        match oauth2::parse_redirect(cfg, &redirect, &state, now()).map_err(auth_err)? {
            RedirectResult::Token(t) => Ok(t),
            RedirectResult::Code(code) => {
                self.token_endpoint(
                    cfg,
                    Grant::Code {
                        code: &code,
                        redirect_uri: &redirect_uri,
                        code_verifier: pkce.as_ref().map(|(v, _)| v.as_str()),
                    },
                )
                .await
            }
        }
    }
}
