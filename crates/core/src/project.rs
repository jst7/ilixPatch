//! Projects: a directory (usually a git repo) with `.ilixpatch/config.toml`
//! whose subdirectories are collections.
//!
//! ```text
//! my-project/
//! ├── .ilixpatch/
//! │   └── config.toml          # version, name, optional collections_dir
//! ├── billing-api/             # a collection (has collection.toml)
//! └── users-api/
//! ```
//!
//! Also holds the filesystem operations the UI needs (create / delete items)
//! together with the guard that keeps them inside a project.

use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::{FORMAT_VERSION, HttpMethod, Project, Request, RequestFile};
use crate::storage::{
    self, COLLECTION_FILE, CollectionFile, ENVIRONMENTS_DIR, Result, StorageError, file_name,
    io_err, parse, sorted_entries,
};

pub const PROJECT_DIR: &str = ".ilixpatch";
pub const CONFIG_FILE: &str = "config.toml";
/// Directories never scanned for collections.
const SKIPPED_DIRS: &[&str] = &["node_modules", "target"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub version: u32,
    pub name: String,
    /// Relative to the project root. Defaults to `"."`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collections_dir: Option<String>,
}

pub fn config_path(root: &Path) -> PathBuf {
    root.join(PROJECT_DIR).join(CONFIG_FILE)
}

pub fn is_project(root: impl AsRef<Path>) -> bool {
    config_path(root.as_ref()).is_file()
}

fn read_config(root: &Path) -> Result<ProjectConfig> {
    let path = config_path(root);
    if !path.is_file() {
        return Err(StorageError::NotAProject(root.into()));
    }
    let config: ProjectConfig = parse(&path)?;
    if config.version > FORMAT_VERSION {
        return Err(StorageError::UnsupportedVersion {
            path,
            found: config.version,
        });
    }
    Ok(config)
}

fn collections_dir(root: &Path, config: &ProjectConfig) -> Result<PathBuf> {
    let rel = Path::new(config.collections_dir.as_deref().unwrap_or("."));
    // Keep it inside the project: no absolute paths, no `..`.
    if rel
        .components()
        .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return Err(StorageError::Forbidden {
            path: rel.into(),
            reason: "collections_dir must be a relative path inside the project",
        });
    }
    Ok(root.join(rel))
}

/// Create `.ilixpatch/config.toml` (name = folder name) if it does not exist.
pub fn init_project(root: impl AsRef<Path>) -> Result<()> {
    let root = root.as_ref();
    let root = fs::canonicalize(root).map_err(io_err(root))?;
    if !root.is_dir() {
        return Err(StorageError::Forbidden {
            path: root,
            reason: "not a directory",
        });
    }
    let path = config_path(&root);
    if path.is_file() {
        return Ok(());
    }
    let config = ProjectConfig {
        version: FORMAT_VERSION,
        name: file_name(&root).to_string(),
        collections_dir: None,
    };
    let dir = root.join(PROJECT_DIR);
    fs::create_dir_all(&dir).map_err(io_err(&dir))?;
    fs::write(&path, toml::to_string_pretty(&config)?).map_err(io_err(&path))
}

/// Load a project and every collection in it. Paths are absolute.
pub fn load_project(root: impl AsRef<Path>) -> Result<Project> {
    let root = root.as_ref();
    let root = fs::canonicalize(root).map_err(io_err(root))?;
    let config = read_config(&root)?;
    let dir = collections_dir(&root, &config)?;
    let mut collections = Vec::new();
    if dir.is_dir() {
        for path in sorted_entries(&dir)? {
            let name = file_name(&path);
            if name.starts_with('.') || SKIPPED_DIRS.contains(&name) || !path.is_dir() {
                continue;
            }
            if path.join(COLLECTION_FILE).is_file() {
                collections.push(storage::load_collection(&path)?);
            }
        }
    }
    let environments = crate::environments::EnvStore::project(&root).environments()?;
    Ok(Project {
        name: config.name,
        version: config.version,
        path: root,
        collections,
        environments,
    })
}

