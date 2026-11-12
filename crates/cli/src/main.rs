mod fmt;
mod hints;
mod import_cmd;
mod load_cmd;
mod net_args;
mod oauth2_cli;
mod run_cmd;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use ilixpatch_core::{Collection, project, storage};
use indexmap::IndexMap;

#[derive(Parser)]
#[command(name = "ilixpatch", version, about = "Fast, open-source API client")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run a collection, folder, request or whole project (Newman-compatible flags)
    Run(RunArgs),
    /// Load test a collection, folder or request with virtual users
    Load(LoadArgs),
    /// Import from other tools (Postman)
    Import {
        #[command(subcommand)]
        cmd: import_cmd::ImportCmd,
    },
    /// List the requests and environments of a collection or project
    Ls {
        /// Path to a collection directory or a project directory
        path: PathBuf,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Reporter {
    Cli,
    Json,
    Junit,
    Html,
}

#[derive(Args)]
pub struct RunArgs {
    /// Project, collection or folder directory, or a request .toml
    pub path: PathBuf,
    /// Environment: a project environment name (.ilixpatch/environments/), else a
    /// legacy collection environment name (<collection>/environments/), else a
    /// path to an environment .toml or a Postman environment .json
    #[arg(short, long)]
    pub env: Option<String>,
    /// Use a global (user-level) environment of the desktop app instead of --env
    #[arg(long, value_name = "NAME", conflicts_with = "env")]
    pub global_env: Option<String>,
    /// Don't load the desktop app's globals (globals.toml) as pm.globals
    #[arg(long)]
    pub no_globals: bool,
    /// Number of iterations (default: rows of the data file, or 1)
    #[arg(short = 'n', long)]
    pub iteration_count: Option<usize>,
    /// CSV (with header row) or JSON (array of objects) data file; one iteration per row
    #[arg(short = 'd', long)]
    pub iteration_data: Option<PathBuf>,
    /// Delay between requests, in ms
    #[arg(long, value_name = "MS", default_value_t = 0)]
    pub delay_request: u64,
    /// Per-request timeout, in ms (0 = none)
    #[arg(long, value_name = "MS", default_value_t = 0)]
    pub timeout_request: u64,
    /// Stop the run on the first failed request or test
    #[arg(long)]
    pub bail: bool,
    /// Only run requests inside this folder (or with this name); repeatable
    #[arg(long, value_name = "NAME")]
    pub folder: Vec<String>,
    /// Only run requests whose path contains this text
    #[arg(short, long)]
    pub filter: Option<String>,
    /// Reporters to use
    #[arg(short, long, value_enum, value_delimiter = ',', default_value = "cli")]
    pub reporters: Vec<Reporter>,
    #[arg(long, value_name = "PATH")]
    pub reporter_json_export: Option<PathBuf>,
    #[arg(long, value_name = "PATH")]
    pub reporter_junit_export: Option<PathBuf>,
    #[arg(long, value_name = "PATH")]
    pub reporter_html_export: Option<PathBuf>,
    /// Write the final environment (Postman JSON format) to this file
    #[arg(long, value_name = "PATH")]
    pub export_environment: Option<PathBuf>,
    /// Set a global variable; repeatable
    #[arg(long, value_name = "KEY=VALUE", value_parser = fmt::parse_kv)]
    pub global_var: Vec<(String, String)>,
    /// Set (override) an environment variable; repeatable
    #[arg(long, value_name = "KEY=VALUE", value_parser = fmt::parse_kv)]
    pub env_var: Vec<(String, String)>,
    /// Accept invalid / self-signed TLS certificates
    #[arg(short = 'k', long)]
    pub insecure: bool,
    #[command(flatten)]
    pub network: net_args::NetworkArgs,
    /// Always exit with code 0, even if requests or tests failed
    #[arg(long)]
    pub suppress_exit_code: bool,
    /// Print response headers and bodies
    #[arg(short, long)]
    pub verbose: bool,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum ProfileArg {
    Fixed,
    RampUp,
    Spike,
    Peak,
}

#[derive(Args)]
pub struct LoadArgs {
    /// Collection or folder directory, or a request .toml
    pub path: PathBuf,
    /// Number of virtual users
    #[arg(long, default_value_t = 1)]
    pub vus: usize,
    /// Test duration (e.g. 30s, 2m)
    #[arg(long, default_value = "10s", value_parser = fmt::parse_duration)]
    pub duration: std::time::Duration,
    #[arg(long, value_enum, default_value = "fixed")]
    pub profile: ProfileArg,
    /// Time to reach all VUs for ramp-up/peak (default: 25% of the duration)
    #[arg(long, value_parser = fmt::parse_duration)]
    pub ramp_up: Option<std::time::Duration>,
    /// Pause per VU between requests (e.g. 100ms)
    #[arg(long, default_value = "0ms", value_parser = fmt::parse_duration)]
    pub think_time: std::time::Duration,
    /// Environment name or file (see `run --env`)
    #[arg(short, long)]
    pub env: Option<String>,
    /// Use a global (user-level) environment of the desktop app instead of --env
    #[arg(long, value_name = "NAME", conflicts_with = "env")]
    pub global_env: Option<String>,
    /// Don't load the desktop app's globals (globals.toml) as pm.globals
    #[arg(long)]
    pub no_globals: bool,
    /// Set (override) an environment variable; repeatable
    #[arg(long, value_name = "KEY=VALUE", value_parser = fmt::parse_kv)]
    pub env_var: Vec<(String, String)>,
    /// Set a global variable; repeatable
    #[arg(long, value_name = "KEY=VALUE", value_parser = fmt::parse_kv)]
    pub global_var: Vec<(String, String)>,
    /// Don't run pre-request/test scripts (faster)
    #[arg(long)]
    pub no_scripts: bool,
    /// Per-request timeout (e.g. 10s)
    #[arg(long, default_value = "30s", value_parser = fmt::parse_duration)]
    pub timeout: std::time::Duration,
    /// Fail (exit 1) if the overall p95 latency exceeds this many ms
    #[arg(long, value_name = "MS")]
    pub max_p95: Option<f64>,
    /// Fail (exit 1) if the error rate exceeds this value (0..1)
    #[arg(long, value_name = "RATE")]
    pub max_error_rate: Option<f64>,
    /// Accept invalid / self-signed TLS certificates
    #[arg(short = 'k', long)]
    pub insecure: bool,
    #[command(flatten)]
    pub network: net_args::NetworkArgs,
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let res = match cli.command {
        Command::Ls { path } => ls_cmd(&path),
        Command::Import { cmd } => import_cmd::run(cmd),
        Command::Run(args) => run_cmd::run(args).await,
        Command::Load(args) => load_cmd::run(args).await,
    };
    match res {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::from(2)
        }
    }
}

/// A path is either a project (all its collections) or a single collection.
fn load(path: &Path) -> Result<Vec<Collection>> {
    if project::is_project(path) {
        let p = project::load_project(path)?;
        println!("project: {}", p.name);
        let envs: Vec<_> = p.environments.iter().map(|e| e.name.as_str()).collect();
        if !envs.is_empty() {
            println!("project environments: {}", envs.join(", "));
        }
        if p.collections.is_empty() {
            bail!("project '{}' has no collections", p.name);
        }
        Ok(p.collections)
    } else {
        Ok(vec![storage::load_collection(path)?])
    }
}

fn ls_cmd(path: &Path) -> Result<bool> {
    for c in load(path)? {
        println!("{} (format v{})", c.name, c.version);
        for (path, r) in c.requests() {
            println!("  {:7} {path}", r.method.as_str());
        }
        let envs: Vec<_> = c.environments.iter().map(|e| e.name.as_str()).collect();
        println!(
            "  environments: {}",
            if envs.is_empty() {
                "-".into()
            } else {
                envs.join(", ")
            }
        );
    }
    Ok(true)
}

/// The desktop app's config dir, shared with the CLI for global environments
/// and globals: `$ILIXPATCH_CONFIG_DIR`, else `<OS config dir>/dev.ilixpatch.app`
/// (exactly what Tauri's `app_config_dir()` returns for our identifier).
pub fn app_config_dir() -> Option<PathBuf> {
    match std::env::var_os("ILIXPATCH_CONFIG_DIR") {
        Some(dir) if !dir.is_empty() => Some(PathBuf::from(dir)),
        _ => dirs::config_dir().map(|d| d.join(APP_IDENTIFIER)),
    }
}

/// Tauri bundle identifier (`app/src-tauri/tauri.conf.json`).
const APP_IDENTIFIER: &str = "dev.ilixpatch.app";

/// `--env` / `--global-env` → environment selection. `--env` resolves a
/// project environment, then a legacy collection environment, then a file.
pub fn env_selection(
    targets: &[ilixpatch_runner::RunTarget],
    env: Option<&str>,
    global_env: Option<&str>,
) -> Result<Option<ilixpatch_runner::EnvSelection>> {
    use ilixpatch_runner::{EnvScope, environment_selection, select_environment};
    if let Some(name) = global_env {
        let dir = app_config_dir().context("no config directory on this system")?;
        return Ok(environment_selection(
            Some(name),
            Some(EnvScope::Global),
            Some(&dir),
        )?);
    }
    Ok(match env {
        Some(e) => Some(select_environment(targets, e)?),
        None => None,
    })
}

/// Initial globals: the app's `globals.toml` (unless `--no-globals`), then
/// `--global-var` overrides.
pub fn initial_globals(
    no_globals: bool,
    overrides: &[(String, String)],
) -> Result<IndexMap<String, String>> {
    let mut globals = match app_config_dir().filter(|_| !no_globals) {
        Some(dir) => ilixpatch_runner::load_globals_map(&dir)?,
        None => IndexMap::new(),
    };
    globals.extend(overrides.iter().cloned());
    Ok(globals)
}

/// Cancel `token` on Ctrl-C (first press); a second press exits immediately.
pub fn cancel_on_ctrl_c(token: ilixpatch_runner::CancellationToken) {
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            eprintln!("\nstopping… (press Ctrl-C again to abort)");
            token.cancel();
            if tokio::signal::ctrl_c().await.is_ok() {
                std::process::exit(130);
            }
        }
    });
}

/// OAuth 2.0 token supplier for `run`/`load` (D39): the shared token store
/// (see `oauth2_cli`), with refresh and automatic client_credentials/password fetch.
/// Token requests use `engine`, i.e. the same proxy / CA / client-certificate flags as the run.
pub fn oauth2_provider(
    engine: &ilixpatch_engine::Engine,
) -> Option<ilixpatch_runner::OAuth2Handle> {
    oauth2_cli::provider(engine.clone())
}
