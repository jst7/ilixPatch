//! OpenAPI 3.0 / 3.1 and Swagger 2.0 → a source tree (D53).
//!
//! The spec is translated into the same [`SrcNode`] tree the Postman importer
//! builds, with Postman-shaped `request` / `auth` JSON, so preview, placement,
//! auth conversion, secrets and writing are shared with it:
//!
//! - the document is a collection with a `baseUrl` variable (first server);
//! - each operation is a request named after its `summary` / `operationId`;
//! - folders come from tags or from the URL path ([`GroupBy`]);
//! - parameters and bodies get their examples (generated from the schema
//!   when there are none), security schemes become auth with `{{variables}}`.

mod doc;
mod example;

use std::collections::HashSet;

use ilixpatch_core::environments::EnvVariable;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use doc::{NULL, RefError, deref};
pub(crate) use doc::{Node, from_json, from_yaml, is_openapi};

use crate::parse::SrcNode;
use crate::{ImportWarning, NodeKind, stable_id};

/// How operations are grouped into folders.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GroupBy {
    /// One folder per (first) tag; untagged operations at the collection root.
    #[default]
    Tags,
    /// One folder per URL path segment.
    Path,
}

const METHODS: [&str; 8] = [
    "get", "put", "post", "delete", "options", "head", "patch", "trace",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Version {
    Swagger2,
    OpenApi3,
}

struct Ctx<'a> {
    root: &'a Node,
    file: &'a str,
    version: Version,
    warnings: Vec<ImportWarning>,
    /// Collection variables (`baseUrl`).
    variables: IndexMap<String, String>,
}

impl<'a> Ctx<'a> {
    fn warn(&mut self, id: &str, code: &str, message: String) {
        if !self
            .warnings
            .iter()
            .any(|w| w.code == code && w.message == message)
        {
            self.warnings
                .push(ImportWarning::new(Some(id), code, message));
        }
    }

    fn deref<'n>(&mut self, id: &str, n: &'n Node) -> Option<&'n Node>
    where
        'a: 'n,
    {
        match deref(self.root, n) {
            Ok(n) => Some(n),
            Err(e) => {
                let msg = match e {
                    RefError::External(r) => {
                        format!("reference {r} points to another file and was skipped")
                    }
                    RefError::Missing(r) => format!("reference {r} was not found"),
                    RefError::Loop(r) => format!("reference {r} loops"),
                };
                self.warn(id, "openapi_ref", msg);
                None
            }
        }
    }
}

/// `{{key}}` for an auth field. Deliberately not created as a collection
/// variable: collection.toml is committed, and credentials belong in an
/// environment (secrets go to its git-ignored `.secret.toml`).
fn var(key: &str) -> String {
    format!("{{{{{key}}}}}")
}

/// The result of converting one spec.
pub(crate) struct Converted {
    pub collection: SrcNode,
    pub warnings: Vec<ImportWarning>,
}

