//! ilixPatch runner: the single-request pipeline ([`execute_request`]), a
//! Newman-compatible collection runner ([`run_collection`]), load testing
//! ([`run_load_test`]) and reporters ([`report`]).
//!
//! The serde shapes of the UI-facing types (`RunOptions`, `RunEvent`,
//! `RequestRunResult`, `RunSummary`, `LoadOptions`, `LoadSnapshot`,
//! `LoadRequestStats`) match `app/src/lib/api.ts`.

pub mod exec;
pub mod load;
pub mod report;
pub mod run;
pub mod target;
pub mod vars;

pub use exec::{ExecContext, ExecResult, execute_request};
pub use ilixpatch_engine::{RequestError, RequestErrorKind};
pub use load::{
    LoadConfig, LoadOptions, LoadProfile, LoadRequestStats, LoadSnapshot, run_load_test, target_vus,
};
pub use run::{
    EnvSelection, RequestExtra, RequestRunResult, RunConfig, RunEvent, RunOptions, RunReport,
    RunStats, RunSummary, run_collection,
};
pub use target::{
    DataRow, EnvScope, RunItem, RunTarget, environment_selection, filter_folders, load_data_file,
    load_environment_file, load_globals_map, postman_environment_json, resolve_target,
    select_environment,
};
pub use tokio_util::sync::CancellationToken;
pub use vars::VarStore;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum RunError {
    #[error("{0}")]
    Target(String),
    #[error("{0}")]
    Environment(String),
    #[error("data file {0}")]
    Data(String),
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Storage(#[from] ilixpatch_core::storage::StorageError),
}

#[cfg(test)]
mod tests;
