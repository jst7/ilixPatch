//! Load testing: virtual users (tokio tasks) loop over a target's requests
//! while latency is aggregated in HDR histograms.
//!
//! Every VU shares one [`Engine`] (one connection pool). Each finished request
//! takes a `std::sync::Mutex` for a few hundred nanoseconds to record into the
//! global and per-request histograms; no lock is ever held across an `.await`.
//! At the request rates a single machine can generate against real servers,
//! this lock is far from being a bottleneck (recording is O(1)).

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hdrhistogram::Histogram;
use ilixpatch_core::HttpMethod;
use ilixpatch_engine::Engine;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::RunError;
use crate::exec::{ExecContext, execute_request};
use crate::run::EnvSelection;
use crate::target::{EnvScope, RunItem, RunTarget, environment_selection, load_globals_map};
use crate::vars::VarStore;
use ilixpatch_engine::RequestErrorKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoadProfile {
    /// All VUs from the start.
    #[default]
    Fixed,
    /// Linear ramp to all VUs over `ramp_up`, then hold.
    RampUp,
    /// 20% of the VUs, jumping to 100% during the middle 20% of the test.
    Spike,
    /// Ramp up, hold, ramp down (symmetric).
    Peak,
}

/// Options from the UI (`LoadOptions` in `app/src/lib/api.ts`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadOptions {
    pub target: String,
    #[serde(default)]
    pub environment: Option<String>,
    /// Where `environment` is looked up (default: project).
    #[serde(default)]
    pub environment_scope: Option<EnvScope>,
    pub virtual_users: usize,
    pub duration_secs: f64,
    #[serde(default)]
    pub ramp_up_secs: Option<f64>,
    pub profile: LoadProfile,
    #[serde(default)]
    pub think_time_ms: Option<u64>,
    /// Default true.
    #[serde(default)]
    pub run_scripts: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct LoadConfig {
    pub environment: Option<EnvSelection>,
    pub env_overrides: IndexMap<String, String>,
    pub globals: IndexMap<String, String>,
    pub virtual_users: usize,
    pub duration: Duration,
    /// For `RampUp`/`Peak`. `None`/zero = 25% of the duration.
    pub ramp_up: Option<Duration>,
    pub profile: LoadProfile,
    pub think_time: Duration,
    pub run_scripts: bool,
    pub timeout: Option<Duration>,
    /// Snapshot period (default 1 s).
    pub interval: Duration,
    /// OAuth 2.0 token supplier (D39).
    pub oauth2: Option<crate::auth::OAuth2Handle>,
}

impl Default for LoadConfig {
    fn default() -> Self {
        Self {
            environment: None,
            env_overrides: IndexMap::new(),
            globals: IndexMap::new(),
            virtual_users: 1,
            duration: Duration::from_secs(10),
            ramp_up: None,
            profile: LoadProfile::Fixed,
            think_time: Duration::ZERO,
            run_scripts: true,
            timeout: Some(Duration::from_secs(30)),
            interval: Duration::from_secs(1),
            oauth2: None,
        }
    }
}

impl LoadConfig {
    pub fn from_options(o: &LoadOptions) -> Result<Self, RunError> {
        Self::from_options_in(o, None)
    }