/// Convert an OpenAPI / Swagger document. `source_url` is where it was
/// downloaded from (relative server URLs are resolved against it).
pub(crate) fn convert(
    root: &Node,
    file: &str,
    source_url: Option<&str>,
    group_by: GroupBy,
) -> Result<Converted, String> {
    let version = if let Some(v) = root.get("openapi") {
        let v = v.scalar_text().unwrap_or_default();
        if !v.starts_with("3.") {
            return Err(format!(
                "OpenAPI {v} is not supported (OpenAPI 3.0, 3.1 and Swagger 2.0 are)"
            ));
        }
        Version::OpenApi3
    } else {
        let v = root
            .get("swagger")
            .and_then(Node::scalar_text)
            .unwrap_or_default();
        if v != "2.0" {
            return Err(format!(
                "Swagger {v} is not supported (OpenAPI 3.0, 3.1 and Swagger 2.0 are)"
            ));
        }
        Version::Swagger2
    };
    let collection_id = stable_id(&format!("{file}#"));
    let info = root.get("info").unwrap_or(&NULL);
    let name = info.text("title").unwrap_or("Imported API").to_string();
    let mut ctx = Ctx {
        root,
        file,
        version,
        warnings: Vec::new(),
        variables: IndexMap::new(),
    };

    let base = base_url(&mut ctx, &collection_id, source_url);
    ctx.variables.insert("baseUrl".into(), base);

    // Security: the document-wide requirement is the collection's auth.
    let global_req = root.get("security").map(requirement);
    let collection_auth = match &global_req {
        Some(r) => auth_for(&mut ctx, &collection_id, r),
        None => None,
    };

    // Operations, in document order.
    let mut ops: Vec<(Option<String>, String, SrcNode)> = Vec::new(); // (tag, path, request)
    let paths: Vec<(&String, &Node)> = root
        .get("paths")
        .map(|p| p.entries().collect())
        .unwrap_or_default();
    if paths.is_empty() {
        ctx.warn(
            &collection_id,
            "openapi_no_paths",
            format!("\"{name}\" has no operations"),
        );
    }
    for (path, item) in paths {
        let Some(item) = ctx.deref(&collection_id, item) else {
            continue;
        };
        let shared_params = item.get("parameters").map(Node::arr).unwrap_or_default();
        for method in METHODS {
            let Some(op) = item.get(method) else { continue };
            let id = stable_id(&format!("{file}#{method} {path}"));
            if method == "trace" {
                // On the collection: the skipped request has no node of its own.
                ctx.warn(
                    &collection_id,
                    "openapi_method",
                    format!("TRACE {path}: the TRACE method is not supported; skipped"),
                );
                continue;
            }
            let req = operation(&mut ctx, &id, method, path, op, shared_params);
            // Auth: inherit when it is the document's requirement, else its own.
            let (auth, effective_auth) = match op.get("security").map(requirement) {
                Some(r) if Some(&r) != global_req.as_ref() => {
                    let a = auth_for(&mut ctx, &id, &r);
                    (a.clone(), a)
                }
                _ => (None, collection_auth.clone()),
            };
            let name = op
                .text("summary")
                .or_else(|| op.text("operationId"))
                .map(str::to_string)
                .unwrap_or_else(|| format!("{} {path}", method.to_uppercase()));
            let tag = op
                .get("tags")
                .map(Node::arr)
                .and_then(|t| t.first())
                .and_then(Node::str)
                .map(str::to_string);
            ops.push((
                tag,
                path.clone(),
                SrcNode {
                    id,
                    kind: NodeKind::Request,
                    name,
                    auth,
                    effective_auth,
                    pre_request: None,
                    tests: None,
                    parents: Vec::new(),
                    has_description: op.get("description").is_some(),
                    variables: Vec::new(),
                    request: Some(req),
                    children: Vec::new(),
                },
            ));
        }
    }

    let children = match group_by {
        GroupBy::Tags => group_by_tags(&ctx, ops),
        GroupBy::Path => group_by_path(file, ops),
    };
    let variables = ctx
        .variables
        .iter()
        .map(|(k, v)| EnvVariable::new(k.clone(), v.clone()))
        .collect();
    let has_description = info.get("description").is_some();
    Ok(Converted {
        collection: SrcNode {
            id: collection_id,
            kind: NodeKind::Collection,
            name,
            auth: collection_auth.clone(),
            effective_auth: collection_auth,
            pre_request: None,
            tests: None,
            parents: Vec::new(),
            has_description,
            variables,
            request: None,
            children,
        },
        warnings: ctx.warnings,
    })
}

// ---------- servers ----------

/// `scheme://host[:port]` of a URL.
fn origin(url: &str) -> Option<&str> {
    let after = url.find("://")? + 3;
    let end = url[after..]
        .find(['/', '?', '#'])
        .map_or(url.len(), |i| after + i);
    Some(&url[..end])
}

/// Resolve a relative server URL against the URL the spec came from.
fn resolve(url: &str, source: &str) -> String {
    if url.contains("://") {
        return url.to_string();
    }
    if let Some(rest) = url.strip_prefix("//") {
        let scheme = source.split("://").next().unwrap_or("https");
        return format!("{scheme}://{rest}");
    }
    let Some(o) = origin(source) else {
        return url.to_string();
    };
    if url.starts_with('/') {
        return format!("{o}{url}");
    }
    let path = source[o.len()..].split(['?', '#']).next().unwrap_or("");
    let dir = path.rfind('/').map_or("/", |i| &path[..=i]);
    format!("{o}{dir}{}", url.trim_start_matches("./"))
}

