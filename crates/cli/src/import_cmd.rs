//! `ilixpatch import postman|openapi <sources…> --project <dir>` (D31, D53).

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use clap::{Args, Subcommand, ValueEnum};
use ilixpatch_core::project;
use ilixpatch_importer::{
    self as importer, ApplyContext, EnvScope, GroupBy, MAX_FILE, OnConflict, ParseOptions, Parsed,
    PlanOptions, default_plan,
};

use crate::net_args::NetworkArgs;

/// Arguments are boxed: their sizes differ a lot (network flags), and paths
/// are larger on Windows, where clippy's `large_enum_variant` fires (D40).
#[derive(Subcommand)]
pub enum ImportCmd {
    /// Import Postman collections, environments, globals or an "Export data" zip
    Postman(Box<PostmanArgs>),
    /// Import OpenAPI 3.x / Swagger 2.0 specs (JSON or YAML), from files or URLs
    #[command(name = "openapi", alias = "swagger")]
    OpenApi(Box<OpenApiArgs>),
}

#[derive(Clone, Copy, ValueEnum)]
pub enum ScopeArg {
    Project,
    Global,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum ConflictArg {
    Rename,
    Overwrite,
    Skip,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum GroupByArg {
    Tags,
    Path,
}

/// Destination options shared by every importer.
#[derive(Args)]
pub struct Destination {
    /// Destination project (initialized if it isn't one yet)
    #[arg(long)]
    project: PathBuf,
    /// Import each collection as a folder inside this collection or folder dir
    #[arg(long)]
    into: Option<PathBuf>,
    /// What to do when a file or folder already exists
    #[arg(long, value_enum, default_value = "rename")]
    on_conflict: ConflictArg,
}

#[derive(Args)]
pub struct PostmanArgs {
    /// Collection/environment/globals .json files, .zip dumps or folders
    #[arg(required = true)]
    paths: Vec<PathBuf>,
    #[command(flatten)]
    dest: Destination,
    /// Where environments go
    #[arg(long, value_enum, default_value = "project")]
    env_scope: ScopeArg,
    /// Don't import pre-request/test scripts
    #[arg(long)]
    no_scripts: bool,
}

#[derive(Args)]
pub struct OpenApiArgs {
    /// Spec files (.json, .yaml, .yml), folders of them, or http(s) URLs
    #[arg(required = true, value_name = "FILE|URL")]
    sources: Vec<String>,
    #[command(flatten)]
    dest: Destination,
    /// Folders: one per tag, or one per URL path segment
    #[arg(long, value_enum, default_value = "tags")]
    group_by: GroupByArg,
    /// Accept invalid / self-signed TLS certificates when downloading
    #[arg(short = 'k', long)]
    insecure: bool,
    #[command(flatten)]
    network: NetworkArgs,
}

fn is_url(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

pub async fn run(cmd: ImportCmd) -> Result<bool> {
    match cmd {
        ImportCmd::Postman(args) => {
            prepare(&args.dest.project)?;
            let parsed = importer::parse_paths(&args.paths);
            finish(&parsed, &args.dest, args.env_scope, !args.no_scripts)
        }
        ImportCmd::OpenApi(args) => {
            prepare(&args.dest.project)?;
            let options = ParseOptions {
                group_by: match args.group_by {
                    GroupByArg::Tags => GroupBy::Tags,
                    GroupByArg::Path => GroupBy::Path,
                },
            };
            let mut parsed = Parsed::new(options);
            let engine = if args.sources.iter().any(|s| is_url(s)) {
                Some(args.network.engine(args.insecure)?)
            } else {
                None
            };
            for source in &args.sources {
                match &engine {
                    Some(engine) if is_url(source) => {
                        eprintln!("downloading {source}");
                        match engine.download(source, MAX_FILE).await {
                            Ok(bytes) => parsed.add_download(source, &bytes),
                            Err(e) => parsed.add_error(source, e),
                        }
                    }
                    _ => parsed.add_path(Path::new(source)),
                }
            }
            finish(&parsed, &args.dest, ScopeArg::Project, true)
        }
    }
}

fn prepare(project_dir: &Path) -> Result<()> {
    if !project_dir.is_dir() {
        bail!("{}: not a directory", project_dir.display());
    }
    if !project::is_project(project_dir) {
        project::init_project(project_dir)?;
        println!("initialized project in {}", project_dir.display());
    }
    Ok(())
}

/// Report unreadable sources, then write everything into the project.
fn finish(
    parsed: &Parsed,
    dest: &Destination,
    env_scope: ScopeArg,
    keep_scripts: bool,
) -> Result<bool> {
    let preview = parsed.preview("cli");
    for e in &preview.errors {
        eprintln!("error: {}: {}", e.file, e.message);
    }
    if preview.collections.is_empty()
        && preview.environments.is_empty()
        && preview.globals.is_none()
    {
        bail!("nothing to import");
    }
    let options = PlanOptions {
        on_conflict: match dest.on_conflict {
            ConflictArg::Rename => OnConflict::Rename,
            ConflictArg::Overwrite => OnConflict::Overwrite,
            ConflictArg::Skip => OnConflict::Skip,
        },
        keep_scripts,
        ..PlanOptions::default()
    };
    let scope = match env_scope {
        ScopeArg::Project => EnvScope::Project,
        ScopeArg::Global => EnvScope::Global,
    };
    let plan = default_plan(
        parsed,
        "cli",
        &dest.project,
        dest.into.as_deref(),
        scope,
        options,
    );
    let ctx = ApplyContext {
        global_dir: crate::app_config_dir(),
    };
    let report = importer::apply(parsed, &plan, &ctx)?;
    println!(
        "imported {} collection(s), {} folder(s), {} request(s), {} environment(s); skipped {}",
        report.collections, report.folders, report.requests, report.environments, report.skipped
    );
    for f in &report.files {
        println!("  {}", f.display());
    }
    for w in &report.warnings {
        println!("warning [{}]: {}", w.code, w.message);
    }
    Ok(preview.errors.is_empty())
}
