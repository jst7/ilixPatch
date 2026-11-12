//! Reading Postman exports into a source tree.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use ilixpatch_core::environments::EnvVariable;
use serde_json::Value;

use crate::convert::{self, ConvertOptions};
use crate::{
    FileError, ImportEnvironment, ImportNode, ImportPreview, ImportWarning, NodeKind, stable_id,
};

/// Largest single JSON file (or zip entry) we read.
const MAX_FILE: u64 = 200 * 1024 * 1024;

/// A collection/folder script inherited by requests below it.
#[derive(Debug, Clone)]
pub(crate) struct ParentScript {
    /// `collection "X"` or `folder "Y"`.
    pub label: String,
    pub pre_request: Option<String>,
    pub tests: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct SrcNode {
    pub id: String,
    pub kind: NodeKind,
    pub name: String,
    /// Own `auth` object, if any.
    pub auth: Option<Value>,
    /// Auth in effect: own (unless `inherit`), else the nearest ancestor's.
    pub effective_auth: Option<Value>,
    pub pre_request: Option<String>,
    pub tests: Option<String>,
    /// Ancestors' scripts, outermost first.
    pub parents: Vec<ParentScript>,
    pub has_description: bool,
    /// Collection variables (collections only).
    pub variables: Vec<EnvVariable>,
    /// The Postman `request` (requests only).
    pub request: Option<Value>,
    pub children: Vec<SrcNode>,
}

impl SrcNode {
    pub(crate) fn walk<'a>(&'a self, out: &mut Vec<&'a SrcNode>) {
        out.push(self);
        for c in &self.children {
            c.walk(out);
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SrcEnv {
    pub id: String,
    pub name: String,
    pub variables: Vec<EnvVariable>,
}

/// Everything parsed from a set of input paths.
#[derive(Debug, Clone, Default)]
pub struct Parsed {
    pub(crate) collections: Vec<SrcNode>,
    pub(crate) environments: Vec<SrcEnv>,
    pub(crate) globals: Option<Vec<EnvVariable>>,
    pub(crate) errors: Vec<FileError>,
}

// ---------- JSON helpers ----------

pub(crate) fn text(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

pub(crate) fn str_at(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(text)
}

pub(crate) fn is_true(v: &Value, key: &str) -> bool {
    v.get(key).and_then(Value::as_bool).unwrap_or(false)
}

/// `[{key, value, disabled}]` → `(key, value, enabled)`.
pub(crate) fn kv_list(v: Option<&Value>) -> Vec<(String, String, bool)> {
    let Some(Value::Array(items)) = v else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|i| {
            let key = str_at(i, "key")?;
            let value = str_at(i, "value").unwrap_or_default();
            let enabled = !is_true(i, "disabled")
                && i.get("enabled").and_then(Value::as_bool).unwrap_or(true);
            Some((key, value, enabled))
        })
        .collect()
}

fn script_of(events: Option<&Value>, listen: &str) -> Option<String> {
    let Some(Value::Array(events)) = events else {
        return None;
    };
    let parts: Vec<String> = events
        .iter()
        .filter(|e| str_at(e, "listen").as_deref() == Some(listen) && !is_true(e, "disabled"))
        .filter_map(|e| {
            let exec = e.get("script")?.get("exec")?;
            match exec {
                Value::Array(lines) => {
                    Some(lines.iter().filter_map(text).collect::<Vec<_>>().join("\n"))
                }
                Value::String(s) => Some(s.clone()),
                _ => None,
            }
        })
        .filter(|s| !s.trim().is_empty())
        .collect();
    (!parts.is_empty()).then(|| parts.join("\n\n"))
}

fn has_description(v: &Value) -> bool {
    match v.get("description") {
        Some(Value::String(s)) => !s.trim().is_empty(),
        Some(Value::Object(o)) => o
            .get("content")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.trim().is_empty()),
        _ => false,
    }
}

fn auth_of(v: &Value) -> Option<Value> {
    v.get("auth")
        .filter(|a| a.is_object() && a.get("type").is_some())
        .cloned()
}

fn auth_type(a: &Value) -> String {
    str_at(a, "type").unwrap_or_default()
}

// ---------- collections ----------

struct Ctx<'a> {
    file: &'a str,
    inherited_auth: Option<Value>,
    parents: Vec<ParentScript>,
}

fn parse_items(items: Option<&Value>, ctx: &Ctx<'_>, path: &str) -> Vec<SrcNode> {
    let Some(Value::Array(items)) = items else {
        return Vec::new();
    };
    items
        .iter()
        .enumerate()
        .map(|(i, item)| parse_item(item, ctx, &format!("{path}/{i}")))
        .collect()
}

fn parse_item(item: &Value, ctx: &Ctx<'_>, path: &str) -> SrcNode {
    let id = stable_id(&format!("{}#{path}", ctx.file));
    let name = str_at(item, "name")
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| "Untitled".into());
    let auth = auth_of(item);
    let effective_auth = match &auth {
        Some(a) if auth_type(a) != "inherit" => Some(a.clone()),
        _ => ctx.inherited_auth.clone(),
    };
    let pre_request = script_of(item.get("event"), "prerequest");
    let tests = script_of(item.get("event"), "test");
    if let Some(children) = item.get("item") {
        let mut parents = ctx.parents.clone();
        if pre_request.is_some() || tests.is_some() {
            parents.push(ParentScript {
                label: format!("folder \"{name}\""),
                pre_request: pre_request.clone(),
                tests: tests.clone(),
            });
        }
        let child_ctx = Ctx {
            file: ctx.file,
            inherited_auth: effective_auth.clone(),
            parents,
        };
        SrcNode {
            id,
            kind: NodeKind::Folder,
            name,
            auth,
            effective_auth,
            pre_request,
            tests,
            parents: ctx.parents.clone(),
            has_description: has_description(item),
            variables: Vec::new(),
            request: None,
            children: parse_items(Some(children), &child_ctx, path),
        }
    } else {
        // v2.0 allows `"request": "https://…"`.
        let request = match item.get("request") {
            Some(Value::String(url)) => serde_json::json!({ "method": "GET", "url": url }),
            Some(r) => r.clone(),
            None => serde_json::json!({}),
        };
        let own_auth = auth.or_else(|| auth_of(&request));
        let effective_auth = match &own_auth {
            Some(a) if auth_type(a) != "inherit" => Some(a.clone()),
            _ => ctx.inherited_auth.clone(),
        };
        SrcNode {
            id,
            kind: NodeKind::Request,
            name,
            auth: own_auth,
            effective_auth,
            pre_request,
            tests,
            parents: ctx.parents.clone(),
            has_description: has_description(item) || has_description(&request),
            variables: Vec::new(),
            request: Some(request),
            children: Vec::new(),
        }
    }
}