fn base_url(ctx: &mut Ctx<'_>, id: &str, source: Option<&str>) -> String {
    let root = ctx.root;
    let url = match ctx.version {
        Version::OpenApi3 => {
            let servers = root.get("servers").map(Node::arr).unwrap_or_default();
            let others: Vec<String> = servers
                .iter()
                .skip(1)
                .filter_map(|s| s.text("url").map(str::to_string))
                .collect();
            if !others.is_empty() {
                ctx.warn(
                    id,
                    "openapi_servers",
                    format!(
                        "baseUrl is the first server; the others are not imported: {}",
                        others.join(", ")
                    ),
                );
            }
            match servers.first() {
                Some(s) => {
                    let mut url = s.text("url").unwrap_or("/").to_string();
                    for (var, def) in s.get("variables").into_iter().flat_map(Node::entries) {
                        if let Some(d) = def.get("default").and_then(Node::scalar_text) {
                            url = url.replace(&format!("{{{var}}}"), &d);
                        }
                    }
                    url
                }
                None => "/".into(),
            }
        }
        Version::Swagger2 => {
            let schemes: Vec<&str> = root
                .get("schemes")
                .map(Node::arr)
                .unwrap_or_default()
                .iter()
                .filter_map(Node::str)
                .collect();
            let base_path = root.text("basePath").unwrap_or("");
            match root.text("host") {
                Some(host) => {
                    let scheme = if schemes.is_empty() || schemes.contains(&"https") {
                        "https"
                    } else {
                        schemes[0]
                    };
                    format!("{scheme}://{host}{base_path}")
                }
                None => format!("/{}", base_path.trim_start_matches('/')),
            }
        }
    };
    let url = match source {
        Some(src) => resolve(&url, src),
        None => url,
    };
    if !url.contains("://") {
        ctx.warn(
            id,
            "openapi_relative_server",
            format!(
                "the server URL \"{url}\" is relative: set the baseUrl collection variable to the API's address"
            ),
        );
    }
    url.trim_end_matches('/').to_string()
}

// ---------- operations ----------

/// A parameter value given by the spec (not a generated placeholder).
fn given_value(root: &Node, p: &Node) -> Option<String> {
    let schema = p.get("schema").map(|s| deref(root, s).unwrap_or(s));
    let explicit = p
        .get("example")
        .or_else(|| p.get("x-example"))
        .cloned()
        .or_else(|| {
            let (_, first) = p.get("examples")?.entries().next()?;
            deref(root, first).ok()?.get("value").cloned()
        })
        .or_else(|| {
            // Swagger 2.0 keeps default/enum on the parameter itself.
            [Some(p), schema]
                .into_iter()
                .flatten()
                .find_map(example::explicit)
        })
        .or_else(|| {
            // Arrays of enums: the first item value.
            let items = schema.unwrap_or(p).get("items")?;
            let items = deref(root, items).unwrap_or(items);
            example::explicit(items).map(|v| Node::Arr(vec![v]))
        })?;
    explicit.param_text()
}

fn header_list(h: &IndexMap<String, String>) -> Value {
    Value::Array(
        h.iter()
            .map(|(k, v)| json!({ "key": k, "value": v }))
            .collect(),
    )
}

