//! The single-request pipeline shared by the app, the collection runner, the
//! CLI and load tests:
//!
//! pre-request script → resolve `{{vars}}` → apply auth → send → test script.

use std::time::Duration;

use ilixpatch_core::Request;
use ilixpatch_engine::{Engine, RequestError, Response, SendOptions};
use ilixpatch_scripting::{
    ResponseView, ScriptInput, ScriptKind, ScriptOutput, TestResult, VarUpdate, run_script,
};
use indexmap::IndexMap;
use serde_json::Value;

use crate::auth::{OAuth2Provider, apply_auth};
use crate::vars::VarStore;

/// Per-execution context (iteration info, timeout, script toggle, OAuth 2.0).
#[derive(Clone, Copy)]
pub struct ExecContext<'a> {
    /// Row of the data file for this iteration.
    pub iteration_data: Option<&'a IndexMap<String, Value>>,
    /// 0-based.
    pub iteration: usize,
    pub iteration_count: usize,
    /// Total timeout for the HTTP send.
    pub timeout: Option<Duration>,
    /// Run pre-request and test scripts.
    pub run_scripts: bool,
    /// Supplies OAuth 2.0 tokens when the auth has no literal `access_token`.
    pub oauth2: Option<&'a dyn OAuth2Provider>,
    /// D41: keep the response body. When `false` it is still kept if the
    /// request has a tests script (and scripts run); otherwise only its size
    /// is counted.
    pub keep_body: bool,
}

impl std::fmt::Debug for ExecContext<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExecContext")
            .field("iteration", &self.iteration)
            .field("iteration_count", &self.iteration_count)
            .field("timeout", &self.timeout)
            .field("run_scripts", &self.run_scripts)
            .field("oauth2", &self.oauth2.is_some())
            .finish_non_exhaustive()
    }
}

impl Default for ExecContext<'_> {
    fn default() -> Self {
        Self {
            iteration_data: None,
            iteration: 0,
            iteration_count: 1,
            timeout: None,
            run_scripts: true,
            oauth2: None,
            keep_body: true,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ExecResult {
    /// The request as sent (resolved), or the (possibly script-modified)
    /// unresolved request if it could not be resolved.
    pub request: Option<Request>,
    pub response: Option<Response>,
    /// Error that prevented a response (variables or network).
    pub error: Option<String>,
    /// The same failure, classified (D36): kind, message, facts.
    pub request_error: Option<RequestError>,
    pub tests: Vec<TestResult>,
    /// `console.*` output of both scripts, in order.
    pub logs: Vec<String>,
    /// First script error (pre-request or tests).
    pub script_error: Option<String>,
    /// All variable writes of both scripts (already applied to the store).
    pub variable_updates: Vec<VarUpdate>,
    /// `postman.setNextRequest`: `Some(Some(name))` jump, `Some(None)` stop.
    pub next_request: Option<Option<String>>,
    /// `pm.execution.skipRequest()` was called: nothing was sent.
    pub skipped: bool,
    pub prerequest_executed: bool,
    pub prerequest_failed: bool,
    pub test_executed: bool,
    pub test_failed: bool,
}

impl ExecResult {
    pub fn failed_tests(&self) -> usize {
        self.tests
            .iter()
            .filter(|t| !t.passed && !t.skipped)
            .count()
    }

    fn absorb(&mut self, out: ScriptOutput, store: &mut VarStore) -> bool {
        store.apply(&out.variable_updates);
        self.variable_updates.extend(out.variable_updates);
        self.tests.extend(out.tests);
        self.logs.extend(out.logs);
        if out.next_request.is_some() {
            self.next_request = out.next_request;
        }
        let failed = out.error.is_some();
        if let Some(e) = out.error
            && self.script_error.is_none()
        {
            self.script_error = Some(e);
        }
        failed
    }
}

fn has_code(s: &Option<String>) -> Option<&str> {
    s.as_deref().filter(|s| !s.trim().is_empty())
}

fn script_input<'a>(
    store: &'a VarStore,
    ctx: &ExecContext<'a>,
    request: &'a Request,
    response: Option<&'a ResponseView>,
) -> ScriptInput<'a> {
    ScriptInput {
        request: Some(request),
        response,
        globals: Some(&store.globals),
        collection_variables: Some(&store.collection),
        environment: Some(&store.environment),
        environment_name: store.environment_name.as_deref(),
        iteration_data: ctx.iteration_data,
        local: Some(&store.local),
        iteration: ctx.iteration,
        iteration_count: ctx.iteration_count,
        request_name: Some(&request.name),
    }
}

