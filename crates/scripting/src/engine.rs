//! QuickJS plumbing: a per-thread cached runtime with chai + the `pm` prelude
//! preloaded, a memory limit and a wall-clock timeout.

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use rquickjs::context::EvalOptions;
use rquickjs::{Context, Ctx, Function, Object, Runtime, Value};

use crate::{HttpSender, ResponseView, SendRequestSpec};

const CHAI_JS: &str = include_str!("../vendor/chai.js");
const PRELUDE_JS: &str = include_str!("prelude.js");

/// Modules for `require()` (D59), evaluated on first use.
fn module_source(name: &str) -> Option<&'static str> {
    Some(match name {
        "crypto-js" => include_str!("../vendor/crypto-js.js"),
        "lodash" => include_str!("../vendor/lodash.min.js"),
        "moment" => include_str!("../vendor/moment.min.js"),
        "uuid" => include_str!("../vendor/uuid.min.js"),
        "xml2js" => include_str!("xml2js.js"),
        _ => return None,
    })
}

/// Most random bytes one `crypto.getRandomValues` call may ask for (like browsers).
const MAX_RANDOM_BYTES: usize = 65536;

/// Wall-clock budget for one script (including timers and promise jobs).
pub(crate) const TIMEOUT: Duration = Duration::from_secs(5);
const MEMORY_LIMIT: usize = 64 * 1024 * 1024;
const MAX_STACK: usize = 512 * 1024;
const NO_DEADLINE: u64 = u64::MAX;

pub(crate) const TIMEOUT_MESSAGE: &str = "script timed out";

pub(crate) struct Engine {
    // Field order matters: the context must be dropped before the runtime.
    ctx: Context,
    _rt: Runtime,
    /// Nanoseconds since `epoch` after which the interrupt handler aborts.
    deadline: Arc<AtomicU64>,
    epoch: Instant,
}

/// Result of driving one script through the engine.
pub(crate) struct RawRun {
    /// JSON produced by `__pmFinish`, if it could be obtained.
    pub json: Option<String>,
    /// Fatal error that escaped the JS-level error handling (timeout, OOM, …).
    pub fatal: Option<String>,
}

impl Engine {
    pub(crate) fn new() -> Result<Self, String> {
        let rt = Runtime::new().map_err(|e| format!("failed to create JS runtime: {e}"))?;
        rt.set_memory_limit(MEMORY_LIMIT);
        rt.set_max_stack_size(MAX_STACK);
        let deadline = Arc::new(AtomicU64::new(NO_DEADLINE));
        let epoch = Instant::now();
        let d = deadline.clone();
        rt.set_interrupt_handler(Some(Box::new(move || {
            let dl = d.load(Ordering::Relaxed);
            dl != NO_DEADLINE && epoch.elapsed().as_nanos() as u64 >= dl
        })));
        let ctx = Context::full(&rt).map_err(|e| format!("failed to create JS context: {e}"))?;
        ctx.with(|ctx| -> Result<(), String> {
            let globals = ctx.globals();
            let host = |e: rquickjs::Error| format!("failed to register host functions: {e}");
            globals
                .set(
                    "__pmModuleSource",
                    Function::new(ctx.clone(), |name: String| module_source(&name))
                        .map_err(host)?,
                )
                .map_err(host)?;
            globals
                .set(
                    "__pmRandomBytes",
                    Function::new(ctx.clone(), |n: usize| -> Vec<u8> {
                        let mut buf = vec![0u8; n.min(MAX_RANDOM_BYTES)];
                        // On failure the bytes stay zero; there is no better source.
                        let _ = getrandom::fill(&mut buf);
                        buf
                    })
                    .map_err(host)?,
                )
                .map_err(host)?;
            let mut opts = EvalOptions::default();
            opts.strict = false;
            opts.filename = Some("chai.js".into());
            ctx.eval_with_options::<(), _>(CHAI_JS, opts)
                .map_err(|e| format!("failed to load chai: {}", describe(&ctx, e)))?;
            let mut opts = EvalOptions::default();
            opts.strict = false;
            opts.filename = Some("prelude.js".into());
            ctx.eval_with_options::<(), _>(PRELUDE_JS, opts)
                .map_err(|e| format!("failed to load prelude: {}", describe(&ctx, e)))?;
            Ok(())
        })?;
        Ok(Self {
            ctx,
            _rt: rt,
            deadline,
            epoch,
        })
    }

    fn now_nanos(&self) -> u64 {
        self.epoch.elapsed().as_nanos() as u64
    }

    fn timed_out(&self) -> bool {
        let dl = self.deadline.load(Ordering::Relaxed);
        dl != NO_DEADLINE && self.now_nanos() >= dl
    }

    pub(crate) fn run(
        &self,
        kind: &str,
        source: &str,
        input_json: &str,
        timeout: Duration,
        http: Option<HttpSender<'_>>,
    ) -> RawRun {
        self.deadline.store(
            self.now_nanos().saturating_add(timeout.as_nanos() as u64),
            Ordering::Relaxed,
        );
        self.ctx.with(|ctx| {
            let fatal = self.drive(&ctx, kind, source, input_json, http).err();
            let timed_out = self.timed_out();
            self.deadline.store(NO_DEADLINE, Ordering::Relaxed);
            let fatal = if timed_out {
                Some(TIMEOUT_MESSAGE.to_string())
            } else {
                fatal
            };
            let json = call::<String>(&ctx, "__pmFinish", ()).ok();
            RawRun { json, fatal }
        })
    }