fn operation(
    ctx: &mut Ctx<'_>,
    id: &str,
    method: &str,
    path: &str,
    op: &Node,
    shared: &[Node],
) -> Value {
    let root = ctx.root;
    // Parameters: path-level, overridden by operation-level (same name + in).
    let mut params: IndexMap<(String, String), &Node> = IndexMap::new();
    for p in shared
        .iter()
        .chain(op.get("parameters").map(Node::arr).unwrap_or_default())
    {
        let Some(p) = ctx.deref(id, p) else { continue };
        let (Some(name), Some(loc)) = (p.text("name"), p.text("in")) else {
            continue;
        };
        params.insert((name.to_string(), loc.to_string()), p);
    }

    let mut url_path = path.to_string();
    let mut query: Vec<(String, String)> = Vec::new();
    let mut headers: IndexMap<String, String> = IndexMap::new();
    let mut cookies: Vec<String> = Vec::new();
    let mut form: Vec<(&Node, String)> = Vec::new(); // Swagger 2.0 formData
    let mut body_param: Option<&Node> = None; // Swagger 2.0 in: body
    for ((name, loc), p) in &params {
        let required = p.is_true("required") || loc == "path";
        let value = given_value(root, p);
        match loc.as_str() {
            "path" => {
                let v = value.unwrap_or_else(|| format!("{{{{{name}}}}}"));
                url_path = url_path.replace(&format!("{{{name}}}"), &v);
            }
            "query" | "header" | "cookie" => {
                let v = match (value, required) {
                    (Some(v), _) => v,
                    (None, true) => format!("{{{{{name}}}}}"),
                    (None, false) => continue,
                };
                match loc.as_str() {
                    "query" => query.push((name.clone(), v)),
                    "header" => {
                        // OpenAPI: these three are described elsewhere.
                        if !["accept", "content-type", "authorization"]
                            .contains(&name.to_ascii_lowercase().as_str())
                        {
                            headers.insert(name.clone(), v);
                        }
                    }
                    _ => cookies.push(format!("{name}={v}")),
                }
            }
            // Swagger 2.0: every form field is listed (empty when it has no value).
            "formData" => form.push((*p, value.unwrap_or_default())),
            "body" => body_param = Some(*p),
            _ => {}
        }
    }
    if !cookies.is_empty() {
        headers.insert("Cookie".into(), cookies.join("; "));
    }
    if let Some(accept) = accept(ctx, op) {
        headers.insert("Accept".into(), accept);
    }

    let body = match ctx.version {
        Version::OpenApi3 => op
            .get("requestBody")
            .and_then(|b| ctx.deref(id, b))
            .and_then(|b| body_v3(ctx, b, &mut headers)),
        Version::Swagger2 => body_v2(ctx, op, body_param, &form, &mut headers),
    };

    let mut raw = format!("{{{{baseUrl}}}}{url_path}");
    if !query.is_empty() {
        let qs: Vec<String> = query.iter().map(|(k, v)| format!("{k}={v}")).collect();
        raw = format!("{raw}?{}", qs.join("&"));
    }
    let mut req = json!({
        "method": method.to_uppercase(),
        "header": header_list(&headers),
        "url": {
            "raw": raw,
            "query": query.iter().map(|(k, v)| json!({ "key": k, "value": v })).collect::<Vec<_>>(),
        },
    });
    if let Some(b) = body {
        req["body"] = b;
    }
    req
}

/// `Accept` from the first successful response's media type.
fn accept(ctx: &Ctx<'_>, op: &Node) -> Option<String> {
    let media = match ctx.version {
        Version::OpenApi3 => {
            let responses = op.get("responses")?;
            let (_, resp) = responses
                .entries()
                .find(|(code, _)| code.starts_with('2'))
                .or_else(|| responses.entries().find(|(code, _)| *code == "default"))?;
            let resp = deref(ctx.root, resp).ok()?;
            resp.get("content")?.obj()?.keys().next()?.clone()
        }
        Version::Swagger2 => op
            .get("produces")
            .or_else(|| ctx.root.get("produces"))?
            .arr()
            .first()?
            .str()?
            .to_string(),
    };
    (media != "*/*").then_some(media)
}

fn is_json(media: &str) -> bool {
    let m = media.split(';').next().unwrap_or("").trim();
    m == "application/json" || m.ends_with("+json") || m.contains("/json")
}

/// Pick the media type to import: JSON, then forms, then anything.
fn pick_media(content: &IndexMap<String, Node>) -> Option<(&String, &Node)> {
    content
        .iter()
        .find(|(m, _)| is_json(m))
        .or_else(|| {
            content
                .iter()
                .find(|(m, _)| m.starts_with("application/x-www-form-urlencoded"))
        })
        .or_else(|| content.iter().find(|(m, _)| m.starts_with("multipart/")))
        .or_else(|| content.iter().next())
}

fn json_body(text: String) -> Value {
    json!({ "mode": "raw", "raw": text, "options": { "raw": { "language": "json" } } })
}

