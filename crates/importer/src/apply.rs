//! Writing an [`ImportPlan`] into a project: validate and plan everything in
//! memory, then write.

use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

use ilixpatch_core::Request;
use ilixpatch_core::auth::Auth;
use ilixpatch_core::environments::{
    self, EnvStore, EnvVariable, EnvironmentDoc, load_globals, save_globals, validate_name,
};
use ilixpatch_core::model::FORMAT_VERSION;
use ilixpatch_core::project::{self, PROJECT_DIR, ProjectConfig, slugify};
use ilixpatch_core::storage::{self, COLLECTION_FILE, ENVIRONMENTS_DIR};
use regex::Regex;

use crate::convert::{self, ConvertOptions};
use crate::parse::SrcNode;
use crate::{
    EnvPlacement, EnvScope, GlobalsMode, ImportError, ImportPlacement, ImportPlan, ImportReport,
    ImportWarning, NodeKind, OnConflict, Parsed, PlaceAs, PlanOptions, Result,
};

/// Where things go besides the project.
#[derive(Debug, Clone, Default)]
pub struct ApplyContext {
    /// App config dir, for global environments and globals. `None` refuses
    /// global-scope environments and globals merging.
    pub global_dir: Option<PathBuf>,
}

fn invalid(msg: impl Into<String>) -> ImportError {
    ImportError::Invalid(msg.into())
}

enum Op {
    Dir(PathBuf),
    Request(PathBuf, Box<Request>),
    Manifest(PathBuf, String),
    /// `folder.toml` `[auth]` of a folder.
    FolderAuth(PathBuf, Box<Auth>),
}

struct Planner<'a> {
    parsed: &'a Parsed,
    conflict: OnConflict,
    convert: ConvertOptions,
    excluded: HashSet<&'a str>,
    /// Source ids placed on their own: skipped inside an ancestor's copy.
    placed: HashSet<&'a str>,
    /// Paths created by this import (never overwritten, always renamed).
    planned: HashSet<PathBuf>,
    /// Dirs known to be collection roots (existing or planned).
    collection_roots: HashSet<PathBuf>,
    ops: Vec<Op>,
    report: ImportReport,
}

