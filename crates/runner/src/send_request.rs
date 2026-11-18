//! `pm.sendRequest` (D59): requests made by scripts, sent with the same network
//! engine (TLS, proxy, certificates, network console) as the request itself.
//!
//! Scripts run synchronously, so a call blocks the script until the response
//! arrives. On a multi-thread tokio runtime (app, CLI) the current worker is
//! handed over with `block_in_place`; elsewhere (current-thread runtimes, no
//! runtime) the request runs on a helper thread with its own runtime and a
//! fresh engine with the same settings (a connection pool is tied to the
//! runtime that created its connections).

use std::time::Duration;

use ilixpatch_core::{Body, HttpMethod, Request};
use ilixpatch_engine::{Engine, SendOptions};
use ilixpatch_scripting::{ResponseView, SendBody, SendRequestSpec};
use indexmap::IndexMap;

use crate::exec::{ExecContext, SCRIPT_BODY_LIMIT};

fn method(m: &str) -> Result<HttpMethod, String> {
    Ok(match m.to_ascii_uppercase().as_str() {
        "GET" => HttpMethod::Get,
        "POST" => HttpMethod::Post,
        "PUT" => HttpMethod::Put,
        "PATCH" => HttpMethod::Patch,
        "DELETE" => HttpMethod::Delete,
        "HEAD" => HttpMethod::Head,
        "OPTIONS" => HttpMethod::Options,
        other => return Err(format!("pm.sendRequest: method {other} is not supported")),
    })
}

/// The script's request as an ilixPatch request (no `{{vars}}` left: the script resolved them).
pub(crate) fn to_request(spec: &SendRequestSpec) -> Result<Request, String> {
    if spec.url.trim().is_empty() {
        return Err("pm.sendRequest: the URL is empty".into());
    }
    let mut headers = IndexMap::new();
    for (k, v) in &spec.headers {
        headers.insert(k.clone(), v.clone());
    }
    let body = spec.body.as_ref().map(|b| match b {
        SendBody::Raw(content) => Body::Text {
            content: content.clone(),
        },
        SendBody::Form(fields) => Body::Form {
            fields: fields.iter().cloned().collect(),
        },
    });
    Ok(Request {
        name: "pm.sendRequest".into(),
        method: method(&spec.method)?,
        url: spec.url.clone(),
        headers,
        query: IndexMap::new(),
        body,
        scripts: Default::default(),
        settings: Default::default(),
        auth: Default::default(),
        extract: Vec::new(),
    })
}

async fn send_async(
    engine: &Engine,
    req: &Request,
    timeout: Option<Duration>,
    trace: Option<std::sync::Arc<ilixpatch_engine::Trace>>,
) -> Result<ResponseView, String> {
    let opts = SendOptions {
        timeout,
        discard_body: false,
        trace,
        phase: None,
    };
    match engine.send_opts(req, opts, None).await {
        Ok(r) => Ok(ResponseView {
            status: r.status,
            status_text: r.status_text.clone(),
            headers: r.headers.clone(),
            body: r
                .body
                .text_limited(SCRIPT_BODY_LIMIT)
                .map(|(text, _)| text)
                .unwrap_or_default(),
            elapsed_ms: r.elapsed_ms,
            size_bytes: r.size_bytes,
        }),
        Err(e) => Err(engine.describe_error(&e, req, timeout).detail),
    }
}

/// Send one `pm.sendRequest`, blocking the calling (script) thread.
pub(crate) fn send_blocking(
    engine: &Engine,
    ctx: &ExecContext<'_>,
    spec: &SendRequestSpec,
) -> Result<ResponseView, String> {
    let req = to_request(spec)?;
    let timeout = ctx.timeout;
    let trace = ctx.trace.clone();
    if let Ok(handle) = tokio::runtime::Handle::try_current()
        && handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread
    {
        return tokio::task::block_in_place(|| {
            handle.block_on(send_async(engine, &req, timeout, trace))
        });
    }
    let settings = engine.settings().clone();
    std::thread::scope(|s| {
        s.spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| format!("pm.sendRequest: {e}"))?;
            let engine =
                Engine::with_settings(settings).map_err(|e| format!("pm.sendRequest: {e}"))?;
            rt.block_on(send_async(&engine, &req, timeout, trace))
        })
        .join()
        .unwrap_or_else(|_| Err("pm.sendRequest: internal error".into()))
    })
}
