//! Postman request item → ilixPatch [`Request`].

use ilixpatch_core::auth::Auth;
use ilixpatch_core::{Body, HttpMethod, Request, Scripts};
use indexmap::IndexMap;
use serde_json::Value;

use crate::ImportWarning;
use crate::auth_map::map_auth;
use crate::parse::{SrcNode, is_true, kv_list, str_at, text};

/// Conversion switches (a subset of the plan options).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConvertOptions {
    pub convert_auth: bool,
    pub keep_scripts: bool,
    pub inline_parent_scripts: bool,
}

impl Default for ConvertOptions {
    fn default() -> Self {
        Self {
            convert_auth: true,
            keep_scripts: true,
            inline_parent_scripts: true,
        }
    }
}

struct Warn<'a> {
    id: &'a str,
    name: &'a str,
    out: &'a mut Vec<ImportWarning>,
}

impl Warn<'_> {
    fn push(&mut self, code: &str, detail: impl AsRef<str>) {
        let message = format!("{}: {}", self.name, detail.as_ref());
        if !self.out.iter().any(|w| {
            w.source_id.as_deref() == Some(self.id) && w.code == code && w.message == message
        }) {
            self.out
                .push(ImportWarning::new(Some(self.id), code, message));
        }
    }
}

/// Method of a Postman request; unknown methods become GET (second value false).
pub(crate) fn method_of(request: &Value) -> (HttpMethod, bool) {
    let m = str_at(request, "method")
        .unwrap_or_else(|| "GET".into())
        .to_ascii_uppercase();
    match m.as_str() {
        "GET" => (HttpMethod::Get, true),
        "POST" => (HttpMethod::Post, true),
        "PUT" => (HttpMethod::Put, true),
        "PATCH" => (HttpMethod::Patch, true),
        "DELETE" => (HttpMethod::Delete, true),
        "HEAD" => (HttpMethod::Head, true),
        "OPTIONS" => (HttpMethod::Options, true),
        _ => (HttpMethod::Get, false),
    }
}

/// The URL as Postman shows it (for the preview).
pub(crate) fn display_url(request: &Value) -> String {
    match request.get("url") {
        Some(Value::String(s)) => s.clone(),
        Some(u @ Value::Object(_)) => str_at(u, "raw").unwrap_or_else(|| rebuild_base(u)),
        _ => String::new(),
    }
}

fn join_parts(v: Option<&Value>, sep: &str) -> String {
    match v {
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|p| text(p).or_else(|| str_at(p, "value")))
            .collect::<Vec<_>>()
            .join(sep),
        Some(v) => text(v).unwrap_or_default(),
        None => String::new(),
    }
}

/// `protocol://host:port/path` from URL parts (no query).
fn rebuild_base(u: &Value) -> String {
    let mut s = String::new();
    if let Some(p) = str_at(u, "protocol").filter(|p| !p.is_empty()) {
        s.push_str(&p);
        s.push_str("://");
    }
    s.push_str(&join_parts(u.get("host"), "."));
    if let Some(port) = str_at(u, "port").filter(|p| !p.is_empty()) {
        s.push(':');
        s.push_str(&port);
    }
    let path = join_parts(u.get("path"), "/");
    if !path.is_empty() {
        if !path.starts_with('/') {
            s.push('/');
        }
        s.push_str(&path);
    }
    s
}

/// Split `a=1&b=2` (no decoding; values keep their `{{vars}}`).
fn parse_query(q: &str) -> Vec<(String, String, bool)> {
    q.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (k.to_string(), v.to_string(), true)
        })
        .collect()
}

