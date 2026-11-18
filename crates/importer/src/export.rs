//! ilixPatch → Postman export (D58): collections as Postman Collection v2.1
//! and environments as Postman environment files.
//!
//! It is the inverse of the importer (`convert.rs`, `auth_map.rs`): what is
//! exported imports back into ilixPatch with the same requests, auth and
//! scripts. What Postman has no place for is converted (response variables,
//! D48 → test script) or reported as a warning (timeout, HTTP version).

use std::fs;
use std::path::{Path, PathBuf};

use ilixpatch_core::auth::*;
use ilixpatch_core::environments::{EnvStore, EnvVariable, EnvironmentDoc};
use ilixpatch_core::extract::{PathStep, parse_json_path};
use ilixpatch_core::project::{is_project, load_project, slugify};
use ilixpatch_core::storage::load_collection;
use ilixpatch_core::{
    Body, Collection, Extract, ExtractFrom, ExtractScope, Folder, HttpVersionPref, Request,
};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

pub const POSTMAN_SCHEMA_V21: &str =
    "https://schema.getpostman.com/json/collection/v2.1.0/collection.json";
const EXPORTED_USING: &str = concat!("ilixPatch/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportOptions {
    /// Write the values of secret variables (and OAuth 2.0 access tokens).
    /// Off: secret variables are exported with an empty value.
    #[serde(default)]
    pub include_secrets: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportWarning {
    /// What it is about: `Collection/Folder/Request` or `environment "Dev"`.
    pub item: String,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportReport {
    /// Files written, in order.
    pub files: Vec<PathBuf>,
    pub collections: usize,
    pub requests: usize,
    pub environments: usize,
    /// Secret values written as empty (`include_secrets` off).
    pub secrets_omitted: usize,
    pub warnings: Vec<ExportWarning>,
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ExportError(pub String);

type Result<T> = std::result::Result<T, ExportError>;

fn err(path: &Path, e: impl std::fmt::Display) -> ExportError {
    ExportError(format!("{}: {e}", path.display()))
}

// ---------- entry points ----------

/// Export what `source` is: a project (every collection and the project
/// environments into `out_dir`) or a collection (into `out_dir`).
pub fn export_postman(source: &Path, out_dir: &Path, opts: &ExportOptions) -> Result<ExportReport> {
    fs::create_dir_all(out_dir).map_err(|e| err(out_dir, e))?;
    let mut report = ExportReport::default();
    let mut used = Vec::new();
    if is_project(source) {
        let project = load_project(source).map_err(|e| err(source, e))?;
        for c in &project.collections {
            write_collection(c, out_dir, opts, &mut used, &mut report)?;
        }
        let docs = EnvStore::project(&project.path)
            .list()
            .map_err(|e| err(source, e))?;
        for doc in &docs {
            write_environment(doc, out_dir, opts, &mut used, &mut report)?;
        }
    } else {
        let c = load_collection(source).map_err(|e| err(source, e))?;
        write_collection(&c, out_dir, opts, &mut used, &mut report)?;
    }
    Ok(report)
}

/// Export one collection to `file` (its environments, if it has any, go next to it).
pub fn export_collection_file(
    collection: &Path,
    file: &Path,
    opts: &ExportOptions,
) -> Result<ExportReport> {
    let c = load_collection(collection).map_err(|e| err(collection, e))?;
    let mut report = ExportReport::default();
    let (doc, warnings) = collection_json(&c, opts);
    write_json(file, &doc)?;
    report.files.push(file.to_path_buf());
    report.collections = 1;
    report.requests = c.requests().len();
    report.warnings = warnings;
    let dir = file.parent().unwrap_or(Path::new("."));
    let mut used = vec![file.to_path_buf()];
    for env in &c.environments {
        write_environment(&legacy_env(&c, env), dir, opts, &mut used, &mut report)?;
    }
    Ok(report)
}

fn write_collection(
    c: &Collection,
    dir: &Path,
    opts: &ExportOptions,
    used: &mut Vec<PathBuf>,
    report: &mut ExportReport,
) -> Result<()> {
    let (doc, warnings) = collection_json(c, opts);
    let path = unique_file(dir, &c.name, "postman_collection", used);
    write_json(&path, &doc)?;
    report.files.push(path);
    report.collections += 1;
    report.requests += c.requests().len();
    report.warnings.extend(warnings);
    // Environments kept inside the collection (older projects).
    for env in &c.environments {
        write_environment(&legacy_env(c, env), dir, opts, used, report)?;
    }
    Ok(())
}

/// A collection environment as an environment doc named "<collection> - <env>".
fn legacy_env(c: &Collection, env: &ilixpatch_core::Environment) -> EnvironmentDoc {
    EnvironmentDoc {
        name: format!("{} - {}", c.name, env.name),
        variables: env
            .variables
            .iter()
            .map(|(k, v)| EnvVariable::new(k, v))
            .collect(),
        color: None,
        icon: None,
    }
}

fn write_environment(
    doc: &EnvironmentDoc,
    dir: &Path,
    opts: &ExportOptions,
    used: &mut Vec<PathBuf>,
    report: &mut ExportReport,
) -> Result<()> {
    let (json, omitted) = environment_json(doc, opts);
    let path = unique_file(dir, &doc.name, "postman_environment", used);
    write_json(&path, &json)?;
    report.files.push(path);
    report.environments += 1;
    report.secrets_omitted += omitted;
    Ok(())
}

/// `<dir>/<slug>.<kind>.json`, `-2`, `-3`… when another file of this export took it.
fn unique_file(dir: &Path, name: &str, kind: &str, used: &mut Vec<PathBuf>) -> PathBuf {
    let slug = Some(slugify(name))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "export".into());
    let mut n = 1;
    loop {
        let stem = if n == 1 {
            slug.clone()
        } else {
            format!("{slug}-{n}")
        };
        let path = dir.join(format!("{stem}.{kind}.json"));
        if !used.contains(&path) {
            used.push(path.clone());
            return path;
        }
        n += 1;
    }
}

fn write_json(path: &Path, value: &Value) -> Result<()> {
    let mut text = serde_json::to_string_pretty(value).map_err(|e| err(path, e))?;
    text.push('\n');
    fs::write(path, text).map_err(|e| err(path, e))
}

// ---------- environments ----------

/// A Postman environment. Returns it and how many secret values were left empty.
pub fn environment_json(doc: &EnvironmentDoc, opts: &ExportOptions) -> (Value, usize) {
    let mut omitted = 0;
    let values: Vec<Value> = doc
        .variables
        .iter()
        .map(|v| {
            let value = if v.secret && !opts.include_secrets {
                if !v.value.is_empty() {
                    omitted += 1;
                }
                String::new()
            } else {
                v.value.clone()
            };
            json!({
                "key": v.key,
                "value": value,
                "type": if v.secret { "secret" } else { "default" },
                "enabled": v.enabled,
            })
        })
        .collect();
    (
        json!({
            "name": doc.name,
            "values": values,
            "_postman_variable_scope": "environment",
            "_postman_exported_using": EXPORTED_USING,
        }),
        omitted,
    )
}

// ---------- collections ----------

struct Ctx<'a> {
    opts: &'a ExportOptions,
    warnings: Vec<ExportWarning>,
}

impl Ctx<'_> {
    fn warn(&mut self, item: &str, code: &str, message: impl Into<String>) {
        self.warnings.push(ExportWarning {
            item: item.to_string(),
            code: code.to_string(),
            message: message.into(),
        });
    }
}