/// Turn a display name into a file-system friendly slug:
/// `"List Users!"` -> `"list-users"`.
pub fn slugify(name: &str) -> String {
    let mut slug = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            slug.push(c);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        "untitled".into()
    } else {
        slug
    }
}

/// First free `<dir>/<slug><ext>`, `<dir>/<slug>-2<ext>`, ... skipping `reserved` stems.
fn unique_path(dir: &Path, slug: &str, ext: &str, reserved: &[&str]) -> PathBuf {
    (1..)
        .map(|i| {
            if i == 1 {
                slug.to_string()
            } else {
                format!("{slug}-{i}")
            }
        })
        .filter(|stem| !reserved.contains(&stem.as_str()))
        .map(|stem| dir.join(format!("{stem}{ext}")))
        .find(|p| !p.exists())
        .expect("unbounded iterator")
}

fn check_name(name: &str) -> Result<&str> {
    let name = name.trim();
    if name.is_empty() {
        return Err(StorageError::InvalidName(name.into()));
    }
    Ok(name)
}

fn existing_dir(path: &Path) -> Result<PathBuf> {
    let dir = fs::canonicalize(path).map_err(io_err(path))?;
    if !dir.is_dir() {
        return Err(StorageError::Forbidden {
            path: dir,
            reason: "not a directory",
        });
    }
    Ok(dir)
}

fn is_collection_root(dir: &Path) -> bool {
    dir.join(COLLECTION_FILE).is_file()
}

/// Create a collection in the project's collections dir. Returns its directory.
pub fn create_collection(project_root: impl AsRef<Path>, name: &str) -> Result<PathBuf> {
    let name = check_name(name)?;
    let root = existing_dir(project_root.as_ref())?;
    let config = read_config(&root)?;
    let parent = collections_dir(&root, &config)?;
    let dir = unique_path(&parent, &slugify(name), "", SKIPPED_DIRS);
    fs::create_dir_all(&dir).map_err(io_err(&dir))?;
    let manifest = dir.join(COLLECTION_FILE);
    let meta = CollectionFile {
        version: FORMAT_VERSION,
        name: name.into(),
        variables: Default::default(),
        auth: Default::default(),
    };
    fs::write(&manifest, toml::to_string_pretty(&meta)?).map_err(io_err(&manifest))?;
    Ok(dir)
}

/// Create a folder inside a collection or folder directory. Returns its path.
pub fn create_folder(parent: impl AsRef<Path>, name: &str) -> Result<PathBuf> {
    let name = check_name(name)?;
    let parent = existing_dir(parent.as_ref())?;
    let reserved: &[&str] = if is_collection_root(&parent) {
        &[ENVIRONMENTS_DIR]
    } else {
        &[]
    };
    let dir = unique_path(&parent, &slugify(name), "", reserved);
    fs::create_dir(&dir).map_err(io_err(&dir))?;
    Ok(dir)
}

/// Create a `GET` request with an empty URL inside a collection or folder directory.
pub fn create_request(folder: impl AsRef<Path>, name: &str) -> Result<RequestFile> {
    let name = check_name(name)?;
    let folder = existing_dir(folder.as_ref())?;
    // `collection.toml` / `folder.toml` are not requests.
    let reserved: &[&str] = if is_collection_root(&folder) {
        &["collection", "folder"]
    } else {
        &["folder"]
    };
    let path = unique_path(&folder, &slugify(name), ".toml", reserved);
    let request = Request {
        name: name.into(),
        method: HttpMethod::Get,
        url: String::new(),
        headers: Default::default(),
        query: Default::default(),
        body: None,
        scripts: Default::default(),
        settings: Default::default(),
        auth: Default::default(),
        extract: Default::default(),
    };
    storage::save_request(&path, &request)?;
    Ok(RequestFile { path, request })
}

