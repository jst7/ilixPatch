//! `ilixpatch run`: Newman-compatible collection runner with CLI, JSON, JUnit
//! and HTML reporters.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use ilixpatch_runner::{
    CancellationToken, RequestRunResult, RunConfig, RunEvent, RunReport, filter_folders,
    load_data_file, postman_environment_json, report, resolve_target, run_collection,
};

use crate::fmt::{bold, dim, green, ms, red, size, table, yellow};
use crate::{Reporter, RunArgs, cancel_on_ctrl_c, env_selection, initial_globals};

/// Live CLI reporter state.
struct CliReporter {
    enabled: bool,
    verbose: bool,
    iterations: usize,
    multi_collection: bool,
    last_collection: Option<String>,
    last_folder: Option<String>,
    failures: Vec<(String, String, String)>,
}

impl CliReporter {
    fn on_event(&mut self, e: &RunEvent) {
        if !self.enabled {
            return;
        }
        match e {
            RunEvent::Started { .. } => println!("{}\n", bold("ilixpatch")),
            RunEvent::Iteration { iteration } => {
                if self.iterations > 1 {
                    println!(
                        "\n{}",
                        dim(&format!("Iteration {}/{}", iteration + 1, self.iterations))
                    );
                }
                self.last_folder = None;
                self.last_collection = None;
            }
            RunEvent::Request { result } => self.request(result),
            RunEvent::Finished { .. } => {}
        }
    }

    fn request(&mut self, r: &RequestRunResult) {
        let x = &r.extra;
        if self.last_collection.as_deref() != Some(&x.collection) {
            if self.multi_collection || self.last_collection.is_none() {
                println!("{}", bold(&x.collection));
            }
            self.last_collection = Some(x.collection.clone());
            self.last_folder = None;
        }
        let folder = x
            .item_path
            .rsplit_once('/')
            .map(|(f, _)| f.to_string())
            .unwrap_or_default();
        if !folder.is_empty() && self.last_folder.as_deref() != Some(&folder) {
            println!("❏ {folder}");
        }
        self.last_folder = Some(folder);

        println!("↳ {}", r.name);
        if x.skipped {
            println!("  {}", yellow("skipped by pre-request script"));
        } else {
            let detail = match r.status {
                Some(code) => {
                    let s = format!(
                        "[{code} {}, {}, {}]",
                        r.status_text.as_deref().unwrap_or(""),
                        size(r.size_bytes),
                        ms(r.elapsed_ms)
                    );
                    if (200..400).contains(&code) {
                        dim(&s)
                    } else {
                        yellow(&s)
                    }
                }
                None => red("[errored]"),
            };
            println!("  {} {} {detail}", r.method.as_str(), r.url);
        }
        for log in &r.logs {
            println!("  {} {log}", dim("│"));
        }
        let suite = format!(
            "{}{}",
            x.item_path,
            if self.iterations > 1 {
                format!(" (iteration {})", r.iteration + 1)
            } else {
                String::new()
            }
        );
        if let Some(e) = &r.error {
            if let Some(re) = &x.request_error {
                // D36: classified send failure, with likely cause and hint.
                let (title, cause, hint) = crate::hints::explain(re);
                let detail = if hint.is_empty() {
                    e.clone()
                } else {
                    format!("{e}\nhint: {hint}")
                };
                self.failures
                    .push((re.kind.as_str().into(), suite.clone(), detail));
                println!(
                    "  {}",
                    red(&format!("{}. ✗ {title} — {cause}", self.failures.len()))
                );
                if !hint.is_empty() {
                    println!("     {}", dim(hint));
                }
            } else {
                self.failures.push((
                    if x.request_failed {
                        "Error".into()
                    } else {
                        "ScriptError".into()
                    },
                    suite.clone(),
                    e.clone(),
                ));
                println!("  {}", red(&format!("{}. {e}", self.failures.len())));
            }
        }
        for t in &r.tests {
            if t.skipped {
                println!("  {}  {}", yellow("-"), dim(&t.name));
            } else if t.passed {
                println!("  {}  {}", green("✓"), t.name);
            } else {
                let msg = t.error.clone().unwrap_or_else(|| "assertion failed".into());
                self.failures.push((
                    "AssertionError".into(),
                    format!("{suite} › {}", t.name),
                    msg,
                ));
                println!("  {}", red(&format!("{}. {}", self.failures.len(), t.name)));
            }
        }
        if self.verbose {
            if let Some(h) = &x.response_headers {
                for (k, v) in h {
                    println!("  {}", dim(&format!("{k}: {v}")));
                }
            }
            if let Some(b) = &x.response_body {
                println!("{b}");
            }
        }
        println!();
    }
}

fn summary(report: &RunReport) -> String {
    let s = &report.stats;
    let failed_iterations = {
        let mut it: Vec<usize> = report
            .results
            .iter()
            .filter(|r| r.failed())
            .map(|r| r.iteration)
            .collect();
        it.dedup();
        it.len()
    };
    let row = |name: &str, exec: usize, failed: usize| {
        vec![
            name.to_string(),
            exec.to_string(),
            if failed > 0 {
                red(&failed.to_string())
            } else {
                failed.to_string()
            },
        ]
    };
    let rows = vec![
        row("iterations", s.iterations, failed_iterations),
        row("requests", s.requests, s.failed_requests),
        row("test-scripts", s.test_scripts, s.failed_test_scripts),
        row(
            "prerequest-scripts",
            s.prerequest_scripts,
            s.failed_prerequest_scripts,
        ),
        row("assertions", s.assertions, s.failed_assertions),
    ];
    let times: Vec<f64> = report
        .results
        .iter()
        .filter(|r| r.status.is_some())
        .map(|r| r.elapsed_ms)
        .collect();
    let received: usize = report.results.iter().map(|r| r.size_bytes).sum();
    let mut footer = vec![
        format!("total run duration: {}", ms(report.summary.total_ms)),
        format!("total data received: {}", size(received)),
    ];
    if !times.is_empty() {
        let n = times.len() as f64;
        let avg = times.iter().sum::<f64>() / n;
        let min = times.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = times.iter().cloned().fold(0.0, f64::max);
        let sd = (times.iter().map(|t| (t - avg).powi(2)).sum::<f64>() / n).sqrt();
        footer.push(format!(
            "average response time: {} [min: {}, max: {}, s.d.: {}]",
            ms(avg),
            ms(min),
            ms(max),
            ms(sd)
        ));
    }
    if report.summary.cancelled {
        footer.push("run cancelled".into());
    }
    table(&["", "executed", "failed"], &rows, &footer, true)
}