    fn drive(
        &self,
        ctx: &Ctx<'_>,
        kind: &str,
        source: &str,
        input_json: &str,
        http: Option<HttpSender<'_>>,
    ) -> Result<(), String> {
        call::<()>(ctx, "__pmRun", (kind, source, input_json))?;
        loop {
            while ctx.execute_pending_job() {
                if self.timed_out() {
                    return Err(TIMEOUT_MESSAGE.into());
                }
            }
            if self.timed_out() {
                return Err(TIMEOUT_MESSAGE.into());
            }
            // pm.sendRequest calls made so far: send them, then run their callbacks.
            let pending: String = call(ctx, "__pmTakeSends", ())?;
            let pending: Vec<(u64, SendRequestSpec)> = serde_json::from_str(&pending)
                .map_err(|e| format!("internal error reading pm.sendRequest: {e}"))?;
            if !pending.is_empty() {
                for (id, spec) in pending {
                    let result = self.send(http, &spec);
                    call::<()>(ctx, "__pmDeliver", (id, result))?;
                }
                continue;
            }
            if !call::<bool>(ctx, "__pmTick", ())? {
                break;
            }
        }
        Ok(())
    }

    /// One `pm.sendRequest`, as JSON for `__pmDeliver`. The wait doesn't count
    /// against the script's time budget (the deadline moves forward).
    fn send(&self, http: Option<HttpSender<'_>>, spec: &SendRequestSpec) -> String {
        let Some(HttpSender(send)) = http else {
            return serde_json::json!({ "error": "pm.sendRequest is not available here" })
                .to_string();
        };
        let started = Instant::now();
        let result = send(spec);
        let waited = started.elapsed().as_nanos() as u64;
        let _ = self
            .deadline
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |d| {
                (d != NO_DEADLINE).then(|| d.saturating_add(waited))
            });
        match result {
            Ok(r) => serde_json::json!({ "ok": response_json(&r) }).to_string(),
            Err(e) => serde_json::json!({ "error": e }).to_string(),
        }
    }
}

/// A response as `makeResponse` in the prelude expects it.
pub(crate) fn response_json(r: &ResponseView) -> serde_json::Value {
    serde_json::json!({
        "code": r.status,
        "status": r.status_text,
        "headers": r.headers.iter().map(|(k, v)| serde_json::json!([k, v])).collect::<Vec<_>>(),
        "body": r.body,
        "time": r.elapsed_ms,
        "size": r.size_bytes,
    })
}

fn call<'js, R: rquickjs::FromJs<'js>>(
    ctx: &Ctx<'js>,
    name: &str,
    args: impl rquickjs::function::IntoArgs<'js>,
) -> Result<R, String> {
    let f: Function = ctx.globals().get(name).map_err(|e| describe(ctx, e))?;
    f.call(args).map_err(|e| describe(ctx, e))
}

/// Turn an rquickjs error (possibly a pending JS exception) into a message.
fn describe(ctx: &Ctx<'_>, err: rquickjs::Error) -> String {
    if !matches!(err, rquickjs::Error::Exception) {
        return err.to_string();
    }
    let v: Value = ctx.catch();
    if let Some(obj) = v.as_object() {
        let obj: &Object = obj;
        let name: Option<String> = obj.get("name").ok();
        let msg: Option<String> = obj.get("message").ok();
        return match (name, msg) {
            (Some(n), Some(m)) if !m.is_empty() => format!("{n}: {m}"),
            (Some(n), _) => n,
            (None, Some(m)) => m,
            (None, None) => "unknown JavaScript exception".into(),
        };
    }
    if let Some(s) = v.as_string() {
        return s.to_string().unwrap_or_default();
    }
    format!("{v:?}")
}

thread_local! {
    static ENGINE: RefCell<Option<Engine>> = const { RefCell::new(None) };
}

/// Run with the thread's cached engine, creating it on first use. The engine is
/// discarded after a fatal error (timeout, out of memory…) so the next call
/// starts from a clean state.
pub(crate) fn with_engine(
    kind: &str,
    source: &str,
    input_json: &str,
    timeout: Duration,
    http: Option<HttpSender<'_>>,
) -> RawRun {
    ENGINE.with(|cell| {
        let mut slot = match cell.try_borrow_mut() {
            Ok(s) => s,
            Err(_) => {
                return RawRun {
                    json: None,
                    fatal: Some("scripting engine is busy (re-entrant call)".into()),
                };
            }
        };
        if slot.is_none() {
            match Engine::new() {
                Ok(e) => *slot = Some(e),
                Err(e) => {
                    return RawRun {
                        json: None,
                        fatal: Some(e),
                    };
                }
            }
        }
        let engine = slot.as_ref().expect("engine initialized above");
        let run = engine.run(kind, source, input_json, timeout, http);
        if run.fatal.is_some() {
            *slot = None;
        }
        run
    })
}