/// Form fields of an example object; `files` are fields uploaded as files.
fn form_body(mode: &str, example: Option<Node>, files: &HashSet<String>) -> Value {
    let fields: Vec<Value> = match example {
        Some(Node::Obj(m)) => m
            .iter()
            .map(|(k, v)| {
                if files.contains(k) {
                    json!({ "key": k, "type": "file" })
                } else {
                    json!({ "key": k, "value": v.param_text().unwrap_or_default(), "type": "text" })
                }
            })
            .collect(),
        _ => Vec::new(),
    };
    json!({ "mode": mode, mode: fields })
}

/// Properties uploaded as files (`format: binary`, or a 3.1 `contentMediaType`).
fn file_props(root: &Node, schema: &Node) -> HashSet<String> {
    let schema = deref(root, schema).unwrap_or(schema);
    schema
        .get("properties")
        .into_iter()
        .flat_map(Node::entries)
        .filter(|(_, p)| {
            let p = deref(root, p).unwrap_or(p);
            let p = match p.s("type") {
                Some("array") => p.get("items").map_or(p, |i| deref(root, i).unwrap_or(i)),
                _ => p,
            };
            p.s("format") == Some("binary") || p.get("contentMediaType").is_some()
        })
        .map(|(k, _)| k.clone())
        .collect()
}

fn body_v3(ctx: &Ctx<'_>, body: &Node, headers: &mut IndexMap<String, String>) -> Option<Value> {
    let root = ctx.root;
    let (media, m) = pick_media(body.get("content")?.obj()?)?;
    let example = example::of_holder(root, m);
    let base = media.split(';').next().unwrap_or("").trim();
    if is_json(media) {
        if base != "application/json" {
            headers.insert("Content-Type".into(), media.clone());
        }
        return Some(json_body(
            example.map(|e| e.to_pretty_json()).unwrap_or_default(),
        ));
    }
    let files = m
        .get("schema")
        .map(|s| file_props(root, s))
        .unwrap_or_default();
    match base {
        "application/x-www-form-urlencoded" => Some(form_body("urlencoded", example, &files)),
        "multipart/form-data" => Some(form_body("formdata", example, &files)),
        b if b.starts_with("text/") || b.ends_with("xml") || b.ends_with("+xml") => {
            headers.insert("Content-Type".into(), media.clone());
            let text = match example {
                Some(Node::Str(s)) => s,
                _ => String::new(),
            };
            Some(json!({ "mode": "raw", "raw": text }))
        }
        _ => Some(json!({ "mode": "file" })),
    }
}

fn body_v2(
    ctx: &Ctx<'_>,
    op: &Node,
    body_param: Option<&Node>,
    form: &[(&Node, String)],
    headers: &mut IndexMap<String, String>,
) -> Option<Value> {
    let root = ctx.root;
    let consumes: Vec<&str> = op
        .get("consumes")
        .or_else(|| root.get("consumes"))
        .map(Node::arr)
        .unwrap_or_default()
        .iter()
        .filter_map(Node::str)
        .collect();
    if let Some(p) = body_param {
        let media = consumes
            .iter()
            .find(|m| is_json(m))
            .or(consumes.first())
            .copied()
            .unwrap_or("application/json");
        let example = p.get("x-example").cloned().or_else(|| {
            p.get("schema")
                .and_then(|s| example::Gen::new(root).example(s))
        });
        if is_json(media) {
            if media != "application/json" {
                headers.insert("Content-Type".into(), media.to_string());
            }
            return Some(json_body(
                example.map(|e| e.to_pretty_json()).unwrap_or_default(),
            ));
        }
        headers.insert("Content-Type".into(), media.to_string());
        let text = match example {
            Some(Node::Str(s)) => s,
            _ => String::new(),
        };
        return Some(json!({ "mode": "raw", "raw": text }));
    }
    if form.is_empty() {
        return None;
    }
    let files: HashSet<String> = form
        .iter()
        .filter(|(p, _)| p.s("type") == Some("file"))
        .filter_map(|(p, _)| p.text("name").map(str::to_string))
        .collect();
    let multipart = !files.is_empty() || consumes.iter().any(|m| m.starts_with("multipart/"));
    let obj = Node::Obj(
        form.iter()
            .filter_map(|(p, v)| Some((p.text("name")?.to_string(), Node::Str(v.clone()))))
            .collect(),
    );
    Some(form_body(
        if multipart { "formdata" } else { "urlencoded" },
        Some(obj),
        &files,
    ))
}