fn failures_table(failures: &[(String, String, String)]) -> String {
    let mut out = String::new();
    if failures.is_empty() {
        return out;
    }
    out.push_str(&format!("\n  {:<4}{:<24}{}\n", "#", "failure", "detail"));
    for (i, (kind, at, detail)) in failures.iter().enumerate() {
        out.push_str(&format!("\n  {:<4}{:<24}{}\n", i + 1, kind, at));
        for line in detail.lines() {
            out.push_str(&format!("  {:<28}{}\n", "", dim(line)));
        }
    }
    out
}

fn write_file(path: &Path, content: &str, what: &str) -> Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        fs::create_dir_all(dir).with_context(|| format!("{}", dir.display()))?;
    }
    fs::write(path, content).with_context(|| format!("{what}: {}", path.display()))?;
    eprintln!("{} {what} written to {}", dim("→"), path.display());
    Ok(())
}

fn default_export(ext: &str) -> PathBuf {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    PathBuf::from("ilixpatch").join(format!("run-{ts}.{ext}"))
}

pub async fn run(args: RunArgs) -> Result<bool> {
    let mut targets = resolve_target(&args.path)?;
    targets = filter_folders(targets, &args.folder);
    if let Some(f) = &args.filter {
        let multi = targets.len() > 1;
        for t in &mut targets {
            let cname = t.collection.name.clone();
            t.items.retain(|i| {
                let p = if multi {
                    format!("{cname}/{}", i.item_path)
                } else {
                    i.item_path.clone()
                };
                p.contains(f.as_str())
            });
        }
        targets.retain(|t| !t.items.is_empty());
    }
    if targets.iter().all(|t| t.items.is_empty()) {
        bail!("no requests matched");
    }

    let data = match &args.iteration_data {
        Some(p) => load_data_file(p)?,
        None => Vec::new(),
    };
    if args.iteration_data.is_some() && data.is_empty() && args.iteration_count.is_none() {
        bail!("iteration data file has no rows");
    }
    let config = RunConfig {
        environment: env_selection(&targets, args.env.as_deref(), args.global_env.as_deref())?,
        env_overrides: args.env_var.iter().cloned().collect(),
        globals: initial_globals(args.no_globals, &args.global_var)?,
        iterations: args.iteration_count.filter(|&n| n > 0),
        data,
        delay: Duration::from_millis(args.delay_request),
        timeout: (args.timeout_request > 0).then(|| Duration::from_millis(args.timeout_request)),
        bail: args.bail,
        persist_variables: true,
        keep_bodies: args.verbose || args.reporters.contains(&Reporter::Html),
    };

    let engine = args.network.engine(args.insecure)?;
    let cancel = CancellationToken::new();
    cancel_on_ctrl_c(cancel.clone());

    let mut cli = CliReporter {
        enabled: args.reporters.contains(&Reporter::Cli),
        verbose: args.verbose,
        iterations: config.iteration_count(),
        multi_collection: targets.len() > 1,
        last_collection: None,
        last_folder: None,
        failures: Vec::new(),
    };
    let report = run_collection(&engine, &targets, &config, &cancel, |e| cli.on_event(e)).await?;

    if cli.enabled {
        print!("{}", summary(&report));
        print!("{}", failures_table(&cli.failures));
    }

    let name = if targets.len() == 1 {
        targets[0].collection.name.clone()
    } else {
        "ilixpatch".to_string()
    };
    let wants =
        |r: Reporter, export: &Option<PathBuf>| args.reporters.contains(&r) || export.is_some();
    if wants(Reporter::Json, &args.reporter_json_export) {
        let path = args
            .reporter_json_export
            .clone()
            .unwrap_or_else(|| default_export("json"));
        write_file(&path, &report::json_report(&report), "JSON report")?;
    }
    if wants(Reporter::Junit, &args.reporter_junit_export) {
        let path = args
            .reporter_junit_export
            .clone()
            .unwrap_or_else(|| default_export("xml"));
        write_file(&path, &report::junit_report(&report, &name), "JUnit report")?;
    }
    if wants(Reporter::Html, &args.reporter_html_export) {
        let path = args
            .reporter_html_export
            .clone()
            .unwrap_or_else(|| default_export("html"));
        write_file(&path, &report::html_report(&report, &name), "HTML report")?;
    }
    if let Some(path) = &args.export_environment {
        let env_name = report
            .environment_name
            .clone()
            .unwrap_or_else(|| "environment".into());
        let json = postman_environment_json(&env_name, &report.environment);
        write_file(path, &serde_json::to_string_pretty(&json)?, "environment")?;
    }

    let s = &report.summary;
    let ok = s.failed_requests == 0 && s.failed_tests == 0 && !s.cancelled;
    Ok(ok || args.suppress_exit_code)
}
