//! Variables from a response (D48): declarative rules of a request, e.g. "save
//! `data.access_token` of the JSON body as the environment variable `token`".
//! Stored as `[[extract]]` tables in the request file; applied by the runner
//! after each send (app, Runner and CLI), like `pm.environment.set` in a script.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Extract {
    /// Variable name to set.
    pub var: String,
    #[serde(default)]
    pub from: ExtractFrom,
    /// JSON path (`data.items[0].id`), header name or regex (first group),
    /// depending on `from`; unused for `status` and `body`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub path: String,
    #[serde(default)]
    pub scope: ExtractScope,
    /// Disabled rules are kept but not applied.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtractFrom {
    /// A value of the JSON body (strings unquoted, other values as JSON).
    #[default]
    Json,
    /// A response header (case-insensitive).
    Header,
    /// The HTTP status code.
    Status,
    /// The whole body as text.
    Body,
    /// First capture group of a regex over the body (the whole match if it has none).
    Regex,
}

/// Where the value goes (a runtime value of that scope, like the scripts' `pm.*.set`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtractScope {
    #[default]
    Environment,
    Collection,
    Global,
}