impl Planner<'_> {
    fn exists(&self, p: &Path) -> bool {
        self.planned.contains(p) || p.exists()
    }

    fn is_collection_root(&self, dir: &Path) -> bool {
        self.collection_roots.contains(dir) || dir.join(COLLECTION_FILE).is_file()
    }

    /// A free path `<parent>/<slug><ext>` honouring the conflict policy.
    /// `Ok(None)` = skipped. The bool says whether it already existed.
    fn claim(
        &mut self,
        parent: &Path,
        name: &str,
        ext: &str,
        reserved: &[&str],
    ) -> Option<(PathBuf, bool)> {
        let slug = slugify(name);
        let candidate = |i: usize| {
            let stem = if i == 1 {
                slug.clone()
            } else {
                format!("{slug}-{i}")
            };
            (
                parent.join(format!("{stem}{ext}")),
                reserved.contains(&stem.as_str()),
            )
        };
        let (first, first_reserved) = candidate(1);
        let unique = |me: &Self| {
            (2..)
                .map(candidate)
                .find(|(p, r)| !r && !me.exists(p))
                .map(|(p, _)| p)
                .expect("unbounded")
        };
        let path = if first_reserved || self.planned.contains(&first) {
            unique(self)
        } else if !first.exists() {
            first
        } else {
            match self.conflict {
                OnConflict::Rename => unique(self),
                OnConflict::Overwrite => {
                    self.planned.insert(first.clone());
                    return Some((first, true));
                }
                OnConflict::Skip => {
                    self.report.skipped += 1;
                    return None;
                }
            }
        };
        self.planned.insert(path.clone());
        Some((path, false))
    }

    fn dir(&mut self, parent: &Path, name: &str) -> Option<PathBuf> {
        let reserved: &[&str] = if self.is_collection_root(parent) {
            &[ENVIRONMENTS_DIR]
        } else {
            &[]
        };
        let (dir, existed) = self.claim(parent, name, "", reserved)?;
        if !existed {
            self.ops.push(Op::Dir(dir.clone()));
        }
        Some(dir)
    }

    /// The node's auth (`effective`: including what it inherits in Postman).
    fn auth_of(&mut self, node: &SrcNode, effective: bool) -> Auth {
        if !self.convert.convert_auth {
            return Auth::Inherit;
        }
        let src = if effective {
            node.effective_auth.as_ref()
        } else {
            node.auth.as_ref()
        };
        let Some(src) = src else {
            return Auth::Inherit;
        };
        let warnings = &mut self.report.warnings;
        crate::auth_map::map_auth(src, &mut |code, msg| {
            warnings.push(ImportWarning::new(
                Some(&node.id),
                code,
                format!("{}: {msg}", node.name),
            ));
        })
    }

    fn folder_auth(&mut self, dir: &Path, node: &SrcNode, effective: bool) {
        let auth = self.auth_of(node, effective);
        if !auth.is_inherit() {
            self.ops
                .push(Op::FolderAuth(dir.to_path_buf(), Box::new(auth)));
        }
    }

    fn request(&mut self, parent: &Path, node: &SrcNode, name: &str, effective: bool) {
        let reserved: &[&str] = if self.is_collection_root(parent) {
            &["collection"]
        } else {
            &[]
        };
        let Some((path, _)) = self.claim(parent, name, ".toml", reserved) else {
            return;
        };
        let mut req = convert::convert(node, &self.convert, effective, &mut self.report.warnings);
        req.name = name.to_string();
        self.ops.push(Op::Request(path, Box::new(req)));
        self.report.requests += 1;
    }

    /// Children of `node` (not excluded) into `dir`.
    fn children(&mut self, node: &SrcNode, dir: &Path) {
        for child in &node.children {
            if self.excluded.contains(child.id.as_str()) || self.placed.contains(child.id.as_str())
            {
                continue;
            }
            match child.kind {
                NodeKind::Request => self.request(dir, child, &child.name, false),
                _ => {
                    if let Some(sub) = self.dir(dir, &child.name) {
                        self.report.folders += 1;
                        self.folder_auth(&sub, child, false);
                        self.children(child, &sub);
                    }
                }
            }
        }
    }

    /// `node` itself (a request) or its contents, into `dir`.
    fn contents(&mut self, node: &SrcNode, dir: &Path) {
        if node.kind == NodeKind::Request {
            self.request(dir, node, &node.name, true);
        } else {
            self.children(node, dir);
        }
    }

    fn requests_flat<'n>(&self, node: &'n SrcNode, out: &mut Vec<&'n SrcNode>) {
        if self.excluded.contains(node.id.as_str()) || self.placed.contains(node.id.as_str()) {
            return;
        }
        if node.kind == NodeKind::Request {
            out.push(node);
        }
        for c in &node.children {
            self.requests_flat(c, out);
        }
    }

    fn collection_vars_dropped(&mut self, node: &SrcNode) {
        if node.kind == NodeKind::Collection && node.variables.iter().any(|v| v.enabled) {
            self.report.warnings.push(ImportWarning::new(
                Some(&node.id),
                "collection_variables",
                format!(
                    "{}: collection variables are only imported when it is placed as a collection",
                    node.name
                ),
            ));
        }
    }

    fn place(
        &mut self,
        p: &ImportPlacement,
        node: &SrcNode,
        target: &Path,
        collections_dir: &Path,
    ) -> Result<()> {
        let name = p
            .name
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .unwrap_or(&node.name)
            .to_string();
        match p.place_as {
            PlaceAs::Collection => {
                let Some((dir, existed)) =
                    self.claim(collections_dir, &name, "", &["node_modules", "target"])
                else {
                    return Ok(());
                };
                if !existed {
                    self.ops.push(Op::Dir(dir.clone()));
                }
                let mut manifest = if existed {
                    existing_manifest(&dir)?
                } else {
                    toml::Table::new()
                };
                let mut variables: toml::Table = manifest
                    .get("variables")
                    .and_then(|v| v.as_table())
                    .cloned()
                    .unwrap_or_default();
                if node.kind == NodeKind::Collection {
                    for v in node.variables.iter().filter(|v| v.enabled) {
                        variables.insert(v.key.clone(), toml::Value::String(v.value.clone()));
                    }
                }
                manifest.insert(
                    "version".into(),
                    toml::Value::Integer(i64::from(FORMAT_VERSION)),
                );
                manifest.insert("name".into(), toml::Value::String(name.clone()));
                if !variables.is_empty() {
                    manifest.insert("variables".into(), toml::Value::Table(variables));
                }
                // Collection auth: what the node uses in Postman (a request keeps its own).
                if node.kind != NodeKind::Request {
                    let auth = self.auth_of(node, true);
                    if !auth.is_inherit() {
                        let v = toml::Value::try_from(&auth).map_err(|e| invalid(e.to_string()))?;
                        manifest.insert("auth".into(), v);
                    }
                }
                let text = toml::to_string_pretty(&manifest).map_err(|e| invalid(e.to_string()))?;
                self.ops.push(Op::Manifest(dir.join(COLLECTION_FILE), text));
                self.collection_roots.insert(dir.clone());
                self.report.collections += 1;
                if node.kind == NodeKind::Request {
                    self.request(&dir, node, &name, true);
                } else {
                    self.children(node, &dir);
                }
            }
            PlaceAs::Folder => {
                self.collection_vars_dropped(node);
                if let Some(dir) = self.dir(target, &name) {
                    self.report.folders += 1;
                    if node.kind != NodeKind::Request {
                        self.folder_auth(&dir, node, true);
                    }
                    self.contents(node, &dir);
                }
            }
            PlaceAs::Request => {
                self.collection_vars_dropped(node);
                if node.kind == NodeKind::Request {
                    self.request(target, node, &name, true);
                } else {
                    let mut reqs = Vec::new();
                    for c in &node.children {
                        self.requests_flat(c, &mut reqs);
                    }
                    for r in reqs {
                        self.request(target, r, &r.name, true);
                    }
                }
            }
        }
        Ok(())
    }
}

