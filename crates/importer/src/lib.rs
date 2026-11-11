//! Postman importer (D31).
//!
//! 1. [`parse_paths`] reads collection / environment / globals `.json` files,
//!    "Export data" `.zip` dumps and directories of them into a [`Parsed`] set.
//! 2. [`Parsed::preview`] describes it for the UI (tree with stable ids,
//!    environments, globals, warnings, unreadable files).
//! 3. [`apply`] writes what an [`ImportPlan`] asks for into a project:
//!    everything is validated and planned in memory first, then written.
//!
//! Conversion rules (auth, bodies, scripts, URLs…) live in `convert.rs`.

mod apply;
mod convert;
mod parse;
mod session;

use std::path::PathBuf;

use ilixpatch_core::HttpMethod;
use ilixpatch_core::environments::EnvVariable;
use serde::{Deserialize, Serialize};

pub use apply::{ApplyContext, apply, default_plan};
pub use convert::ConvertOptions;
pub use parse::{Parsed, parse_paths};
pub use session::Sessions;

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("{0}")]
    Invalid(String),
    /// Writing failed part-way; `written` lists the files already written.
    #[error("{message} (already written: {})", if .written.is_empty() { "nothing".to_string() } else { .written.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ") })]
    Partial {
        message: String,
        written: Vec<PathBuf>,
    },
}

pub type Result<T> = std::result::Result<T, ImportError>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportWarning {
    pub source_id: Option<String>,
    pub code: String,
    pub message: String,
}

impl ImportWarning {
    pub(crate) fn new(
        source_id: Option<&str>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            source_id: source_id.map(str::to_string),
            code: code.into(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeKind {
    Collection,
    Folder,
    Request,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportNode {
    pub id: String,
    pub kind: NodeKind,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<HttpMethod>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub has_scripts: bool,
    pub auth: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variables: Option<Vec<EnvVariable>>,
    pub children: Vec<ImportNode>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportEnvironment {
    pub id: String,
    pub name: String,
    pub variables: Vec<EnvVariable>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileError {
    pub file: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportPreview {
    pub session: String,
    pub collections: Vec<ImportNode>,
    pub environments: Vec<ImportEnvironment>,
    pub globals: Option<Vec<EnvVariable>>,
    pub warnings: Vec<ImportWarning>,
    pub errors: Vec<FileError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlaceAs {
    Collection,
    Folder,
    Request,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportPlacement {
    pub source_id: String,
    /// The project root (→ new collection) or a collection/folder dir.
    pub target_parent: PathBuf,
    #[serde(rename = "as")]
    pub place_as: PlaceAs,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EnvScope {
    Project,
    Global,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvPlacement {
    pub source_id: String,
    pub scope: EnvScope,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GlobalsMode {
    Skip,
    Merge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OnConflict {
    Rename,
    Overwrite,
    Skip,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanOptions {
    pub on_conflict: OnConflict,
    pub convert_auth: bool,
    pub keep_scripts: bool,
    pub inline_parent_scripts: bool,
    /// Case-insensitive regex on variable keys marking them secret.
    pub secret_pattern: Option<String>,
}

impl Default for PlanOptions {
    fn default() -> Self {
        Self {
            on_conflict: OnConflict::Rename,
            convert_auth: true,
            keep_scripts: true,
            inline_parent_scripts: true,
            secret_pattern: Some(DEFAULT_SECRET_PATTERN.into()),
        }
    }
}

/// Keys that usually hold credentials.
pub const DEFAULT_SECRET_PATTERN: &str =
    r"token|secret|passw(or)?d|pwd|api[_-]?key|private[_-]?key|credential|auth";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportPlan {
    pub session: String,
    pub project_path: PathBuf,
    pub placements: Vec<ImportPlacement>,
    #[serde(default)]
    pub excluded: Vec<String>,
    #[serde(default)]
    pub environments: Vec<EnvPlacement>,
    pub globals: GlobalsMode,
    pub options: PlanOptions,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportReport {
    pub collections: u32,
    pub folders: u32,
    pub requests: u32,
    pub environments: u32,
    pub files: Vec<PathBuf>,
    pub skipped: u32,
    pub warnings: Vec<ImportWarning>,
}

/// FNV-1a, for stable ids (same files → same ids).
pub(crate) fn stable_id(s: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}
