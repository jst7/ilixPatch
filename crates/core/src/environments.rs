//! Project and global environments (D28), plus user-level globals.
//!
//! Both kinds share one file format, one file per environment:
//!
//! ```toml
//! name = "Dev"
//!
//! [[variables]]
//! key = "base_url"
//! value = "https://dev.example.com"
//! enabled = true
//!
//! [[variables]]
//! key = "token"
//! secret = true      # project: value lives in <slug>.secret.toml
//! enabled = true
//! ```
//!
//! * **Project** environments live in `<project>/.ilixpatch/environments/`.
//!   Secret values are never written to the repo: they go to
//!   `<slug>.secret.toml` (`[variables] token = "…"`), which
//!   `.ilixpatch/.gitignore` ignores.
//! * **Global** environments live in `<app config dir>/environments/` and keep
//!   every value inline (user-level, never in a repo; `0600` on unix).
//! * **Globals** (`pm.globals`) live in `<app config dir>/globals.toml`.
//!
//! The functions here are path-agnostic: callers pass the base directories.
//! For convenience the reader also accepts the legacy `[variables] k = "v"`
//! table form.

use std::fs;
use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::model::Environment;
use crate::project::{PROJECT_DIR, slugify};
use crate::storage::{
    ENVIRONMENTS_DIR, Result, SECRET_SUFFIX, StorageError, io_err, parse, sorted_entries,
};

pub const GLOBALS_FILE: &str = "globals.toml";
const GITIGNORE_LINE: &str = "environments/*.secret.toml";

/// One variable of an environment (`EnvVariable` in api.ts).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvVariable {
    pub key: String,
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub secret: bool,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

impl EnvVariable {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
            secret: false,
            enabled: true,
        }
    }
}

/// An editable environment (`EnvironmentDoc` in api.ts).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct EnvironmentDoc {
    pub name: String,
    #[serde(default)]
    pub variables: Vec<EnvVariable>,
    /// D49: colour of the environment in the UI (`#rrggbb` or a theme token name).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// D49: icon id of the environment in the UI (see `envIcons.ts`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

impl EnvironmentDoc {
    /// The variables used when sending: enabled ones only, secrets included.
    pub fn to_environment(&self) -> Environment {
        Environment {
            name: self.name.clone(),
            variables: enabled_map(&self.variables),
            color: self.color.clone(),
            icon: self.icon.clone(),
        }
    }
}

/// Enabled variables as a map (later duplicates win).
pub fn enabled_map(vars: &[EnvVariable]) -> IndexMap<String, String> {
    vars.iter()
        .filter(|v| v.enabled)
        .map(|v| (v.key.clone(), v.value.clone()))
        .collect()
}

/// Find an environment by name: exact match first, then case-insensitive.
pub fn find_environment<'a>(envs: &'a [Environment], name: &str) -> Option<&'a Environment> {
    envs.iter()
        .find(|e| e.name == name)
        .or_else(|| envs.iter().find(|e| e.name.eq_ignore_ascii_case(name)))
}

// ---------- file format ----------

#[derive(Debug, Serialize, Deserialize)]
struct FileVar {
    key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    value: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    secret: bool,
    #[serde(default = "yes")]
    enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum FileVars {
    List(Vec<FileVar>),
    /// Legacy `[variables] key = "value"`.
    Map(IndexMap<String, String>),
}

impl Default for FileVars {
    fn default() -> Self {
        Self::List(Vec::new())
    }
}

#[derive(Debug, Deserialize)]
struct FileIn {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    color: Option<String>,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    variables: FileVars,
}

#[derive(Debug, Serialize)]
struct FileOut<'a> {
    name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    color: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    icon: Option<&'a str>,
    variables: Vec<FileVar>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct SecretFile {
    #[serde(default)]
    variables: IndexMap<String, String>,
}

fn read_vars(vars: FileVars) -> Vec<EnvVariable> {
    match vars {
        FileVars::List(list) => list
            .into_iter()
            .map(|v| EnvVariable {
                key: v.key,
                value: v.value.unwrap_or_default(),
                secret: v.secret,
                enabled: v.enabled,
            })
            .collect(),
        FileVars::Map(map) => map
            .into_iter()
            .map(|(k, v)| EnvVariable::new(k, v))
            .collect(),
    }
}

/// `inline_secrets = false` writes secrets without their value.
fn write_vars(vars: &[EnvVariable], inline_secrets: bool) -> Vec<FileVar> {
    vars.iter()
        .map(|v| FileVar {
            key: v.key.clone(),
            value: (!v.secret || inline_secrets).then(|| v.value.clone()),
            secret: v.secret,
            enabled: v.enabled,
        })
        .collect()
}

fn write_file(path: &Path, contents: &str, private: bool) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(io_err(dir))?;
    }
    if !private {
        return fs::write(path, contents).map_err(io_err(path));
    }
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .map_err(io_err(path))?;
        // `mode` only applies on creation; tighten pre-existing files too.
        f.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(io_err(path))?;
        f.write_all(contents.as_bytes()).map_err(io_err(path))
    }
    #[cfg(not(unix))]
    {
        fs::write(path, contents).map_err(io_err(path))
    }
}