fn parse_collection(v: &Value, file: &str) -> Result<SrcNode, String> {
    let info = v.get("info").cloned().unwrap_or(Value::Null);
    let schema = str_at(&info, "schema").unwrap_or_default();
    if schema.contains("v1.") {
        return Err(
            "Postman Collection v1 is not supported: export it again as Collection v2.1".into(),
        );
    }
    if !schema.is_empty() && !schema.contains("v2.0") && !schema.contains("v2.1") {
        return Err(format!("unsupported collection schema: {schema}"));
    }
    let name = str_at(&info, "name")
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| "Imported collection".into());
    let auth = auth_of(v);
    let effective_auth = auth.clone().filter(|a| auth_type(a) != "inherit");
    let pre_request = script_of(v.get("event"), "prerequest");
    let tests = script_of(v.get("event"), "test");
    let mut parents = Vec::new();
    if pre_request.is_some() || tests.is_some() {
        parents.push(ParentScript {
            label: format!("collection \"{name}\""),
            pre_request: pre_request.clone(),
            tests: tests.clone(),
        });
    }
    let variables = kv_list(v.get("variable"))
        .into_iter()
        .map(|(k, val, enabled)| EnvVariable {
            enabled,
            ..EnvVariable::new(k, val)
        })
        .collect();
    let ctx = Ctx {
        file,
        inherited_auth: effective_auth.clone(),
        parents,
    };
    Ok(SrcNode {
        id: stable_id(&format!("{file}#")),
        kind: NodeKind::Collection,
        name,
        auth,
        effective_auth,
        pre_request,
        tests,
        parents: Vec::new(),
        has_description: has_description(&info),
        variables,
        request: None,
        children: parse_items(v.get("item"), &ctx, ""),
    })
}