// ---------- security ----------

/// The chosen alternative of a security requirement list.
#[derive(Debug, Clone, PartialEq)]
enum Requirement {
    /// `security: []` or an optional `{}` alternative.
    None,
    /// First scheme of the first alternative, with its scopes.
    Scheme(String, Vec<String>),
}

fn requirement(sec: &Node) -> Requirement {
    let Some(first) = sec.arr().first() else {
        return Requirement::None;
    };
    match first.entries().next() {
        Some((name, scopes)) => Requirement::Scheme(
            name.clone(),
            scopes
                .arr()
                .iter()
                .filter_map(|s| s.str().map(str::to_string))
                .collect(),
        ),
        None => Requirement::None,
    }
}

fn auth_for(ctx: &mut Ctx<'_>, id: &str, req: &Requirement) -> Option<Value> {
    let Requirement::Scheme(name, scopes) = req else {
        return Some(json!({ "type": "noauth" }));
    };
    let root = ctx.root;
    let schemes = match ctx.version {
        Version::OpenApi3 => root
            .get("components")
            .and_then(|c| c.get("securitySchemes")),
        Version::Swagger2 => root.get("securityDefinitions"),
    };
    let Some(scheme) = schemes.and_then(|s| s.get(name)) else {
        ctx.warn(
            id,
            "openapi_auth",
            format!("security scheme \"{name}\" is not defined"),
        );
        return None;
    };
    let scheme = ctx.deref(id, scheme)?;
    let typ = scheme.s("type").unwrap_or_default();
    let http_scheme = scheme.s("scheme").unwrap_or_default().to_ascii_lowercase();
    let out = match (typ, http_scheme.as_str()) {
        ("http", "bearer") => {
            json!({ "type": "bearer", "bearer": { "token": var("bearerToken") } })
        }
        ("http", "basic") | ("basic", _) => json!({ "type": "basic", "basic": {
            "username": var("basicUsername"), "password": var("basicPassword") } }),
        ("http", "digest") => json!({ "type": "digest", "digest": {
            "username": var("digestUsername"), "password": var("digestPassword") } }),
        ("apiKey", _) => {
            let loc = scheme.s("in").unwrap_or("header");
            if loc == "cookie" {
                ctx.warn(
                    id,
                    "openapi_auth",
                    format!("API key \"{name}\" goes in a cookie: add the Cookie header by hand"),
                );
                return None;
            }
            let key = scheme.text("name").unwrap_or(name);
            json!({ "type": "apikey", "apikey": {
                "key": key, "value": var(name), "in": loc } })
        }
        ("oauth2", _) => oauth2(ctx, scheme, scopes),
        _ => {
            let what = if typ == "http" {
                format!("HTTP \"{http_scheme}\"")
            } else {
                format!("\"{typ}\"")
            };
            ctx.warn(
                id,
                "openapi_auth",
                format!(
                    "security scheme \"{name}\" ({what}) is not supported: set up the auth by hand"
                ),
            );
            return None;
        }
    };
    Some(out)
}

fn oauth2(ctx: &mut Ctx<'_>, scheme: &Node, scopes: &[String]) -> Value {
    // (grant, flow) in order of preference: non-interactive grants work in the CLI too.
    let (grant, flow) = match ctx.version {
        Version::OpenApi3 => {
            let flows = scheme.get("flows").unwrap_or(&NULL);
            [
                ("client_credentials", "clientCredentials"),
                ("authorization_code", "authorizationCode"),
                ("password_credentials", "password"),
                ("implicit", "implicit"),
            ]
            .into_iter()
            .find_map(|(g, f)| flows.get(f).map(|n| (g, n)))
            .unwrap_or(("authorization_code", &NULL))
        }
        Version::Swagger2 => {
            let g = match scheme.s("flow").unwrap_or_default() {
                "application" => "client_credentials",
                "password" => "password_credentials",
                "implicit" => "implicit",
                _ => "authorization_code",
            };
            (g, scheme)
        }
    };
    let mut p = serde_json::Map::new();
    p.insert("grant_type".into(), json!(grant));
    if let Some(u) = flow.text("authorizationUrl") {
        p.insert("authUrl".into(), json!(u));
    }
    if let Some(u) = flow.text("tokenUrl") {
        p.insert("accessTokenUrl".into(), json!(u));
    }
    p.insert("clientId".into(), json!(var("clientId")));
    if grant != "implicit" {
        p.insert("clientSecret".into(), json!(var("clientSecret")));
    }
    if !scopes.is_empty() {
        p.insert("scope".into(), json!(scopes.join(" ")));
    }
    json!({ "type": "oauth2", "oauth2": Value::Object(p) })
}