fn remove_if_exists(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(io_err(path)(e)),
        _ => Ok(()),
    }
}

/// Validate and normalize an environment name.
pub fn validate_name(name: &str) -> Result<String> {
    let trimmed = name.trim();
    if trimmed.is_empty()
        || trimmed.len() > 200
        || trimmed
            .chars()
            .any(|c| c == '/' || c == '\\' || c.is_control())
    {
        return Err(StorageError::InvalidName(name.into()));
    }
    Ok(trimmed.to_string())
}

// ---------- stores ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Secrets split into gitignored `<slug>.secret.toml`.
    Project,
    /// Everything inline, private file permissions.
    Global,
}

/// A directory of environments: a project's or the user's global ones.
#[derive(Debug, Clone)]
pub struct EnvStore {
    dir: PathBuf,
    kind: Kind,
}

struct Stored {
    stem: String,
    doc: EnvironmentDoc,
}

impl EnvStore {
    /// `<project_root>/.ilixpatch/environments/`.
    pub fn project(project_root: impl AsRef<Path>) -> Self {
        Self {
            dir: project_root
                .as_ref()
                .join(PROJECT_DIR)
                .join(ENVIRONMENTS_DIR),
            kind: Kind::Project,
        }
    }

    /// `<config_dir>/environments/` (the app config dir).
    pub fn global(config_dir: impl AsRef<Path>) -> Self {
        Self {
            dir: config_dir.as_ref().join(ENVIRONMENTS_DIR),
            kind: Kind::Global,
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn main_path(&self, stem: &str) -> PathBuf {
        self.dir.join(format!("{stem}.toml"))
    }

    fn secret_path(&self, stem: &str) -> PathBuf {
        self.dir.join(format!("{stem}{SECRET_SUFFIX}"))
    }

    fn read(&self, stem: &str) -> Result<EnvironmentDoc> {
        let file: FileIn = parse(&self.main_path(stem))?;
        let mut variables = read_vars(file.variables);
        let secret_path = self.secret_path(stem);
        if self.kind == Kind::Project && secret_path.is_file() {
            let mut secrets = parse::<SecretFile>(&secret_path)?.variables;
            for v in &mut variables {
                if let Some(val) = secrets.shift_remove(&v.key) {
                    v.value = val;
                    v.secret = true;
                }
            }
            // Secrets not declared in the main file still apply.
            variables.extend(secrets.into_iter().map(|(k, v)| EnvVariable {
                secret: true,
                ..EnvVariable::new(k, v)
            }));
        }
        Ok(EnvironmentDoc {
            name: file.name.unwrap_or_else(|| stem.to_string()),
            variables,
            color: file.color,
            icon: file.icon,
        })
    }

    fn stored(&self) -> Result<Vec<Stored>> {
        if !self.dir.is_dir() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for path in sorted_entries(&self.dir)? {
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if name.starts_with('.') || name.ends_with(SECRET_SUFFIX) || !path.is_file() {
                continue;
            }
            let Some(stem) = name.strip_suffix(".toml") else {
                continue;
            };
            out.push(Stored {
                stem: stem.to_string(),
                doc: self.read(stem)?,
            });
        }
        out.sort_by_cached_key(|s| s.doc.name.to_lowercase());
        Ok(out)
    }

    fn find(stored: &[Stored], name: &str) -> Option<usize> {
        stored.iter().position(|s| s.doc.name == name).or_else(|| {
            stored
                .iter()
                .position(|s| s.doc.name.eq_ignore_ascii_case(name))
        })
    }

    /// Every environment, sorted by name (case-insensitive).
    pub fn list(&self) -> Result<Vec<EnvironmentDoc>> {
        Ok(self.stored()?.into_iter().map(|s| s.doc).collect())
    }

    /// Enabled variables of every environment (for sending).
    pub fn environments(&self) -> Result<Vec<Environment>> {
        Ok(self
            .list()?
            .iter()
            .map(EnvironmentDoc::to_environment)
            .collect())
    }

    pub fn get(&self, name: &str) -> Result<EnvironmentDoc> {
        let stored = self.stored()?;
        Self::find(&stored, name)
            .map(|i| stored[i].doc.clone())
            .ok_or_else(|| StorageError::EnvNotFound(name.into()))
    }

    fn write(&self, stem: &str, doc: &EnvironmentDoc) -> Result<()> {
        let inline = self.kind == Kind::Global;
        let out = FileOut {
            name: &doc.name,
            color: doc.color.as_deref(),
            icon: doc.icon.as_deref(),
            variables: write_vars(&doc.variables, inline),
        };
        write_file(
            &self.main_path(stem),
            &toml::to_string_pretty(&out)?,
            inline,
        )?;
        if self.kind == Kind::Project {
            let secrets: IndexMap<String, String> = doc
                .variables
                .iter()
                .filter(|v| v.secret)
                .map(|v| (v.key.clone(), v.value.clone()))
                .collect();
            let secret_path = self.secret_path(stem);
            if secrets.is_empty() {
                remove_if_exists(&secret_path)?;
            } else {
                self.ensure_gitignore()?;
                let file = SecretFile { variables: secrets };
                write_file(&secret_path, &toml::to_string_pretty(&file)?, true)?;
            }
        }
        Ok(())
    }

    /// `.ilixpatch/.gitignore` must ignore `environments/*.secret.toml`.
    fn ensure_gitignore(&self) -> Result<()> {
        let Some(ilixpatch_dir) = self.dir.parent() else {
            return Ok(());
        };
        ensure_gitignore(ilixpatch_dir)
    }

    fn unique_stem(&self, slug: &str, keep: Option<&str>) -> String {
        (1..)
            .map(|i| {
                if i == 1 {
                    slug.to_string()
                } else {
                    format!("{slug}-{i}")
                }
            })
            .find(|stem| Some(stem.as_str()) == keep || !self.main_path(stem).exists())
            .expect("unbounded iterator")
    }

    /// Create or update `doc`. `original_name` renames an existing environment
    /// (files are moved); `None` creates a new one or updates the one with the
    /// same name. Names are unique per store, case-insensitively.
    pub fn save(
        &self,
        doc: &EnvironmentDoc,
        original_name: Option<&str>,
    ) -> Result<EnvironmentDoc> {
        let name = validate_name(&doc.name)?;
        let doc = EnvironmentDoc {
            name,
            color: doc.color.clone().filter(|c| !c.trim().is_empty()),
            icon: doc.icon.clone().filter(|c| !c.trim().is_empty()),
            variables: doc
                .variables
                .iter()
                .filter(|v| !v.key.trim().is_empty())
                .cloned()
                .collect(),
        };
        let stored = self.stored()?;
        let current = match original_name {
            Some(orig) => Some(
                Self::find(&stored, orig).ok_or_else(|| StorageError::EnvNotFound(orig.into()))?,
            ),
            None => Self::find(&stored, &doc.name),
        };
        if stored
            .iter()
            .enumerate()
            .any(|(i, s)| Some(i) != current && s.doc.name.eq_ignore_ascii_case(&doc.name))
        {
            return Err(StorageError::EnvExists(doc.name));
        }
        let old_stem = current.map(|i| stored[i].stem.as_str());
        let stem = match old_stem {
            Some(old) if stored[current.unwrap()].doc.name == doc.name => old.to_string(),
            _ => self.unique_stem(&slugify(&doc.name), old_stem),
        };
        self.write(&stem, &doc)?;
        if let Some(old) = old_stem.filter(|old| *old != stem) {
            remove_if_exists(&self.main_path(old))?;
            remove_if_exists(&self.secret_path(old))?;
        }
        Ok(doc)
    }

    pub fn delete(&self, name: &str) -> Result<()> {
        let stored = self.stored()?;
        let i = Self::find(&stored, name).ok_or_else(|| StorageError::EnvNotFound(name.into()))?;
        remove_if_exists(&self.main_path(&stored[i].stem))?;
        remove_if_exists(&self.secret_path(&stored[i].stem))
    }

    /// `name`, or `name (2)`, `name (3)`… if taken (case-insensitive).
    fn unique_name(&self, name: &str) -> Result<String> {
        let taken: Vec<String> = self
            .stored()?
            .into_iter()
            .map(|s| s.doc.name.to_lowercase())
            .collect();
        Ok((1..)
            .map(|i| {
                if i == 1 {
                    name.to_string()
                } else {
                    format!("{name} ({i})")
                }
            })
            .find(|n| !taken.contains(&n.to_lowercase()))
            .expect("unbounded iterator"))
    }
}

/// Copy environment `name` from `from` into `to` (any direction: project ↔
/// global, project ↔ project, or a duplicate within one store). The new name
/// is `new_name` (or the original), made unique with ` (2)`, ` (3)`…
pub fn copy_environment(
    from: &EnvStore,
    name: &str,
    to: &EnvStore,
    new_name: Option<&str>,
) -> Result<EnvironmentDoc> {
    let mut doc = from.get(name)?;
    let wanted = validate_name(
        new_name
            .filter(|n| !n.trim().is_empty())
            .unwrap_or(&doc.name),
    )?;
    doc.name = to.unique_name(&wanted)?;
    to.save(&doc, None)
}

/// Make sure `<ilixpatch_dir>/.gitignore` ignores environment secrets
/// (created or appended to, idempotently).
pub fn ensure_gitignore(ilixpatch_dir: &Path) -> Result<()> {
    let path = ilixpatch_dir.join(".gitignore");
    let existing = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(io_err(&path)(e)),
    };
    if existing.lines().any(|l| l.trim() == GITIGNORE_LINE) {
        return Ok(());
    }
    let mut out = existing;
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(GITIGNORE_LINE);
    out.push('\n');
    write_file(&path, &out, false)
}

// ---------- globals ----------

#[derive(Debug, Deserialize)]
struct GlobalsIn {
    #[serde(default)]
    variables: FileVars,
}

#[derive(Debug, Serialize)]
struct GlobalsOut {
    variables: Vec<FileVar>,
}

/// `<config_dir>/globals.toml`; empty if missing.
pub fn load_globals(config_dir: impl AsRef<Path>) -> Result<Vec<EnvVariable>> {
    let path = config_dir.as_ref().join(GLOBALS_FILE);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    Ok(read_vars(parse::<GlobalsIn>(&path)?.variables))
}

pub fn save_globals(config_dir: impl AsRef<Path>, variables: &[EnvVariable]) -> Result<()> {
    let path = config_dir.as_ref().join(GLOBALS_FILE);
    let vars: Vec<EnvVariable> = variables
        .iter()
        .filter(|v| !v.key.trim().is_empty())
        .cloned()
        .collect();
    let out = GlobalsOut {
        variables: write_vars(&vars, true),
    };
    write_file(&path, &toml::to_string_pretty(&out)?, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secret(key: &str, value: &str) -> EnvVariable {
        EnvVariable {
            secret: true,
            ..EnvVariable::new(key, value)
        }
    }

    fn dev() -> EnvironmentDoc {
        EnvironmentDoc {
            name: "Dev".into(),
            variables: vec![
                EnvVariable::new("base", "http://dev"),
                secret("token", "s3cret"),
                EnvVariable {
                    enabled: false,
                    ..EnvVariable::new("off", "x")
                },
                EnvVariable::new("a_last", "1"),
            ],
            color: None,
            icon: None,
        }
    }

    #[test]
    fn project_round_trip_splits_secrets() {
        let tmp = tempfile::tempdir().unwrap();
        let store = EnvStore::project(tmp.path());
        store.save(&dev(), None).unwrap();

        let main = fs::read_to_string(store.dir().join("dev.toml")).unwrap();
        assert!(main.contains("name = \"Dev\""));
        assert!(main.contains("key = \"token\""));
        assert!(!main.contains("s3cret"), "secret leaked:\n{main}");
        let secret = fs::read_to_string(store.dir().join("dev.secret.toml")).unwrap();
        assert!(secret.contains("token = \"s3cret\""));
        let gi = tmp.path().join(".ilixpatch/.gitignore");
        assert_eq!(
            fs::read_to_string(&gi).unwrap(),
            "environments/*.secret.toml\n"
        );

        // Order and flags preserved.
        assert_eq!(store.get("dev").unwrap(), dev());
        let env = store.environments().unwrap().remove(0);
        assert_eq!(
            env.variables.keys().collect::<Vec<_>>(),
            ["base", "token", "a_last"]
        );
        assert_eq!(env.variables["token"], "s3cret");

        // Gitignore is appended to, once.
        fs::write(&gi, "local/").unwrap();
        store.save(&dev(), None).unwrap();
        store.save(&dev(), None).unwrap();
        assert_eq!(
            fs::read_to_string(&gi).unwrap(),
            "local/\nenvironments/*.secret.toml\n"
        );

        // Dropping the last secret removes the secret file.
        let mut d = dev();
        d.variables.retain(|v| !v.secret);
        store.save(&d, Some("Dev")).unwrap();
        assert!(!store.dir().join("dev.secret.toml").exists());
    }

    #[test]
    fn global_keeps_secrets_inline_and_private() {
        let tmp = tempfile::tempdir().unwrap();
        let store = EnvStore::global(tmp.path());
        store.save(&dev(), None).unwrap();
        let path = tmp.path().join("environments/dev.toml");
        assert!(fs::read_to_string(&path).unwrap().contains("s3cret"));
        assert!(!tmp.path().join("environments/dev.secret.toml").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        assert_eq!(store.get("Dev").unwrap(), dev());
    }

    #[test]
    fn names_unique_rename_moves_files_delete() {
        let tmp = tempfile::tempdir().unwrap();
        let store = EnvStore::project(tmp.path());
        store.save(&dev(), None).unwrap();
        // Same name, null original → update in place.
        store.save(&dev(), None).unwrap();
        assert_eq!(store.list().unwrap().len(), 1);

        let staging = EnvironmentDoc {
            name: "Staging".into(),
            variables: vec![],
            color: None,
            icon: None,
        };
        store.save(&staging, None).unwrap();
        // Renaming onto another name (case-insensitive) is refused.
        let mut clash = dev();
        clash.name = "STAGING".into();
        assert!(matches!(
            store.save(&clash, Some("Dev")),
            Err(StorageError::EnvExists(_))
        ));
        // Case-only rename keeps the slug.
        clash.name = "DEV".into();
        store.save(&clash, Some("dev")).unwrap();
        assert!(store.dir().join("dev.toml").exists());

        let mut renamed = dev();
        renamed.name = "Local dev".into();
        store.save(&renamed, Some("DEV")).unwrap();
        assert!(!store.dir().join("dev.toml").exists());
        assert!(!store.dir().join("dev.secret.toml").exists());
        assert!(store.dir().join("local-dev.toml").exists());
        assert!(store.dir().join("local-dev.secret.toml").exists());
        let names: Vec<_> = store.list().unwrap().into_iter().map(|d| d.name).collect();
        assert_eq!(names, ["Local dev", "Staging"]);
        assert_eq!(store.get("local dev").unwrap().variables, dev().variables);

        assert!(matches!(
            store.save(&dev(), Some("nope")),
            Err(StorageError::EnvNotFound(_))
        ));
        for bad in ["", "  ", "a/b", "a\\b", "x\ny"] {
            let d = EnvironmentDoc {
                name: bad.into(),
                variables: vec![],
                color: None,
                icon: None,
            };
            assert!(
                matches!(store.save(&d, None), Err(StorageError::InvalidName(_))),
                "{bad:?}"
            );
        }

        store.delete("local dev").unwrap();
        assert!(!store.dir().join("local-dev.toml").exists());
        assert!(!store.dir().join("local-dev.secret.toml").exists());
        assert!(matches!(
            store.delete("local dev"),
            Err(StorageError::EnvNotFound(_))
        ));
    }

    #[test]
    fn slug_collision_gets_suffix() {
        let tmp = tempfile::tempdir().unwrap();
        let store = EnvStore::global(tmp.path());
        for name in ["My env", "my-env"] {
            store
                .save(
                    &EnvironmentDoc {
                        name: name.into(),
                        variables: vec![],
                        color: None,
                        icon: None,
                    },
                    None,
                )
                .unwrap();
        }
        assert!(store.dir().join("my-env.toml").exists());
        assert!(store.dir().join("my-env-2.toml").exists());
        assert_eq!(store.list().unwrap().len(), 2);
    }

    #[test]
    fn copy_both_directions() {
        let tmp = tempfile::tempdir().unwrap();
        let project = EnvStore::project(tmp.path().join("p"));
        let global = EnvStore::global(tmp.path().join("cfg"));
        project.save(&dev(), None).unwrap();

        // project → global: secrets now inline.
        let g = copy_environment(&project, "dev", &global, None).unwrap();
        assert_eq!(g, dev());
        let text = fs::read_to_string(global.dir().join("dev.toml")).unwrap();
        assert!(text.contains("s3cret"));

        // global → project, colliding name → "Dev (2)", secrets split again.
        let p = copy_environment(&global, "Dev", &project, None).unwrap();
        assert_eq!(p.name, "Dev (2)");
        assert_eq!(p.variables, dev().variables);
        let text = fs::read_to_string(project.dir().join("dev-2.toml")).unwrap();
        assert!(!text.contains("s3cret"));
        assert!(project.dir().join("dev-2.secret.toml").exists());

        // Explicit new name, and duplicate within a store.
        let n = copy_environment(&global, "dev", &global, Some("Prod")).unwrap();
        assert_eq!(n.name, "Prod");
        let n = copy_environment(&global, "dev", &global, Some("prod")).unwrap();
        assert_eq!(n.name, "prod (2)");
        assert!(copy_environment(&global, "missing", &project, None).is_err());
    }

    #[test]
    fn reads_legacy_map_and_undeclared_secrets() {
        let tmp = tempfile::tempdir().unwrap();
        let store = EnvStore::project(tmp.path());
        fs::create_dir_all(store.dir()).unwrap();
        fs::write(store.dir().join("qa.toml"), "[variables]\nhost = \"qa\"\n").unwrap();
        fs::write(
            store.dir().join("qa.secret.toml"),
            "[variables]\ntoken = \"t\"\n",
        )
        .unwrap();
        let doc = store.get("qa").unwrap();
        assert_eq!(doc.name, "qa");
        assert_eq!(
            doc.variables,
            [EnvVariable::new("host", "qa"), secret("token", "t")]
        );
    }

    #[test]
    fn globals_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(load_globals(tmp.path()).unwrap().is_empty());
        let vars = vec![
            EnvVariable::new("g", "1"),
            secret("k", "v"),
            EnvVariable::new("", "dropped"),
        ];
        save_globals(tmp.path(), &vars).unwrap();
        assert_eq!(load_globals(tmp.path()).unwrap(), vars[..2]);
        assert_eq!(enabled_map(&vars[..2])["k"], "v");
    }

    #[test]
    fn find_prefers_exact() {
        let envs = vec![
            Environment {
                name: "dev".into(),
                variables: Default::default(),
                color: None,
                icon: None,
            },
            Environment {
                name: "Dev".into(),
                variables: Default::default(),
                color: None,
                icon: None,
            },
        ];
        assert_eq!(find_environment(&envs, "Dev").unwrap().name, "Dev");
        assert_eq!(find_environment(&envs, "DEV").unwrap().name, "dev");
        assert!(find_environment(&envs, "prod").is_none());
    }

    #[test]
    fn color_and_icon_round_trip() {
        // D49: kept in the file, absent when unset (older files keep loading).
        let tmp = tempfile::tempdir().unwrap();
        let store = EnvStore::project(tmp.path());
        let doc = EnvironmentDoc {
            color: Some("ok".into()),
            icon: Some("rocket".into()),
            ..dev()
        };
        store.save(&doc, None).unwrap();
        let back = &store.list().unwrap()[0];
        assert_eq!(
            (back.color.as_deref(), back.icon.as_deref()),
            (Some("ok"), Some("rocket"))
        );
        assert_eq!(back.to_environment().icon.as_deref(), Some("rocket"));
        // Empty strings are not stored.
        let doc = EnvironmentDoc {
            color: Some(" ".into()),
            icon: None,
            ..dev()
        };
        store.save(&doc, Some("Dev")).unwrap();
        let back = &store.list().unwrap()[0];
        assert_eq!((back.color.as_deref(), back.icon.as_deref()), (None, None));
    }
}
