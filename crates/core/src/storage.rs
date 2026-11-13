//! On-disk format of a collection. One file per request so diffs and merges
//! stay clean in git. Collections live inside a project, see [`crate::project`].
//!
//! ```text
//! my-api/
//! ├── collection.toml          # name, version, collection variables
//! ├── environments/
//! │   ├── dev.toml             # [variables]
//! │   └── dev.secret.toml      # gitignored, merged over dev.toml
//! └── users/                   # folder
//!     └── list-users.toml      # one request
//! ```

use std::fs;
use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::auth::Auth;
use crate::model::{Collection, Environment, FORMAT_VERSION, Folder, Request, RequestFile};

pub const COLLECTION_FILE: &str = "collection.toml";
/// Optional per-folder settings (`[auth]`, D39). Never a request file.
pub const FOLDER_FILE: &str = "folder.toml";
pub const ENVIRONMENTS_DIR: &str = "environments";
pub const SECRET_SUFFIX: &str = ".secret.toml";

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path}: {source}")]
    Parse {
        path: PathBuf,
        // Boxed: on Windows `PathBuf` is 32 bytes and the unboxed variant exceeds
        // clippy's `result_large_err` threshold (128 bytes), failing CI there only.
        source: Box<toml::de::Error>,
    },
    #[error("{0}: not a collection (missing {COLLECTION_FILE})")]
    NotACollection(PathBuf),
    #[error(
        "{path}: unsupported format version {found} (this build supports up to {FORMAT_VERSION})"
    )]
    UnsupportedVersion { path: PathBuf, found: u32 },
    #[error("{0}: not a project (missing .ilixpatch/config.toml)")]
    NotAProject(PathBuf),
    #[error("{path}: {reason}")]
    Forbidden { path: PathBuf, reason: &'static str },
    #[error("invalid name: {0:?}")]
    InvalidName(String),
    #[error("environment not found: {0:?}")]
    EnvNotFound(String),
    #[error("an environment named {0:?} already exists")]
    EnvExists(String),
    #[error("serialize: {0}")]
    Serialize(#[from] toml::ser::Error),
    #[error("{path}: {message}")]
    Edit { path: PathBuf, message: String },
}

pub type Result<T> = std::result::Result<T, StorageError>;

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct CollectionFile {
    pub(crate) version: u32,
    pub(crate) name: String,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub(crate) variables: IndexMap<String, String>,
    /// Collection auth (D39); `Inherit` (= no auth) is omitted.
    #[serde(default, skip_serializing_if = "Auth::is_inherit")]
    pub(crate) auth: Auth,
}

/// `folder.toml`.
#[derive(Debug, Default, Serialize, Deserialize)]
struct FolderFile {
    #[serde(default, skip_serializing_if = "Auth::is_inherit")]
    auth: Auth,
}

pub(crate) fn io_err(path: &Path) -> impl FnOnce(std::io::Error) -> StorageError + '_ {
    move |source| StorageError::Io {
        path: path.into(),
        source,
    }
}

fn read(path: &Path) -> Result<String> {
    fs::read_to_string(path).map_err(|source| StorageError::Io {
        path: path.into(),
        source,
    })
}

pub(crate) fn parse<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    toml::from_str(&read(path)?).map_err(|source| StorageError::Parse {
        path: path.into(),
        source: Box::new(source),
    })
}

pub(crate) fn sorted_entries(dir: &Path) -> Result<Vec<PathBuf>> {
    let io = |source| StorageError::Io {
        path: dir.into(),
        source,
    };
    let mut entries = fs::read_dir(dir)
        .map_err(io)?
        .map(|e| e.map(|e| e.path()))
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(io)?;
    entries.sort();
    Ok(entries)
}

pub(crate) fn file_name(path: &Path) -> &str {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
}