fn env_vars(values: Option<&Value>) -> Vec<EnvVariable> {
    let Some(Value::Array(items)) = values else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|i| {
            let key = str_at(i, "key")?;
            Some(EnvVariable {
                key,
                value: str_at(i, "value").unwrap_or_default(),
                secret: str_at(i, "type").as_deref() == Some("secret"),
                enabled: i.get("enabled").and_then(Value::as_bool).unwrap_or(true)
                    && !is_true(i, "disabled"),
            })
        })
        .collect()
}

fn is_v1_collection(v: &Value) -> bool {
    v.get("requests").is_some() && (v.get("order").is_some() || v.get("folders").is_some())
}

impl Parsed {
    fn error(&mut self, file: &str, message: impl Into<String>) {
        self.errors.push(FileError {
            file: file.to_string(),
            message: message.into(),
        });
    }

    fn add_globals(&mut self, vars: Vec<EnvVariable>) {
        let g = self.globals.get_or_insert_with(Vec::new);
        for v in vars {
            match g.iter_mut().find(|x| x.key == v.key) {
                Some(existing) => *existing = v,
                None => g.push(v),
            }
        }
    }

    fn add_env(&mut self, v: &Value, file: &str, index: usize) {
        let name = str_at(v, "name")
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| "Imported environment".into());
        self.environments.push(SrcEnv {
            id: stable_id(&format!("{file}#env{index}")),
            name,
            variables: env_vars(v.get("values")),
        });
    }

    fn add_collection(&mut self, v: &Value, file: &str) {
        if is_v1_collection(v) {
            self.error(
                file,
                "Postman Collection v1 is not supported: export it again as Collection v2.1",
            );
            return;
        }
        match parse_collection(v, file) {
            Ok(c) => self.collections.push(c),
            Err(e) => self.error(file, e),
        }
    }

    /// Classify one JSON document. Unknown documents are reported only when
    /// the file was given explicitly.
    fn add_json(&mut self, file: &str, bytes: &[u8], explicit: bool) {
        let v: Value = match serde_json::from_slice(bytes) {
            Ok(v) => v,
            Err(e) => {
                if explicit {
                    self.error(file, format!("invalid JSON: {e}"));
                }
                return;
            }
        };
        let scope = str_at(&v, "_postman_variable_scope");
        if (v.get("info").is_some() && v.get("item").is_some()) || is_v1_collection(&v) {
            self.add_collection(&v, file);
        } else if scope.as_deref() == Some("globals") {
            self.add_globals(env_vars(v.get("values")));
        } else if scope.as_deref() == Some("environment")
            || (v.get("values").is_some() && v.get("name").is_some())
        {
            self.add_env(&v, file, 0);
        } else if v.get("collections").is_some() || v.get("environments").is_some() {
            // Old single-file data dump.
            if let Some(Value::Array(cs)) = v.get("collections") {
                for (i, c) in cs.iter().enumerate() {
                    self.add_collection(c, &format!("{file}#collections/{i}"));
                }
            }
            if let Some(Value::Array(es)) = v.get("environments") {
                for (i, e) in es.iter().enumerate() {
                    self.add_env(e, file, i);
                }
            }
            if let Some(g) = v.get("globals") {
                self.add_globals(env_vars(Some(g)));
            }
        } else if file.ends_with("archive.json")
            || v.get("collection").is_some() && v.get("environment").is_some()
        {
            // Index of a data dump: nothing to import by itself.
        } else if explicit {
            self.error(
                file,
                "not a Postman collection, environment or globals export",
            );
        }
    }

    fn add_zip(&mut self, path: &Path) {
        let label = path.display().to_string();
        let file = match fs::File::open(path) {
            Ok(f) => f,
            Err(e) => return self.error(&label, e.to_string()),
        };
        let mut archive = match zip::ZipArchive::new(file) {
            Ok(a) => a,
            Err(e) => return self.error(&label, format!("invalid zip: {e}")),
        };
        let mut names: Vec<String> = archive
            .file_names()
            .flatten()
            .map(|n| n.into_owned())
            .collect();
        names.sort();
        for name in names {
            if !name.to_ascii_lowercase().ends_with(".json") {
                continue;
            }
            let entry_label = format!("{label}!{name}");
            let mut bytes = Vec::new();
            let res = archive
                .by_name(&name)
                .map_err(|e| e.to_string())
                .and_then(|e| {
                    if e.size() > MAX_FILE {
                        return Err("file too large".to_string());
                    }
                    e.take(MAX_FILE)
                        .read_to_end(&mut bytes)
                        .map_err(|e| e.to_string())
                });
            match res {
                Ok(_) => self.add_json(&entry_label, &bytes, false),
                Err(e) => self.error(&entry_label, e),
            }
        }
    }

    fn add_path(&mut self, path: &Path, explicit: bool) {
        let label = path.display().to_string();
        let meta = match fs::metadata(path) {
            Ok(m) => m,
            Err(e) => return self.error(&label, e.to_string()),
        };
        if meta.is_dir() {
            let mut entries: Vec<PathBuf> = match fs::read_dir(path) {
                Ok(rd) => rd.flatten().map(|e| e.path()).collect(),
                Err(e) => return self.error(&label, e.to_string()),
            };
            entries.sort();
            for p in entries {
                let hidden = p
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with('.'));
                let ext = p
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(str::to_ascii_lowercase);
                if hidden {
                    continue;
                }
                if p.is_dir() || matches!(ext.as_deref(), Some("json" | "zip")) {
                    self.add_path(&p, false);
                }
            }
            return;
        }
        let is_zip = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("zip"));
        if is_zip {
            return self.add_zip(path);
        }
        if meta.len() > MAX_FILE {
            return self.error(&label, "file too large");
        }
        match fs::read(path) {
            Ok(bytes) => self.add_json(&label, &bytes, explicit),
            Err(e) => self.error(&label, e.to_string()),
        }
    }

    /// The source node with this id, with nothing else.
    pub(crate) fn node(&self, id: &str) -> Option<&SrcNode> {
        let mut all = Vec::new();
        for c in &self.collections {
            c.walk(&mut all);
        }
        all.into_iter().find(|n| n.id == id)
    }

    pub(crate) fn env(&self, id: &str) -> Option<&SrcEnv> {
        self.environments.iter().find(|e| e.id == id)
    }

    /// What the UI shows, with warnings computed for default options.
    pub fn preview(&self, session: impl Into<String>) -> ImportPreview {
        let mut warnings = Vec::new();
        let opts = ConvertOptions::default();
        for c in &self.collections {
            collection_warnings(c, &opts, &mut warnings);
        }
        ImportPreview {
            session: session.into(),
            collections: self.collections.iter().map(to_import_node).collect(),
            environments: self
                .environments
                .iter()
                .map(|e| ImportEnvironment {
                    id: e.id.clone(),
                    name: e.name.clone(),
                    variables: e.variables.clone(),
                })
                .collect(),
            globals: self.globals.clone(),
            warnings,
            errors: self.errors.clone(),
        }
    }
}