/// An existing collection.toml as a table (kept and updated when overwriting).
fn existing_manifest(dir: &Path) -> Result<toml::Table> {
    let path = dir.join(COLLECTION_FILE);
    let Ok(text) = fs::read_to_string(&path) else {
        return Ok(toml::Table::new());
    };
    toml::from_str(&text).map_err(|e| invalid(format!("{}: {e}", path.display())))
}

fn project_paths(project_path: &Path) -> Result<(PathBuf, PathBuf)> {
    let root = fs::canonicalize(project_path)
        .map_err(|e| invalid(format!("{}: {e}", project_path.display())))?;
    let cfg_path = project::config_path(&root);
    let cfg: ProjectConfig = fs::read_to_string(&cfg_path)
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .ok_or_else(|| invalid(format!("{}: not a project", root.display())))?;
    let rel = Path::new(cfg.collections_dir.as_deref().unwrap_or("."));
    if rel
        .components()
        .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return Err(invalid(
            "collections_dir must be a relative path inside the project",
        ));
    }
    let dir = root.join(rel);
    let dir = fs::canonicalize(&dir).unwrap_or(dir);
    Ok((root, dir))
}

fn secret_regex(pattern: Option<&str>) -> Result<Option<Regex>> {
    match pattern.map(str::trim).filter(|p| !p.is_empty()) {
        None => Ok(None),
        Some(p) => Regex::new(&format!("(?i){p}"))
            .map(Some)
            .map_err(|e| invalid(format!("invalid secret pattern: {e}"))),
    }
}

fn mark_secrets(vars: &[EnvVariable], re: Option<&Regex>) -> Vec<EnvVariable> {
    vars.iter()
        .map(|v| EnvVariable {
            secret: v.secret || re.is_some_and(|re| re.is_match(&v.key)),
            ..v.clone()
        })
        .collect()
}

/// Files of `dir` with their modification times (to report what a save wrote).
fn snapshot(dir: &Path) -> Vec<(PathBuf, Option<SystemTime>)> {
    fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| (e.path(), e.metadata().ok().and_then(|m| m.modified().ok())))
                .collect()
        })
        .unwrap_or_default()
}

fn changed(before: &[(PathBuf, Option<SystemTime>)], dir: &Path) -> Vec<PathBuf> {
    snapshot(dir)
        .into_iter()
        .filter(|f| !before.contains(f))
        .map(|(p, _)| p)
        .collect()
}

struct EnvOp {
    store: EnvStore,
    doc: EnvironmentDoc,
}