/// Load a collection. All paths in the result are absolute (canonicalized).
pub fn load_collection(dir: impl AsRef<Path>) -> Result<Collection> {
    let dir = dir.as_ref();
    let dir = &fs::canonicalize(dir).map_err(io_err(dir))?;
    let manifest = dir.join(COLLECTION_FILE);
    if !manifest.is_file() {
        return Err(StorageError::NotACollection(dir.into()));
    }
    let meta: CollectionFile = parse(&manifest)?;
    if meta.version > FORMAT_VERSION {
        return Err(StorageError::UnsupportedVersion {
            path: manifest,
            found: meta.version,
        });
    }

    let mut root = load_folder(dir, true)?;
    root.name = meta.name.clone();

    Ok(Collection {
        name: meta.name,
        version: meta.version,
        path: dir.clone(),
        variables: meta.variables,
        environments: load_environments(&dir.join(ENVIRONMENTS_DIR))?,
        auth: meta.auth,
        root,
    })
}

fn load_folder(dir: &Path, is_root: bool) -> Result<Folder> {
    let mut folder = Folder {
        name: file_name(dir).to_string(),
        path: dir.to_path_buf(),
        ..Default::default()
    };
    let folder_file = dir.join(FOLDER_FILE);
    if !is_root && folder_file.is_file() {
        let f: FolderFile = parse(&folder_file)?;
        folder.auth = f.auth;
    }
    for path in sorted_entries(dir)? {
        let name = file_name(&path);
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            if is_root && name == ENVIRONMENTS_DIR {
                continue;
            }
            folder.folders.push(load_folder(&path, false)?);
        } else if name.ends_with(".toml")
            && !name.ends_with(SECRET_SUFFIX)
            && !(is_root && name == COLLECTION_FILE)
            && name != FOLDER_FILE
        {
            let request = parse(&path)?;
            folder.requests.push(RequestFile { path, request });
        }
    }
    Ok(folder)
}

fn load_environments(dir: &Path) -> Result<Vec<Environment>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut envs = Vec::new();
    for path in sorted_entries(dir)? {
        let name = file_name(&path);
        if !name.ends_with(".toml") || name.ends_with(SECRET_SUFFIX) {
            continue;
        }
        let stem = name.trim_end_matches(".toml");
        let mut env: Environment = parse(&path)?;
        env.name = stem.to_string();
        let secrets = dir.join(format!("{stem}{SECRET_SUFFIX}"));
        if secrets.is_file() {
            let secret: Environment = parse(&secrets)?;
            env.variables.extend(secret.variables);
        }
        envs.push(env);
    }
    Ok(envs)
}

fn read_doc(path: &Path) -> Result<toml_edit::DocumentMut> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(source) => {
            return Err(StorageError::Io {
                path: path.into(),
                source,
            });
        }
    };
    text.parse::<toml_edit::DocumentMut>()
        .map_err(|e| StorageError::Edit {
            path: path.into(),
            message: e.to_string(),
        })
}

/// A serializable value as a `toml_edit` table (for `[auth]`, `[variables]`).
fn to_table<T: Serialize>(value: &T, path: &Path) -> Result<toml_edit::Table> {
    let text = toml::to_string(value)?;
    let doc = text
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| StorageError::Edit {
            path: path.into(),
            message: e.to_string(),
        })?;
    let mut table = doc.as_table().clone();
    table.set_implicit(false);
    table.set_position(None);
    Ok(table)
}

/// Set (`Some`, not `Inherit`) or remove a top-level table in a TOML file,
/// keeping everything else (comments, order, formatting). Returns the
/// edited document.
fn set_table<T: Serialize>(
    doc: &mut toml_edit::DocumentMut,
    key: &str,
    value: Option<&T>,
    path: &Path,
) -> Result<()> {
    match value {
        Some(v) => {
            doc.insert(key, toml_edit::Item::Table(to_table(v, path)?));
        }
        None => {
            doc.remove(key);
        }
    }
    Ok(())
}

fn require_collection(collection: &Path) -> Result<PathBuf> {
    let manifest = collection.join(COLLECTION_FILE);
    if !manifest.is_file() {
        return Err(StorageError::NotACollection(collection.into()));
    }
    Ok(manifest)
}

/// Set or clear (`None` / `Inherit` = no auth) the `[auth]` of a collection,
/// preserving the rest of `collection.toml`.
pub fn save_collection_auth(collection: impl AsRef<Path>, auth: Option<&Auth>) -> Result<()> {
    let manifest = require_collection(collection.as_ref())?;
    let mut doc = read_doc(&manifest)?;
    set_table(
        &mut doc,
        "auth",
        auth.filter(|a| !a.is_inherit()),
        &manifest,
    )?;
    fs::write(&manifest, doc.to_string()).map_err(io_err(&manifest))
}