/// Conversion warnings of every request in `c`, plus one `description`
/// warning per collection when any descriptions are dropped.
pub(crate) fn collection_warnings(
    c: &SrcNode,
    opts: &ConvertOptions,
    out: &mut Vec<ImportWarning>,
) {
    let mut all = Vec::new();
    c.walk(&mut all);
    if all.iter().any(|n| n.has_description) {
        out.push(ImportWarning::new(
            Some(&c.id),
            "description",
            format!("Descriptions in \"{}\" are not imported", c.name),
        ));
    }
    for n in all.iter().filter(|n| n.kind == NodeKind::Request) {
        convert::convert(n, opts, false, out);
    }
}

fn to_import_node(n: &SrcNode) -> ImportNode {
    let (method, url) = match &n.request {
        Some(r) => (Some(convert::method_of(r).0), Some(convert::display_url(r))),
        None => (None, None),
    };
    let own_scripts = n.pre_request.is_some() || n.tests.is_some();
    let inherited = n
        .parents
        .iter()
        .any(|p| p.pre_request.is_some() || p.tests.is_some());
    let auth = match &n.auth {
        Some(a) => Some(auth_type(a)),
        None if n.kind == NodeKind::Request && n.effective_auth.is_some() => Some("inherit".into()),
        None => None,
    };
    ImportNode {
        id: n.id.clone(),
        kind: n.kind,
        name: n.name.clone(),
        method,
        url,
        has_scripts: own_scripts || (n.kind == NodeKind::Request && inherited),
        auth,
        variables: (n.kind == NodeKind::Collection).then(|| n.variables.clone()),
        children: n.children.iter().map(to_import_node).collect(),
    }
}

/// Parse every path: `.json` files, `.zip` dumps and directories of them.
pub fn parse_paths<P: AsRef<Path>>(paths: &[P]) -> Parsed {
    let mut parsed = Parsed::default();
    for p in paths {
        parsed.add_path(p.as_ref(), true);
    }
    parsed
}
