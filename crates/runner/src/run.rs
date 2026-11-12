//! Newman-compatible collection runner.

use std::path::Path;
use std::time::{Duration, Instant};

use ilixpatch_core::{Environment, HttpMethod};
use ilixpatch_engine::Engine;
use ilixpatch_scripting::TestResult;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::RunError;
use crate::exec::{ExecContext, ExecResult, execute_request};
use crate::target::{
    DataRow, EnvScope, RunTarget, check_environment, environment_selection, load_data_file,
    load_globals_map,
};
use crate::vars::VarStore;
use ilixpatch_engine::{RequestError, RequestErrorKind};

/// Options from the UI (`RunOptions` in `app/src/lib/api.ts`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RunOptions {
    /// Collection or folder directory (or a single request .toml).
    pub target: String,
    #[serde(default)]
    pub environment: Option<String>,
    /// Where `environment` is looked up (default: project).
    #[serde(default)]
    pub environment_scope: Option<EnvScope>,
    #[serde(default)]
    pub iterations: Option<usize>,
    #[serde(default)]
    pub data_file: Option<String>,
    #[serde(default)]
    pub delay_ms: Option<u64>,
    /// 0 = none.
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub bail: Option<bool>,
    /// Default true.
    #[serde(default)]
    pub persist_variables: Option<bool>,
}

/// How the environment is chosen.
#[derive(Debug, Clone)]
pub enum EnvSelection {
    /// The project environment of that name, falling back to the legacy
    /// `environments/<name>.toml` of each collection.
    Named(String),
    /// Given explicitly (e.g. loaded from a file); applies to every collection.
    Inline(Environment),
}

/// Full runner configuration (superset of [`RunOptions`], used by the CLI).
#[derive(Debug, Clone)]
pub struct RunConfig {
    pub environment: Option<EnvSelection>,
    /// Applied over the environment (`--env-var`).
    pub env_overrides: IndexMap<String, String>,
    /// Initial globals (`--global-var`).
    pub globals: IndexMap<String, String>,
    /// Defaults to the number of data rows, or 1.
    pub iterations: Option<usize>,
    pub data: Vec<DataRow>,
    pub delay: Duration,
    pub timeout: Option<Duration>,
    pub bail: bool,
    pub persist_variables: bool,
    /// Keep response bodies/headers in [`RequestExtra`] (verbose output, reports).
    pub keep_bodies: bool,
    /// OAuth 2.0 token supplier (D39); `None`: only literal `access_token`s work.
    pub oauth2: Option<crate::auth::OAuth2Handle>,
}

impl Default for RunConfig {
    fn default() -> Self {
        Self {
            environment: None,
            env_overrides: IndexMap::new(),
            globals: IndexMap::new(),
            iterations: None,
            data: Vec::new(),
            delay: Duration::ZERO,
            timeout: None,
            bail: false,
            persist_variables: true,
            keep_bodies: false,
            oauth2: None,
        }
    }
}

impl RunConfig {
    /// Build from UI options; loads the data file. Global environments need
    /// [`RunConfig::from_options_in`].
    pub fn from_options(o: &RunOptions) -> Result<Self, RunError> {
        Self::from_options_in(o, None)
    }

    /// Like [`RunConfig::from_options`], reading global environments and
    /// `globals.toml` from `config_dir` (the app config dir) when given.
    pub fn from_options_in(o: &RunOptions, config_dir: Option<&Path>) -> Result<Self, RunError> {
        let data = match o.data_file.as_deref().filter(|s| !s.is_empty()) {
            Some(p) => load_data_file(p)?,
            None => Vec::new(),
        };
        Ok(Self {
            environment: environment_selection(
                o.environment.as_deref(),
                o.environment_scope,
                config_dir,
            )?,
            globals: match config_dir {
                Some(dir) => load_globals_map(dir)?,
                None => IndexMap::new(),
            },
            iterations: o.iterations.filter(|&n| n > 0),
            data,
            delay: Duration::from_millis(o.delay_ms.unwrap_or(0)),
            timeout: o.timeout_ms.filter(|&t| t > 0).map(Duration::from_millis),
            bail: o.bail.unwrap_or(false),
            persist_variables: o.persist_variables.unwrap_or(true),
            ..Default::default()
        })
    }

