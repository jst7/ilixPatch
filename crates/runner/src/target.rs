//! Turning a path (project, collection, folder or request file) into the list
//! of requests to run, plus data-file and environment loading.

use std::fs;
use std::path::{Path, PathBuf};

use ilixpatch_core::environments::{EnvStore, enabled_map, find_environment, load_globals};
use ilixpatch_core::{Collection, Environment, Folder, Request, project, storage};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::RunError;
use crate::run::EnvSelection;

/// One request of a run.
#[derive(Debug, Clone)]
pub struct RunItem {
    /// Absolute path of the request `.toml` file.
    pub file: PathBuf,
    /// Path inside the collection, e.g. `users/List users`.
    pub item_path: String,
    pub request: Request,
}

impl RunItem {
    /// Folder segments of [`RunItem::item_path`] (without the request name).
    pub fn folders(&self) -> Vec<&str> {
        let mut parts: Vec<&str> = self.item_path.split('/').collect();
        parts.pop();
        parts
    }
}

/// The requests of one collection selected by a target path.
#[derive(Debug, Clone)]
pub struct RunTarget {
    pub collection: Collection,
    pub items: Vec<RunItem>,
    /// Environments of the enclosing project (`.ilixpatch/environments/`), if any.
    pub project_environments: Vec<Environment>,
}

impl RunTarget {
    /// Environment `name` for this target: the project environment, falling
    /// back to a legacy collection environment (`<collection>/environments/`).
    pub fn environment(&self, name: &str) -> Option<&Environment> {
        find_environment(&self.project_environments, name)
            .or_else(|| find_environment(&self.collection.environments, name))
    }
}

/// Where a named environment is looked up (`EnvScope` in api.ts).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EnvScope {
    /// The project's environments, then legacy collection environments.
    #[default]
    Project,
    /// The user's global environments (app config dir).
    Global,
}

/// Turn a UI environment choice into an [`EnvSelection`]. Global environments
/// are read from `<config_dir>/environments/`.
pub fn environment_selection(
    name: Option<&str>,
    scope: Option<EnvScope>,
    config_dir: Option<&Path>,
) -> Result<Option<EnvSelection>, RunError> {
    let Some(name) = name.filter(|n| !n.is_empty()) else {
        return Ok(None);
    };
    match scope.unwrap_or_default() {
        EnvScope::Project => Ok(Some(EnvSelection::Named(name.to_string()))),
        EnvScope::Global => {
            let dir = config_dir.ok_or_else(|| {
                RunError::Environment("global environments need the app config dir".into())
            })?;
            let doc = EnvStore::global(dir)
                .get(name)
                .map_err(|e| RunError::Environment(e.to_string()))?;
            Ok(Some(EnvSelection::Inline(doc.to_environment())))
        }
    }
}

/// Enabled variables of `<config_dir>/globals.toml` (empty if missing).
pub fn load_globals_map(config_dir: &Path) -> Result<IndexMap<String, String>, RunError> {
    load_globals(config_dir)
        .map(|v| enabled_map(&v))
        .map_err(|e| RunError::Environment(e.to_string()))
}

/// CLI `--env <arg>`: a project environment of that name, else a legacy
/// collection environment, else a path to an environment file (ilixPatch
/// `.toml` or Postman `.json`).
pub fn select_environment(targets: &[RunTarget], arg: &str) -> Result<EnvSelection, RunError> {
    if targets.iter().any(|t| t.environment(arg).is_some()) {
        return Ok(EnvSelection::Named(arg.to_string()));
    }
    if Path::new(arg).is_file() {
        return Ok(EnvSelection::Inline(load_environment_file(arg)?));
    }
    Err(RunError::Environment(format!(
        "environment '{arg}' not found"
    )))
}

/// Check that a named environment exists for at least one target.
pub(crate) fn check_environment(
    targets: &[RunTarget],
    env: Option<&EnvSelection>,
) -> Result<(), RunError> {
    if let Some(EnvSelection::Named(name)) = env
        && !targets.iter().any(|t| t.environment(name).is_some())
    {
        return Err(RunError::Environment(format!(
            "environment '{name}' not found"
        )));
    }
    Ok(())
}

/// Environments of the project enclosing `dir` (empty if none).
fn enclosing_project_environments(dir: &Path) -> Result<Vec<Environment>, RunError> {
    match dir.ancestors().find(|d| project::is_project(d)) {
        Some(root) => Ok(EnvStore::project(root).environments()?),
        None => Ok(Vec::new()),
    }
}

fn collect(folder: &Folder, prefix: &str, out: &mut Vec<RunItem>) {
    for r in &folder.requests {
        out.push(RunItem {
            file: r.path.clone(),
            item_path: format!("{prefix}{}", r.request.name),
            request: r.request.clone(),
        });
    }
    for f in &folder.folders {
        collect(f, &format!("{prefix}{}/", f.name), out);
    }
}