/// A Postman Collection v2.1 document, with the warnings of the conversion.
pub fn collection_json(c: &Collection, opts: &ExportOptions) -> (Value, Vec<ExportWarning>) {
    let mut ctx = Ctx {
        opts,
        warnings: Vec::new(),
    };
    let mut doc = Map::new();
    doc.insert(
        "info".into(),
        json!({ "name": c.name, "schema": POSTMAN_SCHEMA_V21, "_exporter_id": EXPORTED_USING }),
    );
    doc.insert(
        "item".into(),
        Value::Array(items(&c.root, &c.name, &mut ctx)),
    );
    // A collection without auth sends none (Inherit = NoAuth at the top).
    if let Some(a) = auth_json(&c.auth, opts.include_secrets).filter(|_| !c.auth.is_inherit()) {
        doc.insert("auth".into(), a);
    }
    if !c.variables.is_empty() {
        doc.insert(
            "variable".into(),
            c.variables
                .iter()
                .map(|(k, v)| json!({ "key": k, "value": v, "type": "string" }))
                .collect(),
        );
    }
    (Value::Object(doc), ctx.warnings)
}

/// Requests first, then folders (the order of the tree).
fn items(folder: &Folder, path: &str, ctx: &mut Ctx<'_>) -> Vec<Value> {
    let mut out = Vec::new();
    for r in &folder.requests {
        out.push(request_item(
            &r.request,
            &format!("{path}/{}", r.request.name),
            ctx,
        ));
    }
    for f in &folder.folders {
        let fpath = format!("{path}/{}", f.name);
        let mut item = Map::new();
        item.insert("name".into(), json!(f.name));
        item.insert("item".into(), Value::Array(items(f, &fpath, ctx)));
        if let Some(a) = auth_json(&f.auth, ctx.opts.include_secrets) {
            item.insert("auth".into(), a);
        }
        out.push(Value::Object(item));
    }
    out
}