/// Set or clear (`None` / `Inherit`) a folder's auth in `<folder>/folder.toml`.
/// The file is removed when nothing else is left in it.
pub fn save_folder_auth(folder: impl AsRef<Path>, auth: Option<&Auth>) -> Result<()> {
    let folder = folder.as_ref();
    if !folder.is_dir() {
        return Err(StorageError::Forbidden {
            path: folder.into(),
            reason: "not a folder",
        });
    }
    if folder.join(COLLECTION_FILE).is_file() {
        return Err(StorageError::Forbidden {
            path: folder.into(),
            reason: "this is a collection: use its collection auth",
        });
    }
    let file = folder.join(FOLDER_FILE);
    let mut doc = read_doc(&file)?;
    set_table(&mut doc, "auth", auth.filter(|a| !a.is_inherit()), &file)?;
    if doc.as_table().is_empty() && doc.to_string().trim().is_empty() {
        match fs::remove_file(&file) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(io_err(&file)(e)),
        }
    } else {
        fs::write(&file, doc.to_string()).map_err(io_err(&file))
    }
}

/// Replace the `[variables]` of a collection, preserving the rest of
/// `collection.toml`.
pub fn save_collection_variables(
    collection: impl AsRef<Path>,
    variables: &IndexMap<String, String>,
) -> Result<()> {
    let manifest = require_collection(collection.as_ref())?;
    let mut doc = read_doc(&manifest)?;
    set_table(
        &mut doc,
        "variables",
        (!variables.is_empty()).then_some(variables),
        &manifest,
    )?;
    fs::write(&manifest, doc.to_string()).map_err(io_err(&manifest))
}

/// Write a request to `path`, creating parent folders if needed.
pub fn save_request(path: impl AsRef<Path>, request: &Request) -> Result<()> {
    let path = path.as_ref();
    let io = |source| StorageError::Io {
        path: path.into(),
        source,
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(io)?;
    }
    fs::write(path, toml::to_string_pretty(request)?).map_err(io)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::HttpMethod;

    #[test]
    fn round_trip_collection() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(
            root.join(COLLECTION_FILE),
            "version = 1\nname = \"Demo\"\n[variables]\nbase = \"http://x\"\n",
        )
        .unwrap();
        fs::create_dir(root.join(ENVIRONMENTS_DIR)).unwrap();
        fs::write(
            root.join("environments/dev.toml"),
            "[variables]\ntoken = \"public\"\nhost = \"dev\"\n",
        )
        .unwrap();
        fs::write(
            root.join("environments/dev.secret.toml"),
            "[variables]\ntoken = \"s3cret\"\n",
        )
        .unwrap();

        let req = Request {
            name: "List".into(),
            method: HttpMethod::Get,
            url: "{{base}}/users".into(),
            headers: Default::default(),
            query: Default::default(),
            body: None,
            scripts: Default::default(),
            settings: Default::default(),
            auth: Default::default(),
            extract: Default::default(),
        };
        save_request(root.join("users/list.toml"), &req).unwrap();

        let c = load_collection(root).unwrap();
        assert_eq!(c.name, "Demo");
        let canon = fs::canonicalize(root).unwrap();
        assert_eq!(c.path, canon);
        assert_eq!(c.root.path, canon);
        assert_eq!(c.root.folders[0].path, canon.join("users"));
        assert_eq!(c.root.folders[0].requests[0].request, req);
        assert_eq!(
            c.root.folders[0].requests[0].path,
            canon.join("users/list.toml")
        );
        let dev = c.environment("dev").unwrap();
        assert_eq!(dev.variables["token"], "s3cret");
        assert_eq!(dev.variables["host"], "dev");
        assert_eq!(c.environments.len(), 1);
        assert_eq!(c.requests()[0].0, "users/List");
    }

    #[test]
    fn rejects_non_collection() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            load_collection(dir.path()),
            Err(StorageError::NotACollection(_))
        ));
    }
}
