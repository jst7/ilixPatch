//! `ilixpatch import postman <paths…> --project <dir>` (D31).

use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::{Args, Subcommand, ValueEnum};
use ilixpatch_core::project;
use ilixpatch_importer::{
    self as importer, ApplyContext, EnvScope, OnConflict, PlanOptions, default_plan,
};

#[derive(Subcommand)]
pub enum ImportCmd {
    /// Import Postman collections, environments, globals or an "Export data" zip
    Postman(PostmanArgs),
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

#[derive(Args)]
pub struct PostmanArgs {
    /// Collection/environment/globals .json files, .zip dumps or folders
    #[arg(required = true)]
    paths: Vec<PathBuf>,
    /// Destination project (initialized if it isn't one yet)
    #[arg(long)]
    project: PathBuf,
    /// Import each collection as a folder inside this collection or folder dir
    #[arg(long)]
    into: Option<PathBuf>,
    /// Where environments go
    #[arg(long, value_enum, default_value = "project")]
    env_scope: ScopeArg,
    /// Don't import pre-request/test scripts
    #[arg(long)]
    no_scripts: bool,
    /// What to do when a file or folder already exists
    #[arg(long, value_enum, default_value = "rename")]
    on_conflict: ConflictArg,
}

pub fn run(cmd: ImportCmd) -> Result<bool> {
    let ImportCmd::Postman(args) = cmd;
    if !args.project.is_dir() {
        bail!("{}: not a directory", args.project.display());
    }
    if !project::is_project(&args.project) {
        project::init_project(&args.project)?;
        println!("initialized project in {}", args.project.display());
    }
    let parsed = importer::parse_paths(&args.paths);
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
        on_conflict: match args.on_conflict {
            ConflictArg::Rename => OnConflict::Rename,
            ConflictArg::Overwrite => OnConflict::Overwrite,
            ConflictArg::Skip => OnConflict::Skip,
        },
        keep_scripts: !args.no_scripts,
        ..PlanOptions::default()
    };
    let scope = match args.env_scope {
        ScopeArg::Project => EnvScope::Project,
        ScopeArg::Global => EnvScope::Global,
    };
    let plan = default_plan(
        &parsed,
        "cli",
        &args.project,
        args.into.as_deref(),
        scope,
        options,
    );
    let ctx = ApplyContext {
        global_dir: crate::app_config_dir(),
    };
    let report = importer::apply(&parsed, &plan, &ctx)?;
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