fn kv(map: &IndexMap<String, String>) -> Vec<Value> {
    map.iter()
        .map(|(k, v)| json!({ "key": k, "value": v, "type": "text" }))
        .collect()
}

fn request_item(r: &Request, path: &str, ctx: &mut Ctx<'_>) -> Value {
    let mut req = Map::new();
    req.insert("method".into(), json!(r.method.as_str()));
    req.insert("header".into(), Value::Array(kv(&r.headers)));
    if let Some(body) = body_json(r.body.as_ref()) {
        req.insert("body".into(), body);
    }
    req.insert("url".into(), url_json(&r.url, &r.query));
    if let Some(a) = auth_json(&r.auth, ctx.opts.include_secrets) {
        req.insert("auth".into(), a);
    }

    let mut item = Map::new();
    item.insert("name".into(), json!(r.name));

    let mut tests = r.scripts.tests.clone().filter(|s| !s.trim().is_empty());
    if let Some(script) = extract_script(&r.extract) {
        ctx.warn(
            path,
            "extract_script",
            "response variables were converted into a test script",
        );
        tests = Some(match tests {
            Some(t) => format!("{t}\n\n{script}"),
            None => script,
        });
    }
    let events: Vec<Value> = [
        ("prerequest", r.scripts.pre_request.as_deref()),
        ("test", tests.as_deref()),
    ]
    .into_iter()
    .filter_map(|(listen, s)| s.filter(|s| !s.trim().is_empty()).map(|s| (listen, s)))
    .map(|(listen, s)| {
        json!({
            "listen": listen,
            "script": { "type": "text/javascript", "exec": s.lines().collect::<Vec<_>>() },
        })
    })
    .collect();
    if !events.is_empty() {
        item.insert("event".into(), Value::Array(events));
    }

    let s = &r.settings;
    let mut behavior = Map::new();
    if let Some(v) = s.follow_redirects {
        behavior.insert("followRedirects".into(), json!(v));
    }
    if let Some(v) = s.max_redirects {
        behavior.insert("maxRedirects".into(), json!(v));
    }
    if let Some(v) = s.verify_ssl {
        behavior.insert("strictSSL".into(), json!(v));
    }
    if !behavior.is_empty() {
        item.insert("protocolProfileBehavior".into(), Value::Object(behavior));
    }
    if s.timeout_ms.is_some() {
        ctx.warn(
            path,
            "setting_timeout",
            "Postman has no per-request timeout; it was not exported",
        );
    }
    if s.http_version.is_some_and(|v| v != HttpVersionPref::Auto) {
        ctx.warn(
            path,
            "setting_http_version",
            "the HTTP version of the request was not exported",
        );
    }

    item.insert("request".into(), Value::Object(req));
    item.insert("response".into(), json!([]));
    Value::Object(item)
}

fn body_json(body: Option<&Body>) -> Option<Value> {
    Some(match body? {
        Body::Json { content } => json!({
            "mode": "raw", "raw": content, "options": { "raw": { "language": "json" } },
        }),
        Body::Text { content } => json!({
            "mode": "raw", "raw": content, "options": { "raw": { "language": "text" } },
        }),
        Body::Form { fields } => json!({ "mode": "urlencoded", "urlencoded": kv(fields) }),
        Body::Graphql { query, variables } => json!({
            "mode": "graphql", "graphql": { "query": query, "variables": variables },
        }),
    })
}

