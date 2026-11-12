use std::path::PathBuf;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::auth::Auth;

/// Current version of the on-disk format. Bump when making breaking changes.
pub const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    #[default]
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
}

impl HttpMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
            Self::Head => "HEAD",
            Self::Options => "OPTIONS",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Body {
    Json { content: String },
    Text { content: String },
    Form { fields: IndexMap<String, String> },
}

/// A single HTTP request. Stored as one `.toml` file per request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub name: String,
    #[serde(default)]
    pub method: HttpMethod,
    pub url: String,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub headers: IndexMap<String, String>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub query: IndexMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<Body>,
    /// Postman-compatible JavaScript (`pm.*` API), run by `ilixpatch-scripting`.
    #[serde(default, skip_serializing_if = "Scripts::is_empty")]
    pub scripts: Scripts,
    /// Per-request network overrides (D29), `[settings]` in the file.
    #[serde(default, skip_serializing_if = "crate::RequestSettings::is_empty")]
    pub settings: crate::RequestSettings,
    /// Authorization (D39), `[auth]` in the file. `Inherit` (the default) is omitted.
    #[serde(default, skip_serializing_if = "Auth::is_inherit")]
    pub auth: Auth,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Scripts {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pre_request: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tests: Option<String>,
}

impl Scripts {
    pub fn is_empty(&self) -> bool {
        self.pre_request
            .as_deref()
            .is_none_or(|s| s.trim().is_empty())
            && self.tests.as_deref().is_none_or(|s| s.trim().is_empty())
    }
}

/// A set of variables, e.g. `dev`, `staging`, `prod`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Environment {
    /// Derived from the file name; serialized for the UI but never read from disk.
    #[serde(skip_deserializing)]
    pub name: String,
    #[serde(default)]
    pub variables: IndexMap<String, String>,
}

/// A request together with the absolute path of the `.toml` file it lives in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequestFile {
    pub path: PathBuf,
    pub request: Request,
}

/// A folder inside a collection. Folders can be nested.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Folder {
    pub name: String,
    /// Absolute directory path.
    pub path: PathBuf,
    /// Folder auth from `folder.toml` (D39); `Inherit` when absent. Root folder: unused (see Collection.auth).
    #[serde(default)]
    pub auth: Auth,
    pub requests: Vec<RequestFile>,
    pub folders: Vec<Folder>,
}

/// A collection is a directory containing a `collection.toml`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Collection {
    pub name: String,
    pub version: u32,
    /// Absolute directory path (contains `collection.toml`).
    pub path: PathBuf,
    /// Collection-level variables (lowest priority).
    pub variables: IndexMap<String, String>,
    /// Collection auth from `collection.toml` `[auth]` (D39); `Inherit` here means "no auth".
    #[serde(default)]
    pub auth: Auth,
    pub environments: Vec<Environment>,
    /// Top-level folder; `root.path == path`.
    pub root: Folder,
}

impl Collection {
    pub fn environment(&self, name: &str) -> Option<&Environment> {
        self.environments.iter().find(|e| e.name == name)
    }

    /// Iterate over every request with its path inside the collection
    /// (e.g. `users/List users`).
    pub fn requests(&self) -> Vec<(String, &Request)> {
        fn walk<'a>(folder: &'a Folder, prefix: &str, out: &mut Vec<(String, &'a Request)>) {
            for r in &folder.requests {
                out.push((format!("{prefix}{}", r.request.name), &r.request));
            }
            for f in &folder.folders {
                walk(f, &format!("{prefix}{}/", f.name), out);
            }
        }
        let mut out = Vec::new();
        walk(&self.root, "", &mut out);
        out
    }
}

/// A project is a directory (usually a git repo) containing
/// `.ilixpatch/config.toml`. Its collections live in subdirectories.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub version: u32,
    /// Absolute path of the project root.
    pub path: PathBuf,
    pub collections: Vec<Collection>,
    /// Project environments (`.ilixpatch/environments/`), secrets merged in,
    /// disabled variables excluded, sorted by name.
    #[serde(default)]
    pub environments: Vec<Environment>,
}