    pub fn iteration_count(&self) -> usize {
        self.iterations
            .unwrap_or(if self.data.is_empty() {
                1
            } else {
                self.data.len()
            })
            .max(1)
    }
}

/// One executed request (`RequestRunResult` in api.ts).
#[derive(Debug, Clone, Serialize)]
pub struct RequestRunResult {
    /// 0-based.
    pub iteration: usize,
    /// Request file path.
    pub path: String,
    pub name: String,
    pub method: HttpMethod,
    pub url: String,
    pub status: Option<u16>,
    pub status_text: Option<String>,
    pub elapsed_ms: f64,
    pub size_bytes: usize,
    pub tests: Vec<TestResult>,
    pub logs: Vec<String>,
    /// Network / variable / script error.
    pub error: Option<String>,
    /// D36: category of `error` when it's a send failure (`None` for
    /// script/test failures).
    pub error_kind: Option<RequestErrorKind>,
    /// Extra details for the CLI and reporters; not part of the UI contract.
    #[serde(skip)]
    pub extra: RequestExtra,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct RequestExtra {
    pub collection: String,
    /// Path inside the collection, e.g. `users/List users`.
    pub item_path: String,
    pub skipped: bool,
    pub prerequest_executed: bool,
    pub prerequest_failed: bool,
    pub test_executed: bool,
    pub test_failed: bool,
    /// Error prevented getting a response (network or unresolved variable).
    pub request_failed: bool,
    pub script_error: Option<String>,
    /// The classified send failure (D36), for hints in the CLI and reports.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_error: Option<RequestError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_headers: Option<Vec<(String, String)>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_body: Option<String>,
}

impl RequestRunResult {
    pub fn failed_tests(&self) -> usize {
        self.tests
            .iter()
            .filter(|t| !t.passed && !t.skipped)
            .count()
    }