// ---------- grouping ----------

fn folder(id: String, name: String, children: Vec<SrcNode>) -> SrcNode {
    SrcNode {
        id,
        kind: NodeKind::Folder,
        name,
        auth: None,
        effective_auth: None,
        pre_request: None,
        tests: None,
        parents: Vec::new(),
        has_description: false,
        variables: Vec::new(),
        request: None,
        children,
    }
}

fn group_by_tags(ctx: &Ctx<'_>, ops: Vec<(Option<String>, String, SrcNode)>) -> Vec<SrcNode> {
    // Declared tag order first, then first use.
    let mut order: Vec<String> = ctx
        .root
        .get("tags")
        .map(Node::arr)
        .unwrap_or_default()
        .iter()
        .filter_map(|t| t.text("name").map(str::to_string))
        .collect();
    for (tag, _, _) in &ops {
        if let Some(t) = tag
            && !order.contains(t)
        {
            order.push(t.clone());
        }
    }
    let mut folders: IndexMap<String, Vec<SrcNode>> =
        order.into_iter().map(|t| (t, Vec::new())).collect();
    let mut loose = Vec::new();
    for (tag, _, node) in ops {
        match tag {
            Some(t) => folders.entry(t).or_default().push(node),
            None => loose.push(node),
        }
    }
    let mut out: Vec<SrcNode> = folders
        .into_iter()
        .filter(|(_, c)| !c.is_empty())
        .map(|(tag, c)| folder(stable_id(&format!("{}#tag/{tag}", ctx.file)), tag, c))
        .collect();
    out.extend(loose);
    out
}

#[derive(Default)]
struct PathTree {
    requests: Vec<SrcNode>,
    folders: IndexMap<String, PathTree>,
}

impl PathTree {
    /// Into nodes: requests first, then sub-folders. A folder with no
    /// requests and a single sub-folder is merged with it (`api/v1`).
    fn into_nodes(self, file: &str, prefix: &str) -> Vec<SrcNode> {
        let mut out = self.requests;
        for (seg, mut sub) in self.folders {
            let mut name = seg;
            while sub.requests.is_empty() && sub.folders.len() == 1 {
                let (next, inner) = sub.folders.pop().expect("one folder");
                name = format!("{name}/{next}");
                sub = inner;
            }
            let path = format!("{prefix}/{name}");
            let id = stable_id(&format!("{file}#path{path}"));
            out.push(folder(id, name, sub.into_nodes(file, &path)));
        }
        out
    }
}

fn group_by_path(file: &str, ops: Vec<(Option<String>, String, SrcNode)>) -> Vec<SrcNode> {
    let mut tree = PathTree::default();
    for (_, path, node) in ops {
        let mut cur = &mut tree;
        for seg in path.split('/').filter(|s| !s.is_empty()) {
            cur = cur.folders.entry(seg.to_string()).or_default();
        }
        cur.requests.push(node);
    }
    tree.into_nodes(file, "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_resolution() {
        assert_eq!(
            resolve(
                "/api/v3",
                "https://petstore3.swagger.io/api/v3/openapi.json"
            ),
            "https://petstore3.swagger.io/api/v3"
        );
        assert_eq!(
            resolve("v2", "https://h.io/specs/openapi.yaml?x=1"),
            "https://h.io/specs/v2"
        );
        assert_eq!(
            resolve("//cdn.h.io/x", "http://h.io/a"),
            "http://cdn.h.io/x"
        );
        assert_eq!(resolve("https://a.io", "https://h.io/a"), "https://a.io");
        assert_eq!(origin("http://h.io:8080/x?y"), Some("http://h.io:8080"));
    }
}