fn find_folder<'a>(folder: &'a Folder, dir: &Path, prefix: &str) -> Option<(&'a Folder, String)> {
    if folder.path == dir {
        return Some((folder, prefix.to_string()));
    }
    folder
        .folders
        .iter()
        .find_map(|f| find_folder(f, dir, &format!("{prefix}{}/", f.name)))
}

/// Walk up from `start` to the nearest directory containing `collection.toml`.
fn find_collection_dir(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|d| d.join(storage::COLLECTION_FILE).is_file())
        .map(Path::to_path_buf)
}

/// Resolve a target path: a project directory (every collection), a
/// collection directory, a folder directory, or a single request `.toml`.
/// Request order mirrors [`Collection::requests`] (a folder's requests before
/// its subfolders, depth-first).
///
/// Each item's `auth` is its effective auth (D39 inheritance: request →
/// folders → collection; `NoAuth` when nothing is set).
pub fn resolve_target(path: impl AsRef<Path>) -> Result<Vec<RunTarget>, RunError> {
    let mut targets = resolve_target_tree(path.as_ref())?;
    for t in &mut targets {
        for item in &mut t.items {
            item.request.auth = ilixpatch_core::auth::effective_auth(&t.collection, &item.file);
        }
    }
    Ok(targets)
}

fn resolve_target_tree(path: &Path) -> Result<Vec<RunTarget>, RunError> {
    let abs =
        fs::canonicalize(path).map_err(|e| RunError::Target(format!("{}: {e}", path.display())))?;

    if abs.is_dir() && project::is_project(&abs) && !abs.join(storage::COLLECTION_FILE).is_file() {
        let p = project::load_project(&abs)?;
        if p.collections.is_empty() {
            return Err(RunError::Target(format!(
                "project '{}' has no collections",
                p.name
            )));
        }
        let envs = p.environments;
        return Ok(p
            .collections
            .into_iter()
            .map(|c| {
                let mut items = Vec::new();
                collect(&c.root, "", &mut items);
                RunTarget {
                    collection: c,
                    items,
                    project_environments: envs.clone(),
                }
            })
            .collect());
    }

    let dir_start = if abs.is_dir() {
        abs.as_path()
    } else {
        abs.parent().unwrap_or(&abs)
    };
    let col_dir = find_collection_dir(dir_start).ok_or_else(|| {
        RunError::Target(format!(
            "{}: not inside a collection (no collection.toml found)",
            path.display()
        ))
    })?;
    let collection = storage::load_collection(&col_dir)?;

    let items = if abs.is_dir() {
        let (folder, prefix) = find_folder(&collection.root, &abs, "").ok_or_else(|| {
            RunError::Target(format!(
                "{}: not a folder of the collection",
                path.display()
            ))
        })?;
        let mut items = Vec::new();
        collect(folder, &prefix, &mut items);
        items
    } else {
        let mut all = Vec::new();
        collect(&collection.root, "", &mut all);
        let items: Vec<_> = all.into_iter().filter(|i| i.file == abs).collect();
        if items.is_empty() {
            return Err(RunError::Target(format!(
                "{}: not a request of the collection",
                path.display()
            )));
        }
        items
    };
    Ok(vec![RunTarget {
        project_environments: enclosing_project_environments(&col_dir)?,
        collection,
        items,
    }])
}

/// Keep only requests inside a folder named one of `names` (any depth), or
/// whose name is one of `names` (like Newman's `--folder`). Targets left
/// without requests are dropped.
pub fn filter_folders(targets: Vec<RunTarget>, names: &[String]) -> Vec<RunTarget> {
    if names.is_empty() {
        return targets;
    }
    targets
        .into_iter()
        .filter_map(|mut t| {
            t.items.retain(|i| {
                names
                    .iter()
                    .any(|n| i.request.name == *n || i.folders().iter().any(|f| f == n))
            });
            (!t.items.is_empty()).then_some(t)
        })
        .collect()
}

pub type DataRow = IndexMap<String, Value>;

