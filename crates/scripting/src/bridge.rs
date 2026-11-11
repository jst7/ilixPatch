//! JSON bridge between Rust types and the JS prelude.

use ilixpatch_core::{Body, HttpMethod, Request};
use indexmap::IndexMap;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{ScriptInput, ScriptKind, ScriptOutput, TestResult, VarUpdate};

fn pairs(map: Option<&IndexMap<String, String>>) -> Value {
    Value::Array(
        map.into_iter()
            .flatten()
            .map(|(k, v)| json!([k, v]))
            .collect(),
    )
}

/// The URL as the script sees it: `url` plus the `query` table appended.
pub(crate) fn full_url(req: &Request) -> String {
    if req.query.is_empty() {
        return req.url.clone();
    }
    let qs: Vec<String> = req.query.iter().map(|(k, v)| format!("{k}={v}")).collect();
    let sep = if req.url.contains('?') { '&' } else { '?' };
    format!("{}{sep}{}", req.url, qs.join("&"))
}

fn body_json(body: Option<&Body>) -> Value {
    match body {
        None => Value::Null,
        Some(Body::Json { content }) => json!({"mode": "raw", "raw": content, "language": "json"}),
        Some(Body::Text { content }) => json!({"mode": "raw", "raw": content, "language": "text"}),
        Some(Body::Form { fields }) => {
            json!({"mode": "urlencoded", "urlencoded": pairs(Some(fields))})
        }
    }
}

pub(crate) fn input_json(input: &ScriptInput) -> String {
    let request = input.request.map(|r| {
        json!({
            "name": r.name,
            "method": r.method.as_str(),
            "url": full_url(r),
            "headers": pairs(Some(&r.headers)),
            "body": body_json(r.body.as_ref()),
        })
    });
    let response = input.response.map(|r| {
        json!({
            "code": r.status,
            "status": r.status_text,
            "headers": r.headers.iter().map(|(k, v)| json!([k, v])).collect::<Vec<_>>(),
            "body": r.body,
            "time": r.elapsed_ms,
            "size": r.size_bytes,
        })
    });
    json!({
        "request": request,
        "response": response,
        "globals": pairs(input.globals),
        "collection": pairs(input.collection_variables),
        "environment": pairs(input.environment),
        "envName": input.environment_name,
        "data": input.iteration_data.map(|d| json!(d)).unwrap_or_else(|| json!({})),
        "local": pairs(input.local),
        "iteration": input.iteration,
        "iterationCount": input.iteration_count,
        "requestName": input.request_name,
    })
    .to_string()
}

#[derive(Debug, Deserialize)]
pub(crate) struct JsResult {
    #[serde(default)]
    tests: Vec<JsTest>,
    #[serde(default)]
    updates: Vec<VarUpdate>,
    #[serde(default)]
    logs: Vec<String>,
    #[serde(default)]
    next: Option<JsNext>,
    #[serde(default)]
    skip: bool,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    request: Option<JsRequest>,
}

#[derive(Debug, Deserialize)]
struct JsTest {
    name: String,
    passed: bool,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    skipped: bool,
}

#[derive(Debug, Deserialize)]
struct JsNext {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct JsRequest {
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: Option<JsBody>,
}

#[derive(Debug, Deserialize)]
struct JsBody {
    mode: Option<String>,
    #[serde(default)]
    raw: Option<String>,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    urlencoded: Vec<(String, String)>,
}

fn parse_method(m: &str) -> Option<HttpMethod> {
    Some(match m.to_ascii_uppercase().as_str() {
        "GET" => HttpMethod::Get,
        "POST" => HttpMethod::Post,
        "PUT" => HttpMethod::Put,
        "PATCH" => HttpMethod::Patch,
        "DELETE" => HttpMethod::Delete,
        "HEAD" => HttpMethod::Head,
        "OPTIONS" => HttpMethod::Options,
        _ => return None,
    })
}

fn apply_request(orig: &Request, js: JsRequest) -> Request {
    let mut req = orig.clone();
    if let Some(m) = parse_method(&js.method) {
        req.method = m;
    }
    if js.url != full_url(orig) {
        // The script sees (and edits) url+query as one string.
        req.url = js.url;
        req.query.clear();
    }
    req.headers = js.headers.into_iter().collect();
    req.body = match js.body {
        None => None,
        Some(b) => match b.mode.as_deref() {
            Some("urlencoded") => Some(Body::Form {
                fields: b.urlencoded.into_iter().collect(),
            }),
            Some("raw") => {
                let content = b.raw.unwrap_or_default();
                let is_json = match (&orig.body, b.language.as_deref()) {
                    (Some(Body::Json { .. }), _) | (_, Some("json")) => true,
                    (Some(Body::Text { .. }), _) | (_, Some(_)) => false,
                    _ => serde_json::from_str::<Value>(&content).is_ok(),
                };
                Some(if is_json {
                    Body::Json { content }
                } else {
                    Body::Text { content }
                })
            }
            _ => orig.body.clone(),
        },
    };
    req
}

pub(crate) fn into_output(kind: ScriptKind, res: JsResult, orig: Option<&Request>) -> ScriptOutput {
    let request = match (kind, orig, res.request) {
        (ScriptKind::PreRequest, Some(orig), Some(js)) => {
            let new = apply_request(orig, js);
            (new != *orig).then_some(new)
        }
        _ => None,
    };
    ScriptOutput {
        tests: res
            .tests
            .into_iter()
            .map(|t| TestResult {
                name: t.name,
                passed: t.passed,
                error: t.error,
                skipped: t.skipped,
            })
            .collect(),
        variable_updates: res.updates,
        request,
        logs: res.logs,
        error: res.error,
        next_request: res.next.map(|n| n.name),
        skip_request: kind == ScriptKind::PreRequest && res.skip,
    }
}
