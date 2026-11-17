//! `ilixpatch export postman <project|collection> --out <dir>` (D58).

use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Subcommand};
use ilixpatch_importer::{ExportOptions, export_postman};

#[derive(Subcommand)]
pub enum ExportCmd {
    /// Export a project (every collection and its environments) or one collection
    /// as Postman Collection v2.1 and Postman environment files
    Postman(PostmanExportArgs),
}

#[derive(Args)]
pub struct PostmanExportArgs {
    /// Project or collection directory
    path: PathBuf,
    /// Output directory (created if missing; files with the same name are overwritten)
    #[arg(short, long)]
    out: PathBuf,
    /// Write the values of secret variables (by default they are exported empty)
    #[arg(long)]
    include_secrets: bool,
}

pub fn run(cmd: ExportCmd) -> Result<bool> {
    let ExportCmd::Postman(args) = cmd;
    let report = export_postman(
        &args.path,
        &args.out,
        &ExportOptions {
            include_secrets: args.include_secrets,
        },
    )?;
    for f in &report.files {
        println!("wrote {}", f.display());
    }
    for w in &report.warnings {
        eprintln!("warning: {}: {}", w.item, w.message);
    }
    if report.secrets_omitted > 0 {
        eprintln!(
            "note: {} secret value(s) exported empty (use --include-secrets to keep them)",
            report.secrets_omitted
        );
    }
    println!(
        "exported {} collection(s), {} request(s), {} environment(s)",
        report.collections, report.requests, report.environments
    );
    Ok(true)
}