/// `(url without query, query params, path variables substituted)`.
fn convert_url(request: &Value, warn: &mut Warn<'_>) -> (String, IndexMap<String, String>) {
    let (mut base, params, vars) = match request.get("url") {
        Some(Value::String(s)) => {
            let (b, q) = s.split_once('?').unwrap_or((s, ""));
            (b.to_string(), parse_query(q), Vec::new())
        }
        Some(u @ Value::Object(_)) => {
            let raw = str_at(u, "raw");
            let base = match &raw {
                Some(r) => r.split(['?', '#']).next().unwrap_or("").to_string(),
                None => rebuild_base(u),
            };
            let params = match u.get("query") {
                Some(q @ Value::Array(_)) => kv_list(Some(q)),
                _ => raw
                    .as_deref()
                    .and_then(|r| r.split_once('?'))
                    .map(|(_, q)| parse_query(q.split('#').next().unwrap_or("")))
                    .unwrap_or_default(),
            };
            (base, params, kv_list(u.get("variable")))
        }
        _ => (String::new(), Vec::new(), Vec::new()),
    };
    let mut query = IndexMap::new();
    for (k, v, enabled) in params {
        if enabled {
            query.insert(k, v);
        } else {
            warn.push(
                "disabled_query",
                format!("disabled query parameter \"{k}\" was dropped"),
            );
        }
    }
    if !vars.is_empty() || base.split('/').any(|s| s.starts_with(':') && s.len() > 1) {
        let mut replaced = Vec::new();
        let mut missing = Vec::new();
        base = base
            .split('/')
            .map(|seg| {
                let Some(key) = seg.strip_prefix(':').filter(|k| !k.is_empty()) else {
                    return seg.to_string();
                };
                match vars.iter().find(|(k, v, _)| k == key && !v.is_empty()) {
                    Some((_, v, _)) => {
                        replaced.push(format!(":{key}"));
                        v.clone()
                    }
                    None => {
                        missing.push(format!(":{key}"));
                        seg.to_string()
                    }
                }
            })
            .collect::<Vec<_>>()
            .join("/");
        if !replaced.is_empty() {
            warn.push(
                "path_variable",
                format!(
                    "path variables {} were replaced by their values",
                    replaced.join(", ")
                ),
            );
        }
        if !missing.is_empty() {
            warn.push(
                "path_variable",
                format!(
                    "path variables {} have no value and were kept as is",
                    missing.join(", ")
                ),
            );
        }
    }
    (base, query)
}

fn convert_body(
    body: &Value,
    headers: &IndexMap<String, String>,
    warn: &mut Warn<'_>,
) -> Option<Body> {
    if is_true(body, "disabled") {
        return None;
    }
    let mode = str_at(body, "mode").unwrap_or_default();
    let form = |list: Vec<(String, String, bool)>, warn: &mut Warn<'_>| {
        let mut fields = IndexMap::new();
        for (k, v, enabled) in list {
            if enabled {
                fields.insert(k, v);
            } else {
                warn.push(
                    "disabled_param",
                    format!("disabled body field \"{k}\" was dropped"),
                );
            }
        }
        fields
    };
    match mode.as_str() {
        "raw" => {
            let content = str_at(body, "raw").unwrap_or_default();
            let lang = body
                .get("options")
                .and_then(|o| o.get("raw"))
                .and_then(|r| str_at(r, "language"));
            let json_header = headers
                .iter()
                .any(|(k, v)| k.eq_ignore_ascii_case("content-type") && v.contains("json"));
            if lang.as_deref() == Some("json") || (lang.is_none() && json_header) {
                Some(Body::Json { content })
            } else {
                Some(Body::Text { content })
            }
        }
        "urlencoded" => Some(Body::Form {
            fields: form(kv_list(body.get("urlencoded")), warn),
        }),
        "formdata" => {
            let Some(Value::Array(items)) = body.get("formdata") else {
                return Some(Body::Form {
                    fields: IndexMap::new(),
                });
            };
            let (files, texts): (Vec<&Value>, Vec<&Value>) = items
                .iter()
                .partition(|i| str_at(i, "type").as_deref() == Some("file"));
            for f in files {
                warn.push(
                    "body_file",
                    format!(
                        "file field \"{}\" was dropped (file uploads are not supported yet)",
                        str_at(f, "key").unwrap_or_default()
                    ),
                );
            }
            warn.push(
                "body_formdata",
                "multipart form-data was imported as a urlencoded form (multipart is not supported yet)",
            );
            let list = texts
                .into_iter()
                .filter_map(|i| {
                    Some((
                        str_at(i, "key")?,
                        str_at(i, "value").unwrap_or_default(),
                        !is_true(i, "disabled"),
                    ))
                })
                .collect();
            Some(Body::Form {
                fields: form(list, warn),
            })
        }
        "graphql" => {
            let g = body.get("graphql").cloned().unwrap_or(Value::Null);
            let variables = match g.get("variables") {
                Some(Value::String(s)) if !s.trim().is_empty() => {
                    serde_json::from_str(s).unwrap_or_else(|_| Value::String(s.clone()))
                }
                Some(v @ Value::Object(_)) => v.clone(),
                _ => Value::Object(Default::default()),
            };
            let doc = serde_json::json!({
                "query": str_at(&g, "query").unwrap_or_default(),
                "variables": variables,
            });
            warn.push(
                "body_graphql",
                "GraphQL body was converted to a JSON body ({query, variables})",
            );
            Some(Body::Json {
                content: serde_json::to_string_pretty(&doc).unwrap_or_default(),
            })
        }
        "file" => {
            warn.push(
                "body_file",
                "binary file body was dropped (file uploads are not supported yet)",
            );
            None
        }
        "" => None,
        other => {
            warn.push(
                "body_unsupported",
                format!("body mode \"{other}\" is not supported"),
            );
            None
        }
    }
}