    /// Failed request, failed test or script error.
    pub fn failed(&self) -> bool {
        self.error.is_some() || self.failed_tests() > 0
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct RunSummary {
    pub iterations: usize,
    pub requests: usize,
    pub failed_requests: usize,
    pub tests: usize,
    pub failed_tests: usize,
    pub skipped_tests: usize,
    pub total_ms: f64,
    pub avg_response_ms: f64,
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RunEvent {
    Started {
        total_requests: usize,
        iterations: usize,
    },
    Iteration {
        iteration: usize,
    },
    Request {
        result: Box<RequestRunResult>,
    },
    Finished {
        summary: RunSummary,
    },
}

/// Newman-style counters (executed / failed) for the summary table.
#[derive(Debug, Clone, Default, Serialize)]
pub struct RunStats {
    pub iterations: usize,
    pub requests: usize,
    pub failed_requests: usize,
    pub prerequest_scripts: usize,
    pub failed_prerequest_scripts: usize,
    pub test_scripts: usize,
    pub failed_test_scripts: usize,
    pub assertions: usize,
    pub failed_assertions: usize,
    pub skipped_assertions: usize,
}

/// Everything a run produced.
#[derive(Debug, Clone, Serialize)]
pub struct RunReport {
    pub summary: RunSummary,
    pub stats: RunStats,
    pub results: Vec<RequestRunResult>,
    /// Final globals.
    pub globals: IndexMap<String, String>,
    /// Final environment (merged across collections, last wins).
    pub environment: IndexMap<String, String>,
    pub environment_name: Option<String>,
}

/// Initial variable store of a collection.
pub(crate) fn initial_store(t: &RunTarget, config: &RunConfig) -> VarStore {
    let mut store = VarStore {
        globals: config.globals.clone(),
        collection: t.collection.variables.clone(),
        ..Default::default()
    };
    match &config.environment {
        Some(EnvSelection::Named(n)) => {
            if let Some(e) = t.environment(n) {
                store.environment = e.variables.clone();
                store.environment_name = Some(e.name.clone());
            }
        }
        Some(EnvSelection::Inline(e)) => {
            store.environment = e.variables.clone();
            store.environment_name = Some(e.name.clone());
        }
        None => {}
    }
    if !config.env_overrides.is_empty() {
        store.environment.extend(
            config
                .env_overrides
                .iter()
                .map(|(k, v)| (k.clone(), v.clone())),
        );
    }
    store
}

fn to_result(
    res: ExecResult,
    iteration: usize,
    target: &RunTarget,
    item: &crate::target::RunItem,
    keep_bodies: bool,
) -> RequestRunResult {
    let req = res.request.as_ref().unwrap_or(&item.request);
    let resp = res.response.as_ref();
    let error = res.error.clone().or_else(|| {
        res.script_error
            .as_ref()
            .map(|e| format!("script error: {e}"))
    });
    RequestRunResult {
        iteration,
        path: item.file.to_string_lossy().into_owned(),
        name: req.name.clone(),
        method: req.method,
        url: display_url(req),
        status: resp.map(|r| r.status),
        status_text: resp.map(|r| r.status_text.clone()),
        elapsed_ms: resp.map_or(0.0, |r| r.elapsed_ms),
        size_bytes: resp.map_or(0, |r| r.size_bytes),
        extra: RequestExtra {
            collection: target.collection.name.clone(),
            item_path: item.item_path.clone(),
            skipped: res.skipped,
            prerequest_executed: res.prerequest_executed,
            prerequest_failed: res.prerequest_failed,
            test_executed: res.test_executed,
            test_failed: res.test_failed,
            request_failed: res.error.is_some(),
            script_error: res.script_error.clone(),
            request_error: res.request_error.clone(),
            response_headers: resp.filter(|_| keep_bodies).map(|r| r.headers.clone()),
            // Reports keep at most 1 MB of each body (D41).
            response_body: resp.filter(|_| keep_bodies).map(|r| {
                r.body
                    .text_limited(1024 * 1024)
                    .map(|(t, _)| t)
                    .unwrap_or_default()
            }),
        },
        tests: res.tests,
        logs: res.logs,
        error,
        error_kind: res.request_error.as_ref().map(|e| e.kind),
    }
}

/// The URL with its `query` table appended (for display and reports).
fn display_url(req: &ilixpatch_core::Request) -> String {
    if req.query.is_empty() {
        return req.url.clone();
    }
    let qs: Vec<String> = req.query.iter().map(|(k, v)| format!("{k}={v}")).collect();
    let sep = if req.url.contains('?') { '&' } else { '?' };
    format!("{}{sep}{}", req.url, qs.join("&"))
}

/// Run `targets` (from [`crate::resolve_target`]). Events are delivered to
/// `on_event` as they happen. Cancelling `cancel` stops the run as soon as
/// possible (an in-flight request is abandoned) and the summary has
/// `cancelled = true`.
pub async fn run_collection<F>(
    engine: &Engine,
    targets: &[RunTarget],
    config: &RunConfig,
    cancel: &CancellationToken,
    mut on_event: F,
) -> Result<RunReport, RunError>
where
    F: FnMut(&RunEvent) + Send,
{
    check_environment(targets, config.environment.as_ref())?;
    let total_items: usize = targets.iter().map(|t| t.items.len()).sum();
    if total_items == 0 {
        return Err(RunError::Target("no requests to run".into()));
    }

    let started = Instant::now();
    let iterations = config.iteration_count();
    on_event(&RunEvent::Started {
        total_requests: total_items * iterations,
        iterations,
    });

    let initial: Vec<VarStore> = targets.iter().map(|t| initial_store(t, config)).collect();
    let mut stores = initial.clone();
    // Globals are shared by every collection of the run.
    let mut globals = config.globals.clone();

    // Flat list of (target index, item index) for setNextRequest jumps.
    let flat: Vec<(usize, usize)> = targets
        .iter()
        .enumerate()
        .flat_map(|(ti, t)| (0..t.items.len()).map(move |ii| (ti, ii)))
        .collect();

    let mut results = Vec::new();
    let mut stats = RunStats::default();
    let mut cancelled = false;
    let mut first_request = true;

    'outer: for iteration in 0..iterations {
        if cancel.is_cancelled() {
            cancelled = true;
            break;
        }
        stats.iterations += 1;
        on_event(&RunEvent::Iteration { iteration });
        let data = config.data.get(iteration % config.data.len().max(1));
        let ctx = ExecContext {
            iteration_data: data,
            iteration,
            iteration_count: iterations,
            timeout: config.timeout,
            run_scripts: true,
            oauth2: config.oauth2.as_ref().map(|h| &*h.0),
            keep_body: config.keep_bodies,
        };

        let mut pos = 0;
        // Guard against infinite setNextRequest loops.
        let mut steps = 0usize;
        let max_steps = flat.len().saturating_mul(1000).max(10_000);
        while pos < flat.len() {
            steps += 1;
            if steps > max_steps {
                break;
            }
            let (ti, ii) = flat[pos];
            let target = &targets[ti];
            let item = &target.items[ii];

            if !first_request && !config.delay.is_zero() {
                tokio::select! {
                    _ = cancel.cancelled() => { cancelled = true; break 'outer; }
                    _ = tokio::time::sleep(config.delay) => {}
                }
            }
            first_request = false;

            if !config.persist_variables {
                stores[ti] = initial[ti].clone();
                globals = config.globals.clone();
            }
            let store = &mut stores[ti];
            std::mem::swap(&mut store.globals, &mut globals);
            let exec = tokio::select! {
                _ = cancel.cancelled() => None,
                r = execute_request(engine, &item.request, store, &ctx) => Some(r),
            };
            std::mem::swap(&mut store.globals, &mut globals);
            let Some(exec) = exec else {
                cancelled = true;
                break 'outer;
            };

            let next = exec.next_request.clone();
            let result = to_result(exec, iteration, target, item, config.keep_bodies);
            stats.prerequest_scripts += result.extra.prerequest_executed as usize;
            stats.failed_prerequest_scripts += result.extra.prerequest_failed as usize;
            stats.test_scripts += result.extra.test_executed as usize;
            stats.failed_test_scripts += result.extra.test_failed as usize;
            if !result.extra.skipped {
                stats.requests += 1;
                stats.failed_requests += result.error.is_some() as usize;
            }
            for t in &result.tests {
                stats.assertions += 1;
                if t.skipped {
                    stats.skipped_assertions += 1;
                } else if !t.passed {
                    stats.failed_assertions += 1;
                }
            }
            let failed = result.failed();
            on_event(&RunEvent::Request {
                result: Box::new(result.clone()),
            });
            results.push(result);

            if config.bail && failed {
                break 'outer;
            }

            pos = match next {
                None => pos + 1,
                Some(None) => break,
                Some(Some(name)) => match flat
                    .iter()
                    .position(|&(t, i)| targets[t].items[i].request.name == name)
                {
                    Some(p) => p,
                    // Unknown name: Newman ends the iteration.
                    None => break,
                },
            };
        }
    }

    let responded: Vec<f64> = results
        .iter()
        .filter(|r| r.status.is_some())
        .map(|r| r.elapsed_ms)
        .collect();
    let summary = RunSummary {
        iterations: stats.iterations,
        requests: stats.requests,
        failed_requests: stats.failed_requests,
        tests: stats.assertions,
        failed_tests: stats.failed_assertions,
        skipped_tests: stats.skipped_assertions,
        total_ms: started.elapsed().as_secs_f64() * 1000.0,
        avg_response_ms: if responded.is_empty() {
            0.0
        } else {
            responded.iter().sum::<f64>() / responded.len() as f64
        },
        cancelled,
    };
    on_event(&RunEvent::Finished {
        summary: summary.clone(),
    });

    let mut environment = IndexMap::new();
    let mut environment_name = None;
    for s in &stores {
        environment.extend(s.environment.iter().map(|(k, v)| (k.clone(), v.clone())));
        if environment_name.is_none() {
            environment_name.clone_from(&s.environment_name);
        }
    }
    Ok(RunReport {
        summary,
        stats,
        results,
        globals,
        environment,
        environment_name,
    })
}