/// Write `plan` into its project. `plan.project_path` must already be
/// authorized by the caller (a registered project).
pub fn apply(parsed: &Parsed, plan: &ImportPlan, ctx: &ApplyContext) -> Result<ImportReport> {
    let (root, collections_dir) = project_paths(&plan.project_path)?;
    let opts = &plan.options;
    let re = secret_regex(opts.secret_pattern.as_deref())?;
    let mut planner = Planner {
        parsed,
        conflict: opts.on_conflict,
        convert: ConvertOptions {
            convert_auth: opts.convert_auth,
            keep_scripts: opts.keep_scripts,
            inline_parent_scripts: opts.inline_parent_scripts,
        },
        excluded: plan.excluded.iter().map(String::as_str).collect(),
        placed: plan
            .placements
            .iter()
            .map(|p| p.source_id.as_str())
            .collect(),
        planned: HashSet::new(),
        collection_roots: HashSet::new(),
        ops: Vec::new(),
        report: ImportReport::default(),
    };

    // ---- validate and plan collections ----
    let mut described = HashSet::new();
    for p in &plan.placements {
        let node = planner
            .parsed
            .node(&p.source_id)
            .ok_or_else(|| invalid(format!("unknown source id {}", p.source_id)))?;
        if planner.excluded.contains(node.id.as_str()) {
            continue;
        }
        let target = fs::canonicalize(&p.target_parent)
            .map_err(|e| invalid(format!("{}: {e}", p.target_parent.display())))?;
        let rel = target
            .strip_prefix(&root)
            .map_err(|_| invalid(format!("{}: not inside the project", target.display())))?;
        if rel.components().next() == Some(Component::Normal(PROJECT_DIR.as_ref())) {
            return Err(invalid(format!(
                "{}: refusing to write inside {PROJECT_DIR}",
                target.display()
            )));
        }
        if !target.is_dir() {
            return Err(invalid(format!("{}: not a directory", target.display())));
        }
        match p.place_as {
            PlaceAs::Collection if target != root && target != collections_dir => {
                return Err(invalid(format!(
                    "{}: collections can only be created at the project root",
                    target.display()
                )));
            }
            PlaceAs::Folder | PlaceAs::Request
                if !target
                    .ancestors()
                    .take_while(|a| a.starts_with(&root))
                    .any(|a| planner.is_collection_root(a)) =>
            {
                return Err(invalid(format!(
                    "{}: folders and requests must go inside a collection",
                    target.display()
                )));
            }
            _ => {}
        }
        // Descriptions: one warning per source collection.
        if let Some(c) = parsed.collections.iter().find(|c| {
            let mut all = Vec::new();
            c.walk(&mut all);
            all.iter().any(|n| n.id == node.id)
        }) && described.insert(c.id.clone())
        {
            let mut w = Vec::new();
            collection_warnings_descriptions(c, &mut w);
            planner.report.warnings.extend(w);
        }
        planner.place(p, node, &target, &collections_dir)?;
    }

    // ---- validate environments ----
    let mut env_ops = Vec::new();
    for EnvPlacement {
        source_id,
        scope,
        name,
    } in &plan.environments
    {
        let env = parsed
            .env(source_id)
            .ok_or_else(|| invalid(format!("unknown environment id {source_id}")))?;
        let name = validate_name(
            name.as_deref()
                .filter(|n| !n.trim().is_empty())
                .unwrap_or(&env.name),
        )
        .map_err(|e| invalid(e.to_string()))?;
        let store =
            match scope {
                EnvScope::Project => EnvStore::project(&root),
                EnvScope::Global => EnvStore::global(ctx.global_dir.as_ref().ok_or_else(|| {
                    invalid("no global config directory for global environments")
                })?),
            };
        env_ops.push(EnvOp {
            store,
            doc: EnvironmentDoc {
                name,
                variables: mark_secrets(&env.variables, re.as_ref()),
            },
        });
    }
    let globals = match (plan.globals, &parsed.globals) {
        (GlobalsMode::Merge, Some(g)) => Some((
            ctx.global_dir
                .clone()
                .ok_or_else(|| invalid("no global config directory for globals"))?,
            mark_secrets(g, re.as_ref()),
        )),
        _ => None,
    };

    // ---- write ----
    let Planner {
        ops, mut report, ..
    } = planner;
    let mut written = Vec::new();
    let fail = |e: String, written: &Vec<PathBuf>| ImportError::Partial {
        message: e,
        written: written.clone(),
    };
    for op in ops {
        match op {
            Op::Dir(d) => fs::create_dir_all(&d)
                .map_err(|e| fail(format!("{}: {e}", d.display()), &written))?,
            Op::Request(path, req) => {
                storage::save_request(&path, &req).map_err(|e| fail(e.to_string(), &written))?;
                written.push(path);
            }
            Op::FolderAuth(dir, auth) => {
                storage::save_folder_auth(&dir, Some(&auth))
                    .map_err(|e| fail(e.to_string(), &written))?;
                written.push(dir.join(storage::FOLDER_FILE));
            }
            Op::Manifest(path, text) => {
                fs::write(&path, text)
                    .map_err(|e| fail(format!("{}: {e}", path.display()), &written))?;
                written.push(path);
            }
        }
    }
    for EnvOp { store, mut doc } in env_ops {
        let existing = store.list().map_err(|e| fail(e.to_string(), &written))?;
        let taken = |n: &str| existing.iter().any(|d| d.name.eq_ignore_ascii_case(n));
        if taken(&doc.name) {
            match plan.options.on_conflict {
                OnConflict::Skip => {
                    report.skipped += 1;
                    continue;
                }
                OnConflict::Overwrite => {}
                OnConflict::Rename => {
                    let base = doc.name.clone();
                    doc.name = (2..)
                        .map(|i| format!("{base} ({i})"))
                        .find(|n| !taken(n))
                        .expect("unbounded");
                }
            }
        }
        let before = snapshot(store.dir());
        let gitignore = store.dir().parent().map(|d| d.join(".gitignore"));
        let gi_before = gitignore
            .as_ref()
            .and_then(|g| fs::metadata(g).ok()?.modified().ok());
        store
            .save(&doc, None)
            .map_err(|e| fail(e.to_string(), &written))?;
        written.extend(changed(&before, store.dir()));
        if let Some(g) = gitignore
            && g.starts_with(&root)
            && fs::metadata(&g).ok().and_then(|m| m.modified().ok()) != gi_before
        {
            written.push(g);
        }
        report.environments += 1;
    }
    if let Some((dir, vars)) = globals {
        let mut current = load_globals(&dir).map_err(|e| fail(e.to_string(), &written))?;
        for v in vars {
            match current.iter_mut().find(|c| c.key == v.key) {
                Some(c) => *c = v,
                None => current.push(v),
            }
        }
        save_globals(&dir, &current).map_err(|e| fail(e.to_string(), &written))?;
        written.push(dir.join(environments::GLOBALS_FILE));
    }
    report.files = written;
    Ok(report)
}