/// Canonicalize `path` (which may not exist yet; its parent must) and require it
/// to be strictly inside one of `roots`, not inside `.ilixpatch`.
/// Returns the canonical path.
pub fn resolve_inside<P: AsRef<Path>>(roots: &[P], path: impl AsRef<Path>) -> Result<PathBuf> {
    let path = path.as_ref();
    let forbidden = |reason| StorageError::Forbidden {
        path: path.into(),
        reason,
    };
    let canonical = match fs::canonicalize(path) {
        Ok(p) => p,
        Err(_) => {
            let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
                return Err(forbidden("invalid path"));
            };
            fs::canonicalize(parent).map_err(io_err(parent))?.join(name)
        }
    };
    for root in roots {
        let Ok(root) = fs::canonicalize(root.as_ref()) else {
            continue;
        };
        if let Ok(rel) = canonical.strip_prefix(&root) {
            if rel.as_os_str().is_empty() {
                return Err(forbidden("refusing to touch the project root"));
            }
            if rel.components().next() == Some(Component::Normal(PROJECT_DIR.as_ref())) {
                return Err(forbidden("refusing to touch .ilixpatch"));
            }
            return Ok(canonical);
        }
    }
    Err(forbidden("not inside a registered project"))
}

/// Delete a request file or a folder/collection directory. The path must be
/// strictly inside one of `roots` (see [`resolve_inside`]).
pub fn delete_item<P: AsRef<Path>>(roots: &[P], path: impl AsRef<Path>) -> Result<()> {
    let path = resolve_inside(roots, path)?;
    let meta = fs::symlink_metadata(&path).map_err(io_err(&path))?;
    if meta.is_dir() {
        fs::remove_dir_all(&path).map_err(io_err(&path))
    } else if meta.is_file() && path.extension().is_some_and(|e| e == "toml") {
        fs::remove_file(&path).map_err(io_err(&path))
    } else {
        Err(StorageError::Forbidden {
            path,
            reason: "only .toml files and directories can be deleted",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap().join("My API");
        fs::create_dir(&root).unwrap();
        init_project(&root).unwrap();
        (dir, root)
    }

    #[test]
    fn slugs() {
        assert_eq!(slugify("List Users!"), "list-users");
        assert_eq!(slugify("  --Get / user--  "), "get-user");
        assert_eq!(slugify("../../etc"), "etc");
        assert_eq!(slugify("Ünïcode Café"), "ünïcode-café");
        assert_eq!(slugify("!!!"), "untitled");
    }

    #[test]
    fn init_is_idempotent_and_uses_folder_name() {
        let (_tmp, root) = project();
        let p = load_project(&root).unwrap();
        assert_eq!(p.name, "My API");
        assert_eq!(p.version, FORMAT_VERSION);
        assert_eq!(p.path, root);
        assert!(p.collections.is_empty());

        fs::write(config_path(&root), "version = 1\nname = \"Renamed\"\n").unwrap();
        init_project(&root).unwrap();
        assert_eq!(load_project(&root).unwrap().name, "Renamed");
    }

    #[test]
    fn rejects_non_project() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            load_project(dir.path()),
            Err(StorageError::NotAProject(_))
        ));
    }

    #[test]
    fn create_and_load_tree() {
        let (_tmp, root) = project();
        let c = create_collection(&root, "Users API").unwrap();
        assert_eq!(c, root.join("users-api"));
        assert_eq!(
            create_collection(&root, "Users API").unwrap(),
            root.join("users-api-2")
        );

        let f = create_folder(&c, "Admin").unwrap();
        assert_eq!(
            create_folder(&c, "environments").unwrap(),
            c.join("environments-2")
        );
        let r = create_request(&f, "List users").unwrap();
        assert_eq!(r.path, f.join("list-users.toml"));
        assert_eq!(
            create_request(&f, "List users").unwrap().path,
            f.join("list-users-2.toml")
        );
        assert_eq!(
            create_request(&c, "Collection").unwrap().path,
            c.join("collection-2.toml")
        );

        // Ignored: hidden dirs, node_modules, plain dirs without collection.toml.
        for skip in [".hidden", "node_modules", "docs"] {
            fs::create_dir_all(root.join(skip)).unwrap();
        }
        fs::write(
            root.join("node_modules").join(COLLECTION_FILE),
            "version = 1\nname = \"x\"\n",
        )
        .unwrap();

        let p = load_project(&root).unwrap();
        let names: Vec<_> = p.collections.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Users API", "Users API"]);
        let col = &p.collections[0];
        assert_eq!(col.path, c);
        assert_eq!(col.root.path, c);
        let admin = &col.root.folders[0];
        assert_eq!(admin.path, f);
        assert_eq!(admin.requests.len(), 2);
        assert!(admin.requests.contains(&r));
        assert_eq!(col.root.requests[0].request.name, "Collection");
    }

    #[test]
    fn collections_dir_is_respected() {
        let (_tmp, root) = project();
        fs::write(
            config_path(&root),
            "version = 1\nname = \"P\"\ncollections_dir = \"apis\"\n",
        )
        .unwrap();
        let c = create_collection(&root, "One").unwrap();
        assert_eq!(c, root.join("apis/one"));
        assert_eq!(load_project(&root).unwrap().collections.len(), 1);

        fs::write(
            config_path(&root),
            "version = 1\nname = \"P\"\ncollections_dir = \"../x\"\n",
        )
        .unwrap();
        assert!(load_project(&root).is_err());
    }

    #[test]
    fn empty_name_is_rejected() {
        let (_tmp, root) = project();
        assert!(matches!(
            create_collection(&root, "   "),
            Err(StorageError::InvalidName(_))
        ));
    }

    #[test]
    fn delete_guards() {
        let (tmp, root) = project();
        let roots = [root.clone()];
        let c = create_collection(&root, "C").unwrap();
        let f = create_folder(&c, "F").unwrap();
        let r = create_request(&f, "R").unwrap();
        let outside = tmp.path().join("outside.toml");
        fs::write(&outside, "").unwrap();
        let txt = c.join("notes.txt");
        fs::write(&txt, "").unwrap();

        assert!(delete_item(&roots, &root).is_err());
        assert!(delete_item(&roots, root.join(".")).is_err());
        assert!(delete_item(&roots, root.join(PROJECT_DIR)).is_err());
        assert!(delete_item(&roots, config_path(&root)).is_err());
        assert!(delete_item(&roots, &outside).is_err());
        assert!(delete_item(&roots, c.join("../../outside.toml")).is_err());
        assert!(delete_item(&roots, &txt).is_err());
        assert!(delete_item(&[tmp.path().join("nope")], &r.path).is_err());
        assert!(outside.exists() && txt.exists() && is_project(&root));

        delete_item(&roots, &r.path).unwrap();
        assert!(!r.path.exists());
        delete_item(&roots, &f).unwrap();
        assert!(!f.exists());
        delete_item(&roots, &c).unwrap();
        assert!(!c.exists());
    }

    #[test]
    fn resolve_inside_allows_new_files() {
        let (_tmp, root) = project();
        let c = create_collection(&root, "C").unwrap();
        let new = c.join("new.toml");
        assert_eq!(resolve_inside(&[&root], &new).unwrap(), new);
        assert!(resolve_inside(&[&root], root.join(PROJECT_DIR).join("x.toml")).is_err());
    }

    #[test]
    fn loads_project_and_legacy_collection_environments() {
        use crate::environments::{EnvStore, EnvVariable, EnvironmentDoc};
        let (_tmp, root) = project();
        let c = create_collection(&root, "C").unwrap();
        fs::create_dir_all(c.join(ENVIRONMENTS_DIR)).unwrap();
        fs::write(
            c.join("environments/legacy.toml"),
            "[variables]\nhost = \"legacy\"\n",
        )
        .unwrap();
        let store = EnvStore::project(&root);
        for name in ["staging", "Dev"] {
            let doc = EnvironmentDoc {
                name: name.into(),
                variables: vec![
                    EnvVariable::new("host", name),
                    EnvVariable {
                        enabled: false,
                        ..EnvVariable::new("off", "x")
                    },
                ],
                color: None,
                icon: None,
            };
            store.save(&doc, None).unwrap();
        }
        let p = load_project(&root).unwrap();
        let names: Vec<_> = p.environments.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["Dev", "staging"]);
        assert_eq!(p.environments[0].variables.len(), 1, "disabled excluded");
        assert_eq!(p.collections[0].environments[0].name, "legacy");
        assert_eq!(p.collections[0].environments[0].variables["host"], "legacy");
        // .ilixpatch is never picked up as a collection.
        assert_eq!(p.collections.len(), 1);
    }
}