    /// Like [`LoadConfig::from_options`], reading global environments and
    /// `globals.toml` from `config_dir` (the app config dir) when given.
    pub fn from_options_in(o: &LoadOptions, config_dir: Option<&Path>) -> Result<Self, RunError> {
        if o.virtual_users == 0 {
            return Err(RunError::Invalid("virtual_users must be at least 1".into()));
        }
        if !(o.duration_secs.is_finite() && o.duration_secs > 0.0) {
            return Err(RunError::Invalid("duration_secs must be > 0".into()));
        }
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
            virtual_users: o.virtual_users,
            duration: Duration::from_secs_f64(o.duration_secs),
            ramp_up: o
                .ramp_up_secs
                .filter(|s| s.is_finite() && *s > 0.0)
                .map(Duration::from_secs_f64),
            profile: o.profile,
            think_time: Duration::from_millis(o.think_time_ms.unwrap_or(0)),
            run_scripts: o.run_scripts.unwrap_or(true),
            ..Default::default()
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LoadRequestStats {
    pub path: String,
    pub name: String,
    pub method: HttpMethod,
    pub count: u64,
    pub errors: u64,
    pub avg_ms: f64,
    pub p95_ms: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct LoadSnapshot {
    pub elapsed_secs: f64,
    pub active_vus: usize,
    pub requests: u64,
    pub errors: u64,
    /// Requests/s over the last interval (whole run for the final snapshot).
    pub rps: f64,
    pub avg_ms: f64,
    pub p50_ms: f64,
    pub p90_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
    pub error_rate: f64,
    pub per_request: Vec<LoadRequestStats>,
    #[serde(rename = "final")]
    pub is_final: bool,
}

/// Number of VUs that should be active `t` into the test.
pub fn target_vus(
    profile: LoadProfile,
    vus: usize,
    duration: Duration,
    ramp_up: Option<Duration>,
    t: Duration,
) -> usize {
    let vus = vus.max(1);
    let d = duration.as_secs_f64().max(f64::EPSILON);
    let t = t.as_secs_f64();
    let ramp = ramp_up
        .map(|r| r.as_secs_f64())
        .filter(|r| *r > 0.0)
        .unwrap_or(d * 0.25);
    let scaled = |frac: f64| ((vus as f64 * frac).ceil() as usize).clamp(1, vus);
    match profile {
        LoadProfile::Fixed => vus,
        LoadProfile::RampUp => scaled((t / ramp).min(1.0)),
        LoadProfile::Spike => {
            if t >= d * 0.4 && t < d * 0.6 {
                vus
            } else {
                scaled(0.2)
            }
        }
        LoadProfile::Peak => {
            let ramp = ramp.min(d / 2.0);
            let up = t / ramp;
            let down = (d - t) / ramp;
            scaled(up.min(down).clamp(0.0, 1.0))
        }
    }
}

fn new_hist() -> Histogram<u64> {
    // Microseconds, 1 µs .. 1 h, 3 significant digits.
    Histogram::new_with_bounds(1, 3_600_000_000, 3).expect("valid histogram bounds")
}

struct Bucket {
    hist: Histogram<u64>,
    count: u64,
    errors: u64,
}

impl Bucket {
    fn new() -> Self {
        Self {
            hist: new_hist(),
            count: 0,
            errors: 0,
        }
    }

    fn record(&mut self, latency_us: Option<u64>, error: bool) {
        self.count += 1;
        self.errors += error as u64;
        if let Some(us) = latency_us {
            self.hist.saturating_record(us.max(1));
        }
    }

    fn ms(&self, q: f64) -> f64 {
        if self.hist.is_empty() {
            0.0
        } else {
            self.hist.value_at_quantile(q) as f64 / 1000.0
        }
    }

    fn avg_ms(&self) -> f64 {
        if self.hist.is_empty() {
            0.0
        } else {
            self.hist.mean() / 1000.0
        }
    }
}

struct Stats {
    total: Bucket,
    per_request: Vec<Bucket>,
}

struct LoadItem {
    target: usize,
    item: RunItem,
}

#[allow(clippy::too_many_arguments)]
async fn virtual_user(
    engine: Engine,
    items: Arc<Vec<LoadItem>>,
    base: Arc<Vec<VarStore>>,
    stats: Arc<Mutex<Stats>>,
    token: CancellationToken,
    think_time: Duration,
    run_scripts: bool,
    timeout: Option<Duration>,
    oauth2: Option<crate::auth::OAuth2Handle>,
) {
    let mut stores: Vec<VarStore> = (*base).clone();
    let mut iteration = 0usize;
    loop {
        for (idx, li) in items.iter().enumerate() {
            if token.is_cancelled() {
                return;
            }
            let ctx = ExecContext {
                iteration_data: None,
                iteration,
                iteration_count: 0,
                timeout,
                run_scripts,
                oauth2: oauth2.as_ref().map(|h| &*h.0),
                // Count bytes only, unless a tests script needs the body.
                keep_body: false,
            };
            let start = Instant::now();
            let store = &mut stores[li.target];
            let res = tokio::select! {
                _ = token.cancelled() => return,
                r = execute_request(&engine, &li.item.request, store, &ctx) => r,
            };
            let latency_us = match &res.response {
                Some(r) => Some((r.elapsed_ms * 1000.0) as u64),
                // Nothing was sent: don't pollute latency percentiles.
                None if res.skipped
                    || res.request_error.as_ref().is_some_and(|e| {
                        matches!(
                            e.kind,
                            RequestErrorKind::UndefinedVariables | RequestErrorKind::InvalidUrl
                        )
                    }) =>
                {
                    None
                }
                None => Some(start.elapsed().as_micros() as u64),
            };
            let error = res.error.is_some()
                || res.script_error.is_some()
                || res.failed_tests() > 0
                || res
                    .response
                    .as_ref()
                    .is_some_and(|r| !(200..400).contains(&r.status));
            if !res.skipped {
                let mut s = stats.lock().unwrap_or_else(|e| e.into_inner());
                s.total.record(latency_us, error);
                s.per_request[idx].record(latency_us, error);
            }
            if think_time.is_zero() {
                // Never starve the runtime, e.g. when every request fails instantly.
                tokio::task::yield_now().await;
            } else {
                tokio::select! {
                    _ = token.cancelled() => return,
                    _ = tokio::time::sleep(think_time) => {}
                }
            }
        }
        iteration += 1;
    }
}

fn snapshot(
    stats: &Stats,
    items: &[LoadItem],
    elapsed: Duration,
    active_vus: usize,
    rps: f64,
    is_final: bool,
) -> LoadSnapshot {
    let t = &stats.total;
    LoadSnapshot {
        elapsed_secs: elapsed.as_secs_f64(),
        active_vus,
        requests: t.count,
        errors: t.errors,
        rps,
        avg_ms: t.avg_ms(),
        p50_ms: t.ms(0.5),
        p90_ms: t.ms(0.9),
        p95_ms: t.ms(0.95),
        p99_ms: t.ms(0.99),
        max_ms: if t.hist.is_empty() {
            0.0
        } else {
            t.hist.max() as f64 / 1000.0
        },
        error_rate: if t.count == 0 {
            0.0
        } else {
            t.errors as f64 / t.count as f64
        },
        per_request: items
            .iter()
            .zip(&stats.per_request)
            .map(|(li, b)| LoadRequestStats {
                path: li.item.file.to_string_lossy().into_owned(),
                name: li.item.request.name.clone(),
                method: li.item.request.method,
                count: b.count,
                errors: b.errors,
                avg_ms: b.avg_ms(),
                p95_ms: b.ms(0.95),
            })
            .collect(),
        is_final,
    }
}

/// Run a load test until `config.duration` elapses or `stop` is cancelled.
/// `on_snapshot` gets a snapshot every `config.interval` and a last one with
/// `final = true`, which is also returned. Must run inside a Tokio runtime.
pub async fn run_load_test<F>(
    engine: &Engine,
    targets: &[RunTarget],
    config: &LoadConfig,
    stop: &CancellationToken,
    mut on_snapshot: F,
) -> Result<LoadSnapshot, RunError>
where
    F: FnMut(&LoadSnapshot) + Send,
{
    crate::target::check_environment(targets, config.environment.as_ref())?;
    if config.virtual_users == 0 {
        return Err(RunError::Invalid(
            "at least 1 virtual user is required".into(),
        ));
    }
    let items: Vec<LoadItem> = targets
        .iter()
        .enumerate()
        .flat_map(|(ti, t)| {
            t.items.iter().map(move |i| LoadItem {
                target: ti,
                item: i.clone(),
            })
        })
        .collect();
    if items.is_empty() {
        return Err(RunError::Target("no requests to run".into()));
    }
    let run_config = crate::run::RunConfig {
        environment: config.environment.clone(),
        env_overrides: config.env_overrides.clone(),
        globals: config.globals.clone(),
        ..Default::default()
    };
    let base: Arc<Vec<VarStore>> = Arc::new(
        targets
            .iter()
            .map(|t| crate::run::initial_store(t, &run_config))
            .collect(),
    );
    let stats = Arc::new(Mutex::new(Stats {
        total: Bucket::new(),
        per_request: items.iter().map(|_| Bucket::new()).collect(),
    }));
    let items = Arc::new(items);

    let all = stop.child_token();
    let mut vus: Vec<CancellationToken> = Vec::new();
    let mut handles = Vec::new();
    let start = Instant::now();
    let mut ticker = tokio::time::interval(Duration::from_millis(50));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let interval = if config.interval.is_zero() {
        Duration::from_secs(1)
    } else {
        config.interval
    };
    let mut next_snapshot = interval;
    let mut last = (Duration::ZERO, 0u64);

    loop {
        tokio::select! {
            _ = all.cancelled() => break,
            _ = ticker.tick() => {}
        }
        let t = start.elapsed();
        if t >= config.duration {
            break;
        }
        let want = target_vus(
            config.profile,
            config.virtual_users,
            config.duration,
            config.ramp_up,
            t,
        );
        while vus.len() < want {
            let token = all.child_token();
            handles.push(tokio::spawn(virtual_user(
                engine.clone(),
                items.clone(),
                base.clone(),
                stats.clone(),
                token.clone(),
                config.think_time,
                config.run_scripts,
                config.timeout,
                config.oauth2.clone(),
            )));
            vus.push(token);
        }
        while vus.len() > want {
            if let Some(token) = vus.pop() {
                token.cancel();
            }
        }
        if t >= next_snapshot {
            let snap = {
                let s = stats.lock().unwrap_or_else(|e| e.into_inner());
                let dt = (t - last.0).as_secs_f64();
                let rps = if dt > 0.0 {
                    (s.total.count - last.1) as f64 / dt
                } else {
                    0.0
                };
                last = (t, s.total.count);
                snapshot(&s, &items, t, vus.len(), rps, false)
            };
            on_snapshot(&snap);
            while next_snapshot <= t {
                next_snapshot += interval;
            }
        }
    }

    all.cancel();
    for h in handles {
        let _ = h.await;
    }
    let elapsed = start.elapsed();
    let snap = {
        let s = stats.lock().unwrap_or_else(|e| e.into_inner());
        let rps = s.total.count as f64 / elapsed.as_secs_f64().max(f64::EPSILON);
        snapshot(&s, &items, elapsed, 0, rps, true)
    };
    on_snapshot(&snap);
    Ok(snap)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(profile: LoadProfile, secs: f64) -> usize {
        target_vus(
            profile,
            10,
            Duration::from_secs(100),
            Some(Duration::from_secs(20)),
            Duration::from_secs_f64(secs),
        )
    }

    #[test]
    fn profiles() {
        assert_eq!(at(LoadProfile::Fixed, 0.0), 10);
        assert_eq!(at(LoadProfile::RampUp, 0.0), 1);
        assert_eq!(at(LoadProfile::RampUp, 10.0), 5);
        assert_eq!(at(LoadProfile::RampUp, 50.0), 10);
        assert_eq!(at(LoadProfile::Spike, 10.0), 2);
        assert_eq!(at(LoadProfile::Spike, 50.0), 10);
        assert_eq!(at(LoadProfile::Spike, 70.0), 2);
        assert_eq!(at(LoadProfile::Peak, 10.0), 5);
        assert_eq!(at(LoadProfile::Peak, 50.0), 10);
        assert_eq!(at(LoadProfile::Peak, 90.0), 5);
        assert_eq!(at(LoadProfile::Peak, 99.9), 1);
    }

    #[test]
    fn options_deserialize() {
        let o: LoadOptions = serde_json::from_str(
            r#"{"target":"/x","virtual_users":3,"duration_secs":5,"profile":"ramp_up"}"#,
        )
        .unwrap();
        assert_eq!(o.profile, LoadProfile::RampUp);
        let c = LoadConfig::from_options(&o).unwrap();
        assert!(c.run_scripts);
        assert_eq!(c.duration, Duration::from_secs(5));
    }
}