/// Postman URL object: `raw` plus its parts (`protocol`, `host`, `port`, `path`, `query`).
pub(crate) fn url_json(url: &str, query: &IndexMap<String, String>) -> Value {
    let (base, inline) = url.split_once('?').unwrap_or((url, ""));
    let mut params: Vec<(String, String)> = inline
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (k.to_string(), v.to_string())
        })
        .collect();
    params.extend(query.iter().map(|(k, v)| (k.clone(), v.clone())));

    let mut raw = base.to_string();
    if !params.is_empty() {
        raw.push('?');
        raw.push_str(
            &params
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join("&"),
        );
    }

    let mut out = Map::new();
    out.insert("raw".into(), json!(raw));
    let (protocol, rest) = match base.split_once("://") {
        Some((p, r)) => (Some(p), r),
        None => (None, base),
    };
    if let Some(p) = protocol {
        out.insert("protocol".into(), json!(p));
    }
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    let (host, port) = match authority.rsplit_once(':') {
        // `{{host}}:{{port}}` or `example.com:8080` (not part of a `{{var}}`).
        Some((h, p)) if !h.is_empty() && (!p.contains('}') || p.ends_with("}}")) => (h, Some(p)),
        _ => (authority, None),
    };
    if !host.is_empty() {
        out.insert(
            "host".into(),
            host.split('.').map(|s| json!(s)).collect::<Vec<_>>().into(),
        );
    }
    if let Some(p) = port {
        out.insert("port".into(), json!(p));
    }
    if !path.is_empty() {
        out.insert(
            "path".into(),
            path.split('/').map(|s| json!(s)).collect::<Vec<_>>().into(),
        );
    }
    if !params.is_empty() {
        out.insert(
            "query".into(),
            params
                .iter()
                .map(|(k, v)| json!({ "key": k, "value": v }))
                .collect::<Vec<_>>()
                .into(),
        );
    }
    Value::Object(out)
}

// ---------- response variables (D48) → test script ----------