fn collection_warnings_descriptions(c: &SrcNode, out: &mut Vec<ImportWarning>) {
    let mut all = Vec::new();
    c.walk(&mut all);
    if all.iter().any(|n| n.has_description) {
        out.push(ImportWarning::new(
            Some(&c.id),
            "description",
            format!("Descriptions in \"{}\" are not imported", c.name),
        ));
    }
}

/// Everything in `parsed` with default placements: each collection becomes a
/// new collection (or a folder inside `into`), every environment goes to
/// `env_scope`, globals are merged.
pub fn default_plan(
    parsed: &Parsed,
    session: &str,
    project_path: &Path,
    into: Option<&Path>,
    env_scope: EnvScope,
    options: PlanOptions,
) -> ImportPlan {
    let placements = parsed
        .collections
        .iter()
        .map(|c| ImportPlacement {
            source_id: c.id.clone(),
            target_parent: into.unwrap_or(project_path).to_path_buf(),
            place_as: if into.is_some() {
                PlaceAs::Folder
            } else {
                PlaceAs::Collection
            },
            name: None,
        })
        .collect();
    ImportPlan {
        session: session.to_string(),
        project_path: project_path.to_path_buf(),
        placements,
        excluded: Vec::new(),
        environments: parsed
            .environments
            .iter()
            .map(|e| EnvPlacement {
                source_id: e.id.clone(),
                scope: env_scope,
                name: None,
            })
            .collect(),
        globals: if parsed.globals.is_some() {
            GlobalsMode::Merge
        } else {
            GlobalsMode::Skip
        },
        options,
    }
}