/// Run the full pipeline for one request. Script variable writes are applied
/// to `store` as they happen (so a pre-request `pm.environment.set` is visible
/// when resolving `{{vars}}`), and are also returned.
pub async fn execute_request(
    engine: &Engine,
    request: &Request,
    store: &mut VarStore,
    ctx: &ExecContext<'_>,
) -> ExecResult {
    let mut res = ExecResult::default();
    let mut request = std::borrow::Cow::Borrowed(request);

    // 1. Pre-request script.
    if ctx.run_scripts
        && let Some(src) = has_code(&request.scripts.pre_request).map(str::to_owned)
    {
        let out = run_script(
            ScriptKind::PreRequest,
            &src,
            &script_input(store, ctx, &request, None),
        );
        res.prerequest_executed = true;
        let skip = out.skip_request;
        let modified = out.request.clone();
        res.prerequest_failed = res.absorb(out, store);
        if let Some(mut m) = modified {
            // Scripts can't change the request's scripts.
            m.scripts = request.scripts.clone();
            m.settings = request.settings;
            m.auth = request.auth.clone();
            request = std::borrow::Cow::Owned(m);
        }
        if skip {
            res.skipped = true;
            res.request = Some(request.into_owned());
            return res;
        }
    }

    // 2. Resolve variables.
    let resolved = match store.scope(ctx.iteration_data).resolve_request(&request) {
        Ok(r) => r,
        Err(e) => {
            let re = RequestError::from_var_error(&e, &request.url);
            res.error = Some(re.detail.clone());
            res.request_error = Some(re);
            res.request = Some(request.into_owned());
            return res;
        }
    };

    // 3. Authorization (D39): sign, or let the engine run a challenge.
    let (resolved, challenge) = match apply_auth(resolved.clone(), ctx.oauth2, &mut res.logs).await
    {
        Ok(r) => r,
        Err(re) => {
            res.error = Some(re.detail.clone());
            res.request_error = Some(*re);
            res.request = Some(resolved);
            return res;
        }
    };

    // 4. Send.
    let needs_body =
        ctx.keep_body || (ctx.run_scripts && has_code(&request.scripts.tests).is_some());
    let opts = SendOptions {
        timeout: ctx.timeout,
        discard_body: !needs_body,
    };
    let sent = engine.send_opts(&resolved, opts, challenge.as_ref()).await;
    let response = match sent {
        Ok(r) => r,
        Err(e) => {
            let re = engine.describe_error(&e, &resolved, ctx.timeout);
            res.error = Some(re.detail.clone());
            res.request_error = Some(re);
            res.request = Some(resolved);
            return res;
        }
    };

    // 5. Test script.
    if ctx.run_scripts
        && let Some(src) = has_code(&request.scripts.tests)
    {
        let view = ResponseView {
            status: response.status,
            status_text: response.status_text.clone(),
            headers: response.headers.clone(),
            body: script_body(&response, &mut res.logs),
            elapsed_ms: response.elapsed_ms,
            size_bytes: response.size_bytes,
        };
        let out = run_script(
            ScriptKind::Test,
            src,
            &script_input(store, ctx, &resolved, Some(&view)),
        );
        res.test_executed = true;
        res.test_failed = res.absorb(out, store);
    }

    res.request = Some(resolved);
    res.response = Some(response);
    res
}

/// `Display` of an error and its sources, e.g. "error sending request: dns error: …".
pub fn error_chain(e: &dyn std::error::Error) -> String {
    let mut s = e.to_string();
    let mut src = e.source();
    while let Some(inner) = src {
        let msg = inner.to_string();
        if !s.contains(&msg) {
            s.push_str(": ");
            s.push_str(&msg);
        }
        src = inner.source();
    }
    s
}

/// Most bytes of a body given to `pm.response.text()/json()` (D41).
pub const SCRIPT_BODY_LIMIT: usize = 64 * 1024 * 1024;

/// The body for test scripts, read up to [`SCRIPT_BODY_LIMIT`] (with a log
/// warning when cut).
fn script_body(response: &Response, logs: &mut Vec<String>) -> String {
    if !response.body_complete {
        logs.push(format!(
            "[warn] the response body was cut at {} bytes (max_body_mb)",
            response.size_bytes
        ));
    }
    match response.body.text_limited(SCRIPT_BODY_LIMIT) {
        Ok((text, truncated)) => {
            if truncated {
                logs.push(format!(
                    "[warn] pm.response sees only the first {} MB of a {} byte body",
                    SCRIPT_BODY_LIMIT / (1024 * 1024),
                    response.body.len()
                ));
            }
            text
        }
        Err(e) => {
            logs.push(format!("[warn] could not read the response body: {e}"));
            String::new()
        }
    }
}