fn js(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

/// Postman test-script equivalent of the enabled `[[extract]]` rules.
pub(crate) fn extract_script(rules: &[Extract]) -> Option<String> {
    let rules: Vec<&Extract> = rules
        .iter()
        .filter(|r| r.enabled && !r.var.trim().is_empty())
        .collect();
    if rules.is_empty() {
        return None;
    }
    let mut out = vec![
        "// Variables from the response (exported from ilixPatch)".to_string(),
        "{".into(),
    ];
    let needs_json = rules.iter().any(|r| r.from == ExtractFrom::Json);
    if needs_json {
        out.push(
            "    const jsonAt = (steps) => { let v; try { v = pm.response.json(); } catch (e) { return undefined; } \
             for (const k of steps) { if (v === null || typeof v !== \"object\" || !(k in v)) return undefined; v = v[k]; } \
             return typeof v === \"string\" ? v : JSON.stringify(v); };"
                .into(),
        );
    }
    for r in rules {
        let value = match r.from {
            ExtractFrom::Json => {
                let steps = match parse_json_path(&r.path) {
                    Ok(steps) => steps,
                    Err(_) => continue,
                };
                let list: Vec<String> = steps
                    .iter()
                    .map(|s| match s {
                        PathStep::Key(k) => js(k),
                        PathStep::Index(i) => i.to_string(),
                    })
                    .collect();
                format!("jsonAt([{}])", list.join(", "))
            }
            ExtractFrom::Header => format!("pm.response.headers.get({})", js(r.path.trim())),
            ExtractFrom::Status => "String(pm.response.code)".into(),
            ExtractFrom::Body => "pm.response.text()".into(),
            ExtractFrom::Regex => format!(
                "((m) => (m ? (m[1] !== undefined ? m[1] : m[0]) : undefined))(pm.response.text().match(new RegExp({})))",
                js(&r.path)
            ),
        };
        let scope = match r.scope {
            ExtractScope::Environment => "environment",
            ExtractScope::Collection => "collectionVariables",
            ExtractScope::Global => "globals",
        };
        out.push(format!(
            "    {{ const v = {value}; if (v !== undefined && v !== null) pm.{scope}.set({}, v); }}",
            js(r.var.trim())
        ));
    }
    out.push("}".into());
    Some(out.join("\n"))
}

// ---------- auth (inverse of auth_map.rs) ----------

enum P {
    S(String),
    B(bool),
}

struct Params(Vec<(&'static str, P)>);

impl Params {
    fn s(mut self, key: &'static str, v: impl Into<String>) -> Self {
        self.0.push((key, P::S(v.into())));
        self
    }
    fn opt(self, key: &'static str, v: &Option<String>) -> Self {
        match v {
            Some(v) if !v.is_empty() => self.s(key, v.clone()),
            _ => self,
        }
    }
    fn b(mut self, key: &'static str, v: bool) -> Self {
        self.0.push((key, P::B(v)));
        self
    }
    fn into_auth(self, typ: &str) -> Value {
        let list: Vec<Value> = self
            .0
            .into_iter()
            .map(|(k, v)| match v {
                P::S(s) => json!({ "key": k, "value": s, "type": "string" }),
                P::B(b) => json!({ "key": k, "value": b, "type": "boolean" }),
            })
            .collect();
        json!({ "type": typ, typ: list })
    }
}

fn params() -> Params {
    Params(Vec::new())
}

fn duration(secs: u64) -> String {
    if secs > 0 && secs.is_multiple_of(3600) {
        format!("{}h", secs / 3600)
    } else if secs > 0 && secs.is_multiple_of(60) {
        format!("{}m", secs / 60)
    } else {
        format!("{secs}s")
    }
}

/// Postman `auth` object; `None` for `Inherit` (Postman's default too).
pub(crate) fn auth_json(auth: &Auth, include_secrets: bool) -> Option<Value> {
    Some(match auth {
        Auth::Inherit => return None,
        Auth::NoAuth => json!({ "type": "noauth" }),
        Auth::ApiKey(a) => params()
            .s("key", &a.key)
            .s("value", &a.value)
            .s(
                "in",
                if a.add_to == AddTo::Query {
                    "query"
                } else {
                    "header"
                },
            )
            .into_auth("apikey"),
        Auth::Bearer(a) => params().s("token", &a.token).into_auth("bearer"),
        Auth::Basic(a) => params()
            .s("username", &a.username)
            .s("password", &a.password)
            .into_auth("basic"),
        Auth::Digest(a) => params()
            .s("username", &a.username)
            .s("password", &a.password)
            .opt("realm", &a.realm)
            .opt("nonce", &a.nonce)
            .opt("algorithm", &a.algorithm)
            .opt("qop", &a.qop)
            .opt("nonceCount", &a.nonce_count)
            .opt("clientNonce", &a.client_nonce)
            .opt("opaque", &a.opaque)
            .b("disableRetryRequest", a.disable_retry)
            .into_auth("digest"),
        Auth::Oauth1(a) => params()
            .s("signatureMethod", &a.signature_method)
            .s("consumerKey", &a.consumer_key)
            .s("consumerSecret", &a.consumer_secret)
            .opt("token", &a.token)
            .opt("tokenSecret", &a.token_secret)
            .opt("privateKey", &a.private_key)
            .opt("callback", &a.callback_url)
            .opt("verifier", &a.verifier)
            .opt("timestamp", &a.timestamp)
            .opt("nonce", &a.nonce)
            .opt("version", &a.version)
            .opt("realm", &a.realm)
            .b("addParamsToHeader", a.add_to == AddTo::Header)
            .b("includeBodyHash", a.include_body_hash)
            .b("addEmptyParamsToSign", a.add_empty_params)
            .into_auth("oauth1"),
        Auth::Oauth2(a) => {
            let grant = match a.grant_type {
                OAuth2Grant::AuthorizationCode => "authorization_code",
                OAuth2Grant::AuthorizationCodePkce => "authorization_code_with_pkce",
                OAuth2Grant::Implicit => "implicit",
                OAuth2Grant::Password => "password_credentials",
                OAuth2Grant::ClientCredentials => "client_credentials",
            };
            let mut v = params()
                .s("grant_type", grant)
                .s("tokenName", &a.token_name)
                .opt("authUrl", &a.auth_url)
                .opt("accessTokenUrl", &a.access_token_url)
                .s("clientId", &a.client_id)
                .opt("clientSecret", &a.client_secret)
                .opt("scope", &a.scope)
                .opt("state", &a.state)
                .opt("redirect_uri", &a.callback_url)
                .opt("challengeAlgorithm", &a.code_challenge_method)
                .opt("code_verifier", &a.code_verifier)
                .opt("username", &a.username)
                .opt("password", &a.password)
                .s(
                    "client_authentication",
                    if a.client_authentication == ClientAuthentication::Body {
                        "body"
                    } else {
                        "header"
                    },
                )
                .opt("audience", &a.audience)
                .opt("resource", &a.resource)
                .s(
                    "addTokenTo",
                    if a.add_to == AddTo::Query {
                        "queryParams"
                    } else {
                        "header"
                    },
                )
                .opt("headerPrefix", &a.header_prefix);
            if include_secrets {
                v = v.opt("accessToken", &a.access_token);
            }
            let mut out = v.into_auth("oauth2");
            if !a.extra_params.is_empty()
                && let Some(Value::Array(list)) = out.get_mut("oauth2")
            {
                let extra: Vec<Value> = a
                    .extra_params
                    .iter()
                    .map(|(k, v)| json!({ "key": k, "value": v, "enabled": true, "send_as": "request_body" }))
                    .collect();
                list.push(json!({ "key": "tokenRequestParams", "value": extra, "type": "any" }));
            }
            out
        }
        Auth::Hawk(a) => params()
            .s("authId", &a.id)
            .s("authKey", &a.key)
            .opt("algorithm", &a.algorithm)
            .opt("user", &a.user)
            .opt("nonce", &a.nonce)
            .opt("extraData", &a.ext)
            .opt("app", &a.app)
            .opt("delegation", &a.dlg)
            .opt("timestamp", &a.timestamp)
            .b("includePayloadHash", a.include_payload_hash)
            .into_auth("hawk"),
        Auth::Awsv4(a) => params()
            .s("accessKey", &a.access_key)
            .s("secretKey", &a.secret_key)
            .opt("sessionToken", &a.session_token)
            .s("region", &a.region)
            .s("service", &a.service)
            .b("addAuthDataToQuery", a.add_to == AddTo::Query)
            .into_auth("awsv4"),
        Auth::Ntlm(a) => params()
            .s("username", &a.username)
            .s("password", &a.password)
            .opt("domain", &a.domain)
            .opt("workstation", &a.workstation)
            .b("disableRetryRequest", a.disable_retry)
            .into_auth("ntlm"),
        Auth::Akamai(a) => params()
            .s("clientToken", &a.client_token)
            .s("clientSecret", &a.client_secret)
            .s("accessToken", &a.access_token)
            .opt("baseURL", &a.base_url)
            .opt("nonce", &a.nonce)
            .opt("timestamp", &a.timestamp)
            .opt("headersToSign", &a.headers_to_sign)
            .opt("maxBodySize", &a.max_body_size.map(|n| n.to_string()))
            .into_auth("edgegrid"),
        Auth::Jwt(a) => params()
            .s("algorithm", &a.algorithm)
            .opt("secret", &a.secret)
            .b("isSecretBase64Encoded", a.secret_base64)
            .opt("privateKey", &a.private_key)
            .s("payload", &a.payload)
            .opt("header", &a.header)
            .s(
                "addTokenTo",
                if a.add_to == AddTo::Query {
                    "queryParam"
                } else {
                    "header"
                },
            )
            .opt("headerPrefix", &a.header_prefix)
            .opt("queryParamKey", &a.query_param)
            .into_auth("jwt"),
        Auth::Asap(a) => params()
            .s("alg", &a.algorithm)
            .s("iss", &a.issuer)
            .opt("sub", &a.subject)
            .s("aud", &a.audience)
            .s("kid", &a.key_id)
            .s("privateKey", &a.private_key)
            .opt("claims", &a.additional_claims)
            .opt("exp", &a.expires_in.map(duration))
            .into_auth("asap"),
    })
}