fn join_scripts(parts: Vec<String>) -> Option<String> {
    let parts: Vec<String> = parts.into_iter().filter(|p| !p.trim().is_empty()).collect();
    (!parts.is_empty()).then(|| parts.join("\n\n"))
}

/// Convert one request node. Warnings are appended to `out`.
/// `use_effective`: give the request its inherited Postman auth (when it is
/// placed without the ancestors it inherited from) instead of its own.
pub(crate) fn convert(
    node: &SrcNode,
    opts: &ConvertOptions,
    use_effective: bool,
    out: &mut Vec<ImportWarning>,
) -> Request {
    let mut warn = Warn {
        id: &node.id,
        name: &node.name,
        out,
    };
    let empty = Value::Null;
    let req = node.request.as_ref().unwrap_or(&empty);
    let (method, known) = method_of(req);
    if !known {
        warn.push(
            "method",
            format!(
                "method {} is not supported; imported as GET",
                str_at(req, "method").unwrap_or_default()
            ),
        );
    }
    let (url, query) = convert_url(req, &mut warn);

    let mut headers = IndexMap::new();
    let header_list = match req.get("header") {
        Some(Value::String(s)) => s
            .lines()
            .filter_map(|l| l.split_once(':'))
            .map(|(k, v)| (k.trim().to_string(), v.trim().to_string(), true))
            .collect(),
        h => kv_list(h),
    };
    for (k, v, enabled) in header_list {
        if enabled {
            headers.insert(k, v);
        } else {
            warn.push(
                "disabled_header",
                format!("disabled header \"{k}\" was dropped"),
            );
        }
    }

    let body = req
        .get("body")
        .and_then(|b| convert_body(b, &headers, &mut warn));

    let auth = if opts.convert_auth {
        let source = if use_effective {
            node.effective_auth.as_ref()
        } else {
            node.auth.as_ref()
        };
        source
            .map(|a| map_auth(a, &mut |code, msg| warn.push(code, msg)))
            .unwrap_or_default()
    } else {
        Auth::Inherit
    };

    let mut pre = Vec::new();
    let mut tests = Vec::new();
    if opts.keep_scripts {
        let parent_scripts: Vec<_> = node
            .parents
            .iter()
            .filter(|p| p.pre_request.is_some() || p.tests.is_some())
            .collect();
        if opts.inline_parent_scripts {
            for p in &parent_scripts {
                if let Some(s) = &p.pre_request {
                    pre.push(format!("// From {}\n{s}", p.label));
                }
                if let Some(s) = &p.tests {
                    tests.push(format!("// From {}\n{s}", p.label));
                }
            }
        } else if !parent_scripts.is_empty() {
            let labels: Vec<_> = parent_scripts.iter().map(|p| p.label.as_str()).collect();
            warn.push(
                "parent_script",
                format!(
                    "scripts of {} were not copied into this request",
                    labels.join(", ")
                ),
            );
        }
        pre.extend(node.pre_request.clone());
        tests.extend(node.tests.clone());
    }

    Request {
        name: node.name.clone(),
        method,
        url,
        headers,
        query,
        body,
        scripts: Scripts {
            pre_request: join_scripts(pre),
            tests: join_scripts(tests),
        },
        settings: Default::default(),
        auth,
        extract: Default::default(),
    }
}