/// Load iteration data: CSV (header row) or JSON (array of objects).
/// The format is chosen by extension (`.csv` / `.json`), falling back to
/// sniffing the first non-blank character.
pub fn load_data_file(path: impl AsRef<Path>) -> Result<Vec<DataRow>, RunError> {
    let path = path.as_ref();
    let text =
        fs::read_to_string(path).map_err(|e| RunError::Data(format!("{}: {e}", path.display())))?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    let is_json = match ext.as_deref() {
        Some("json") => true,
        Some("csv") => false,
        _ => text.trim_start().starts_with('['),
    };
    let err = |e: &dyn std::fmt::Display| RunError::Data(format!("{}: {e}", path.display()));
    if is_json {
        let v: Value = serde_json::from_str(&text).map_err(|e| err(&e))?;
        let Value::Array(rows) = v else {
            return Err(err(&"expected a JSON array of objects"));
        };
        rows.into_iter()
            .map(|r| match r {
                Value::Object(o) => Ok(o.into_iter().collect()),
                _ => Err(err(&"expected a JSON array of objects")),
            })
            .collect()
    } else {
        let mut rdr = csv::ReaderBuilder::new()
            .trim(csv::Trim::Headers)
            .flexible(true)
            .from_reader(text.trim_start_matches('\u{feff}').as_bytes());
        let headers = rdr.headers().map_err(|e| err(&e))?.clone();
        let mut rows = Vec::new();
        for rec in rdr.records() {
            let rec = rec.map_err(|e| err(&e))?;
            if rec.iter().all(|f| f.is_empty()) {
                continue;
            }
            rows.push(
                headers
                    .iter()
                    .zip(rec.iter())
                    .map(|(h, v)| (h.to_string(), Value::String(v.to_string())))
                    .collect(),
            );
        }
        Ok(rows)
    }
}

/// Load an environment from a file: a ilixPatch environment `.toml`
/// (`[variables]`) or a Postman environment export (`.json`, `values: [{key,
/// value, enabled}]`). The name is the Postman `name` or the file stem.
pub fn load_environment_file(path: impl AsRef<Path>) -> Result<Environment, RunError> {
    let path = path.as_ref();
    let err = |e: &dyn std::fmt::Display| RunError::Environment(format!("{}: {e}", path.display()));
    let text = fs::read_to_string(path).map_err(|e| err(&e))?;
    let stem = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("environment")
        .trim_end_matches(".json")
        .trim_end_matches(".toml")
        .trim_end_matches(".postman_environment")
        .to_string();
    let is_json =
        path.extension().is_some_and(|e| e == "json") || text.trim_start().starts_with('{');
    if is_json {
        let v: Value = serde_json::from_str(&text).map_err(|e| err(&e))?;
        let name = v
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or(stem);
        let mut variables = IndexMap::new();
        if let Some(values) = v.get("values").and_then(Value::as_array) {
            for item in values {
                if item.get("enabled").and_then(Value::as_bool) == Some(false) {
                    continue;
                }
                if let Some(k) = item.get("key").and_then(Value::as_str) {
                    let val = item
                        .get("value")
                        .map(crate::vars::value_to_string)
                        .unwrap_or_default();
                    variables.insert(k.to_string(), val);
                }
            }
        } else {
            return Err(err(&"not a Postman environment (missing \"values\")"));
        }
        Ok(Environment { name, variables })
    } else {
        let mut env: Environment = toml::from_str(&text).map_err(|e| err(&e))?;
        env.name = stem;
        Ok(env)
    }
}

/// Serialize variables as a Postman environment JSON (what Newman's
/// `--export-environment` writes).
pub fn postman_environment_json(name: &str, vars: &IndexMap<String, String>) -> Value {
    serde_json::json!({
        "name": name,
        "values": vars.iter().map(|(k, v)| serde_json::json!({
            "key": k, "value": v, "type": "default", "enabled": true
        })).collect::<Vec<_>>(),
        "_postman_variable_scope": "environment",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_and_json_data() {
        let dir = tempfile::tempdir().unwrap();
        let csv = dir.path().join("d.csv");
        fs::write(&csv, "id, name\n1,Ann\n\n2,\"Bo, b\"\n").unwrap();
        let rows = load_data_file(&csv).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1]["name"], Value::from("Bo, b"));
        assert_eq!(rows[0]["id"], Value::from("1"));

        let json = dir.path().join("d.json");
        fs::write(&json, r#"[{"id":1},{"id":2,"x":"y"}]"#).unwrap();
        let rows = load_data_file(&json).unwrap();
        assert_eq!(rows[1]["x"], Value::from("y"));
        assert_eq!(rows[0]["id"], Value::from(1));

        fs::write(&json, r#"{"id":1}"#).unwrap();
        assert!(load_data_file(&json).is_err());
    }

    #[test]
    fn postman_environment() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("staging.postman_environment.json");
        fs::write(
            &p,
            r#"{"name":"Staging","values":[{"key":"a","value":"1","enabled":true},{"key":"b","value":"2","enabled":false}]}"#,
        )
        .unwrap();
        let env = load_environment_file(&p).unwrap();
        assert_eq!(env.name, "Staging");
        assert_eq!(env.variables.len(), 1);

        let t = dir.path().join("qa.toml");
        fs::write(&t, "[variables]\nhost = \"x\"\n").unwrap();
        let env = load_environment_file(&t).unwrap();
        assert_eq!(env.name, "qa");
        assert_eq!(env.variables["host"], "x");
    }
}
