//! ilixPatch core: the data model, `{{variable}}` resolution and the
//! git-friendly on-disk format. This crate knows nothing about HTTP or UI.

pub mod auth;
pub mod environments;
pub mod extract;
pub mod model;
pub mod project;
pub mod request_settings;
pub mod storage;
pub mod vars;

pub use auth::Auth;
pub use extract::{Extract, ExtractFrom, ExtractScope};
pub use model::{
    Body, Collection, Environment, Folder, HttpMethod, Project, Request, RequestFile, Scripts,
};
pub use request_settings::{HttpVersionPref, RequestSettings};
pub use vars::{Scope, VarError};

#[cfg(test)]
mod auth_tests;
