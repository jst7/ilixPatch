//! `ilixpatch load`: load testing with live progress and CI thresholds.

use anyhow::Result;
use ilixpatch_runner::{
    CancellationToken, LoadConfig, LoadProfile, LoadSnapshot, resolve_target, run_load_test,
};

use crate::fmt::{bold, green, ms, red, table};
use crate::{LoadArgs, ProfileArg, cancel_on_ctrl_c, env_selection, initial_globals};

fn live_line(s: &LoadSnapshot) -> String {
    format!(
        "{:>5.0}s  vus {:>4}  reqs {:>7}  rps {:>8.1}  avg {:>8}  p95 {:>8}  errors {} ({:.1}%)",
        s.elapsed_secs,
        s.active_vus,
        s.requests,
        s.rps,
        ms(s.avg_ms),
        ms(s.p95_ms),
        s.errors,
        s.error_rate * 100.0
    )
}

fn final_table(s: &LoadSnapshot) -> String {
    let mut rows: Vec<Vec<String>> = s
        .per_request
        .iter()
        .map(|r| {
            vec![
                format!("{} {}", r.method.as_str(), r.name),
                r.count.to_string(),
                if r.errors > 0 {
                    red(&r.errors.to_string())
                } else {
                    "0".into()
                },
                ms(r.avg_ms),
                ms(r.p95_ms),
            ]
        })
        .collect();
    rows.push(vec![
        bold("total"),
        s.requests.to_string(),
        if s.errors > 0 {
            red(&s.errors.to_string())
        } else {
            "0".into()
        },
        ms(s.avg_ms),
        ms(s.p95_ms),
    ]);
    let footer = vec![
        format!(
            "latency  p50 {}  p90 {}  p95 {}  p99 {}  max {}",
            ms(s.p50_ms),
            ms(s.p90_ms),
            ms(s.p95_ms),
            ms(s.p99_ms),
            ms(s.max_ms)
        ),
        format!(
            "throughput {:.1} req/s over {:.1}s · error rate {:.2}%",
            s.rps,
            s.elapsed_secs,
            s.error_rate * 100.0
        ),
    ];
    table(
        &["request", "count", "errors", "avg", "p95"],
        &rows,
        &footer,
        false,
    )
}

pub async fn run(args: LoadArgs) -> Result<bool> {
    if args.vus == 0 {
        anyhow::bail!("--vus must be at least 1");
    }
    if args.duration.is_zero() {
        anyhow::bail!("--duration must be > 0");
    }
    if let Some(r) = args.max_error_rate
        && !(0.0..=1.0).contains(&r)
    {
        anyhow::bail!("--max-error-rate must be between 0 and 1");
    }
    let targets = resolve_target(&args.path)?;
    // Built first: OAuth 2.0 token requests reuse it (proxy, CA, client-cert flags).
    let engine = args.network.engine(args.insecure)?;
    let config = LoadConfig {
        environment: env_selection(&targets, args.env.as_deref(), args.global_env.as_deref())?,
        env_overrides: args.env_var.iter().cloned().collect(),
        globals: initial_globals(args.no_globals, &args.global_var)?,
        virtual_users: args.vus,
        duration: args.duration,
        ramp_up: args.ramp_up.filter(|d| !d.is_zero()),
        profile: match args.profile {
            ProfileArg::Fixed => LoadProfile::Fixed,
            ProfileArg::RampUp => LoadProfile::RampUp,
            ProfileArg::Spike => LoadProfile::Spike,
            ProfileArg::Peak => LoadProfile::Peak,
        },
        think_time: args.think_time,
        run_scripts: !args.no_scripts,
        timeout: (!args.timeout.is_zero()).then_some(args.timeout),
        oauth2: crate::oauth2_provider(&engine),
        ..Default::default()
    };
    let stop = CancellationToken::new();
    cancel_on_ctrl_c(stop.clone());

    let requests: usize = targets.iter().map(|t| t.items.len()).sum();
    println!(
        "{} {} request(s) · {} VUs · {:.0?} · profile {}{}\n",
        bold("ilixpatch load"),
        requests,
        args.vus,
        args.duration,
        serde_json::to_value(config.profile)?
            .as_str()
            .unwrap_or_default(),
        if config.run_scripts {
            ""
        } else {
            " · no scripts"
        }
    );
    let last = run_load_test(&engine, &targets, &config, &stop, |s| {
        if !s.is_final {
            println!("{}", live_line(s));
        }
    })
    .await?;
    println!();
    print!("{}", final_table(&last));

    let mut ok = true;
    if let Some(max) = args.max_p95 {
        if last.p95_ms > max {
            println!(
                "{}",
                red(&format!(
                    "✗ p95 {} exceeds --max-p95 {}",
                    ms(last.p95_ms),
                    ms(max)
                ))
            );
            ok = false;
        } else {
            println!(
                "{}",
                green(&format!("✓ p95 {} ≤ {}", ms(last.p95_ms), ms(max)))
            );
        }
    }
    if let Some(max) = args.max_error_rate {
        if last.error_rate > max {
            println!(
                "{}",
                red(&format!(
                    "✗ error rate {:.4} exceeds --max-error-rate {max}",
                    last.error_rate
                ))
            );
            ok = false;
        } else {
            println!(
                "{}",
                green(&format!("✓ error rate {:.4} ≤ {max}", last.error_rate))
            );
        }
    }
    if last.requests == 0 {
        println!("{}", red("✗ no requests completed"));
        ok = false;
    }
    Ok(ok)
}
