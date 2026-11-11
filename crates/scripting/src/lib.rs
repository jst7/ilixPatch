//! Postman-compatible script sandbox.
//!
//! CONTRACT (fixed — other crates code against it; implementation may change):
//! - [`run_script`] executes one script synchronously and never panics; JS errors
//!   are reported in [`ScriptOutput::error`].
//! - Scripts see a `pm` object compatible with Postman/Newman (see README of this crate).
//! - Variable writes are returned as [`VarUpdate`]s; the caller decides how to apply
//!   them (runner: in memory; app: runtime layer; CLI: `--export-environment`).

use ilixpatch_core::Request;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptKind {
    PreRequest,
    Test,
}

/// Variable scopes, matching Postman's `pm.globals`, `pm.collectionVariables`,
/// `pm.environment` and `pm.variables` (local, current execution only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VarScope {
    Global,
    Collection,
    Environment,
    Local,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VarUpdate {
    pub scope: VarScope,
    pub key: String,
    /// `None` = unset.
    pub value: Option<String>,
}

/// The response as seen by test scripts (`pm.response`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResponseView {
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
    pub elapsed_ms: f64,
    pub size_bytes: usize,
}

/// Everything a script can read. Variable maps are the CURRENT values of each scope.
#[derive(Debug, Clone, Default)]
pub struct ScriptInput<'a> {
    /// Pre-request: the unresolved request (scripts may modify it).
    /// Test: the resolved request that was sent.
    pub request: Option<&'a Request>,
    pub response: Option<&'a ResponseView>,
    pub globals: Option<&'a IndexMap<String, String>>,
    pub collection_variables: Option<&'a IndexMap<String, String>>,
    pub environment: Option<&'a IndexMap<String, String>>,
    pub environment_name: Option<&'a str>,
    /// Row of the data file for this iteration (`pm.iterationData`, `data.*`).
    pub iteration_data: Option<&'a IndexMap<String, serde_json::Value>>,
    /// Local variables (`pm.variables.set`) of the current execution, e.g. written by
    /// the pre-request script and read by the tests script. Highest precedence.
    pub local: Option<&'a IndexMap<String, String>>,
    /// 0-based (`pm.info.iteration`).
    pub iteration: usize,
    pub iteration_count: usize,
    /// `pm.info.requestName`
    pub request_name: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TestResult {
    pub name: String,
    pub passed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// `pm.test.skip(...)`
    #[serde(default)]
    pub skipped: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ScriptOutput {
    pub tests: Vec<TestResult>,
    pub variable_updates: Vec<VarUpdate>,
    /// Pre-request only: the request after script modifications (`pm.request.headers.add`, url, body…).
    /// `None` if unchanged.
    pub request: Option<Request>,
    /// `console.log/info/warn/error` lines, prefixed with the level for non-log ones.
    pub logs: Vec<String>,
    /// Uncaught exception / syntax error / timeout.
    pub error: Option<String>,
    /// `postman.setNextRequest(name)`: `Some(Some(name))`, or `Some(None)` for `setNextRequest(null)` (stop).
    pub next_request: Option<Option<String>>,
    /// `pm.execution.skipRequest()` called in a pre-request script.
    pub skip_request: bool,
}

/// Run a script. Must be cheap to call many times (load tests run it per request):
/// implementations should reuse a runtime/precompiled prelude where possible.
///
/// Each thread keeps one QuickJS runtime with chai and the `pm` prelude
/// preloaded; a call only builds the per-run `pm` object. Scripts are limited to
/// 64 MiB of heap and 5 s of wall-clock time (`"script timed out"`).
pub fn run_script(kind: ScriptKind, source: &str, input: &ScriptInput) -> ScriptOutput {
    run_script_with_timeout(kind, source, input, engine::TIMEOUT)
}

/// Like [`run_script`] with a custom wall-clock timeout.
pub fn run_script_with_timeout(
    kind: ScriptKind,
    source: &str,
    input: &ScriptInput,
    timeout: std::time::Duration,
) -> ScriptOutput {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_inner(kind, source, input, timeout)
    }));
    result.unwrap_or_else(|_| ScriptOutput {
        error: Some("internal error in script engine".into()),
        ..Default::default()
    })
}

fn run_inner(
    kind: ScriptKind,
    source: &str,
    input: &ScriptInput,
    timeout: std::time::Duration,
) -> ScriptOutput {
    let kind_str = match kind {
        ScriptKind::PreRequest => "pre_request",
        ScriptKind::Test => "test",
    };
    let input_json = bridge::input_json(input);
    let raw = engine::with_engine(kind_str, source, &input_json, timeout);
    let mut out = match raw
        .json
        .as_deref()
        .map(serde_json::from_str::<bridge::JsResult>)
    {
        Some(Ok(res)) => bridge::into_output(kind, res, input.request),
        Some(Err(e)) => ScriptOutput {
            error: Some(format!("internal error decoding script result: {e}")),
            ..Default::default()
        },
        None => ScriptOutput::default(),
    };
    if let Some(fatal) = raw.fatal {
        out.error = Some(fatal);
    }
    out
}

mod bridge;
mod engine;

#[cfg(test)]
mod tests;
