//! `ilixpatch mock <project|collection> [--port 4000]` (D62): serve the saved
//! example responses so a frontend can be built before the backend exists.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Args;
use ilixpatch_mock::{MockOptions, Source, start};

use crate::fmt::{bold, dim, green, red, yellow};

#[derive(Args)]
pub struct MockArgs {
    /// Project or collection directory
    pub path: PathBuf,
    /// Port to listen on
    #[arg(short, long, default_value_t = 4000)]
    pub port: u16,
    /// Address to listen on (use 0.0.0.0 to accept other machines)
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,
    /// Latency added to every response, in milliseconds (an example's own delay wins)
    #[arg(long, default_value_t = 0)]
    pub delay: u64,
}

pub async fn run(args: MockArgs) -> Result<bool> {
    let addr: SocketAddr = format!("{}:{}", args.host, args.port)
        .parse()
        .with_context(|| format!("invalid address {}:{}", args.host, args.port))?;
    let log = Arc::new(|e: ilixpatch_mock::MockLogEntry| {
        let status = e.status.to_string();
        let status = match e.status {
            200..=299 => green(&status),
            400..=599 => red(&status),
            _ => yellow(&status),
        };
        let what = match (&e.route, &e.example) {
            (Some(r), Some(x)) => dim(&format!("← {r} › {x}")),
            _ => String::new(),
        };
        println!(
            "{} {:<7} {} {status} {what}",
            dim(&format!("{:>6.1}ms", e.elapsed_ms)),
            e.method,
            e.path
        );
    });
    let server = start(
        Source::new(&args.path),
        addr,
        MockOptions {
            delay_ms: args.delay,
        },
        Some(log),
    )
    .await
    .map_err(anyhow::Error::msg)?;

    println!("{} {}", bold("Mock server on"), bold(&server.url()));
    if server.routes.is_empty() {
        println!(
            "{}",
            yellow(
                "No request has examples yet: add some ([[examples]] in a request, or \"Save as example\" in the app)."
            )
        );
    }
    for r in &server.routes {
        println!(
            "  {:<7} {:<40} {}",
            r.method,
            r.pattern,
            dim(&format!("{} [{}]", r.name, r.examples.join(", ")))
        );
    }
    println!(
        "{}",
        dim(
            "Pick an example with the X-Mock-Example header or ?__example=<name|status>. Ctrl-C to stop."
        )
    );
    tokio::signal::ctrl_c().await.ok();
    server.stop();
    Ok(true)
}
