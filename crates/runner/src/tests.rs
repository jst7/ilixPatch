//! Integration-style tests against a tiny local HTTP server (no network).

use std::fs;
use std::path::Path;
use std::time::Duration;

use ilixpatch_engine::Engine;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::*;

/// Minimal HTTP/1.1 server: `/status/<code>`, `/slow`, anything else → 200 JSON
/// echoing the path.
async fn server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    let n = sock.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                let head = String::from_utf8_lossy(&buf).to_string();
                let path = head.split_whitespace().nth(1).unwrap_or("/").to_string();
                if path.starts_with("/slow") {
                    tokio::time::sleep(Duration::from_secs(3)).await;
                }
                let status = path
                    .strip_prefix("/status/")
                    .and_then(|s| s.parse::<u16>().ok())
                    .unwrap_or(200);
                let body = format!("{{\"path\":\"{path}\"}}");
                let resp = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes()).await;
            });
        }
    });
    format!("http://{addr}")
}

fn write(path: &Path, content: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

fn collection(base: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let c = dir.path().join("api");
    write(
        &c.join("collection.toml"),
        &format!(
            "version = 1\nname = \"API\"\n[variables]\nbase = \"{base}\"\nwho = \"collection\"\n"
        ),
    );
    write(
        &c.join("environments/dev.toml"),
        "[variables]\nwho = \"env\"\n",
    );
    write(
        &c.join("01-root.toml"),
        "name = \"Root\"\nurl = \"{{base}}/root/{{who}}\"\n",
    );
    write(
        &c.join("users/01-list.toml"),
        "name = \"List\"\nurl = \"{{base}}/users/{{id}}\"\n",
    );
    write(
        &c.join("users/admin/01-x.toml"),
        "name = \"Admin\"\nurl = \"{{base}}/status/500\"\n",
    );
    write(
        &c.join("users/02-get.toml"),
        "name = \"Get\"\nurl = \"{{base}}/users/get\"\n",
    );
    dir
}

#[tokio::test]
async fn resolves_targets_in_tree_order() {
    let dir = collection("http://x");
    let c = dir.path().join("api");
    let t = resolve_target(&c).unwrap();
    let names: Vec<_> = t[0].items.iter().map(|i| i.item_path.as_str()).collect();
    assert_eq!(
        names,
        ["Root", "users/List", "users/Get", "users/admin/Admin"]
    );

    let t = resolve_target(c.join("users")).unwrap();
    assert_eq!(t[0].items.len(), 3);
    let t = resolve_target(c.join("users/02-get.toml")).unwrap();
    assert_eq!(t[0].items.len(), 1);
    assert_eq!(t[0].collection.name, "API");

    let filtered = filter_folders(resolve_target(&c).unwrap(), &["admin".into()]);
    assert_eq!(filtered[0].items.len(), 1);
    assert!(resolve_target(dir.path().join("nope")).is_err());
}

#[tokio::test]
async fn runs_with_data_env_and_events() {
    let base = server().await;
    let dir = collection(&base);
    let c = dir.path().join("api");
    let data = dir.path().join("data.csv");
    fs::write(&data, "id\n7\n8\n").unwrap();

    let opts = RunOptions {
        target: c.to_string_lossy().into(),
        environment: Some("dev".into()),
        data_file: Some(data.to_string_lossy().into()),
        ..Default::default()
    };
    let config = RunConfig::from_options(&opts).unwrap();
    let targets = resolve_target(&opts.target).unwrap();
    let mut events = Vec::new();
    let report = run_collection(
        &Engine::new(),
        &targets,
        &config,
        &CancellationToken::new(),
        |e| events.push(serde_json::to_value(e).unwrap()),
    )
    .await
    .unwrap();

    assert_eq!(report.summary.iterations, 2);
    assert_eq!(report.summary.requests, 8);
    // Non-2xx without tests is not a failure (Newman semantics).
    assert_eq!(report.summary.failed_requests, 0);
    assert_eq!(report.results[0].url, format!("{base}/root/env"));
    assert_eq!(report.results[1].url, format!("{base}/users/7"));
    assert_eq!(report.results[5].url, format!("{base}/users/8"));
    assert_eq!(report.results[3].status, Some(500));

    assert_eq!(
        events[0],
        serde_json::json!({"type": "started", "total_requests": 8, "iterations": 2})
    );
    assert_eq!(
        events[1],
        serde_json::json!({"type": "iteration", "iteration": 0})
    );
    let req = &events[2];
    assert_eq!(req["type"], "request");
    let keys: Vec<_> = req["result"].as_object().unwrap().keys().cloned().collect();
    assert_eq!(
        keys.len(),
        13,
        "RequestRunResult must match api.ts exactly: {keys:?}"
    );
    let last = events.last().unwrap();
    assert_eq!(last["type"], "finished");
    assert_eq!(last["summary"]["requests"], 8);
    assert_eq!(last["summary"]["cancelled"], false);

    // Reporters.
    let junit = report::junit_report(&report, "API");
    assert!(junit.starts_with("<?xml"));
    assert_eq!(junit.matches("<testsuite ").count(), 8);
    let html = report::html_report(&report, "API");
    assert!(html.contains("<style>") && !html.contains("<link"));
    let json: serde_json::Value = serde_json::from_str(&report::json_report(&report)).unwrap();
    assert_eq!(json["results"][0]["item_path"], "Root");
}

#[tokio::test]
async fn bail_timeout_and_missing_vars() {
    let base = server().await;
    let dir = collection(&base);
    let c = dir.path().join("api");
    write(
        &c.join("00-slow.toml"),
        "name = \"Slow\"\nurl = \"{{base}}/slow\"\n",
    );
    let targets = resolve_target(&c).unwrap();
    let config = RunConfig {
        timeout: Some(Duration::from_millis(200)),
        bail: true,
        ..Default::default()
    };
    let report = run_collection(
        &Engine::new(),
        &targets,
        &config,
        &CancellationToken::new(),
        |_| {},
    )
    .await
    .unwrap();
    assert_eq!(
        report.results.len(),
        1,
        "bail stops after the first failure"
    );
    assert!(
        report.results[0]
            .error
            .as_deref()
            .unwrap()
            .contains("timed out")
    );
    assert_eq!(
        report.results[0].error_kind,
        Some(RequestErrorKind::Timeout)
    );
    let re = report.results[0].extra.request_error.as_ref().unwrap();
    assert_eq!(re.timeout_ms, Some(200));

    // Without bail: unresolved {{id}} is an error but the run continues.
    let report = run_collection(
        &Engine::new(),
        &filter_folders(targets, &["users".into()]),
        &RunConfig::default(),
        &CancellationToken::new(),
        |_| {},
    )
    .await
    .unwrap();
    assert_eq!(report.summary.requests, 3);
    assert_eq!(report.summary.failed_requests, 1);
    assert!(report.results[0].error.as_deref().unwrap().contains("id"));
    assert_eq!(
        report.results[0].error_kind,
        Some(RequestErrorKind::UndefinedVariables)
    );
    assert_eq!(
        report.results[0]
            .extra
            .request_error
            .as_ref()
            .unwrap()
            .variables,
        Some(vec!["id".to_string()])
    );
    let v = serde_json::to_value(&report.results[0]).unwrap();
    assert_eq!(v["error_kind"], "undefined_variables");
    let ok = serde_json::to_value(&report.results[1]).unwrap();
    assert!(ok["error_kind"].is_null());
}

#[tokio::test]
async fn cancellation_and_unknown_environment() {
    let dir = collection("http://127.0.0.1:9");
    let targets = resolve_target(dir.path().join("api")).unwrap();
    let cancel = CancellationToken::new();
    cancel.cancel();
    let report = run_collection(
        &Engine::new(),
        &targets,
        &RunConfig::default(),
        &cancel,
        |_| {},
    )
    .await
    .unwrap();
    assert!(report.summary.cancelled);
    assert_eq!(report.summary.requests, 0);

    let config = RunConfig {
        environment: Some(EnvSelection::Named("prod".into())),
        ..Default::default()
    };
    assert!(
        run_collection(
            &Engine::new(),
            &targets,
            &config,
            &CancellationToken::new(),
            |_| {}
        )
        .await
        .is_err()
    );
}

#[tokio::test]
async fn execute_request_without_scripts() {
    let base = server().await;
    let mut store = VarStore {
        environment: [("base".to_string(), base.clone())].into_iter().collect(),
        ..Default::default()
    };
    let req: ilixpatch_core::Request =
        toml::from_str("name = \"x\"\nurl = \"{{base}}/hello\"\n").unwrap();
    let res = execute_request(&Engine::new(), &req, &mut store, &ExecContext::default()).await;
    assert_eq!(res.response.unwrap().status, 200);
    assert!(res.script_error.is_none() && !res.prerequest_executed && !res.test_executed);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn load_test_short() {
    let base = server().await;
    let dir = collection(&base);
    let targets = resolve_target(dir.path().join("api/users/02-get.toml")).unwrap();
    let config = LoadConfig {
        virtual_users: 2,
        duration: Duration::from_millis(1200),
        interval: Duration::from_millis(400),
        ..Default::default()
    };
    let mut snaps = Vec::new();
    let last = run_load_test(
        &Engine::new(),
        &targets,
        &config,
        &CancellationToken::new(),
        |s| snaps.push(s.clone()),
    )
    .await
    .unwrap();
    assert!(last.is_final);
    assert!(last.requests > 0);
    assert_eq!(last.errors, 0);
    assert!(last.p95_ms > 0.0 && last.p50_ms <= last.p99_ms);
    assert_eq!(last.per_request.len(), 1);
    assert!(snaps.len() >= 2);
    assert!(!snaps[0].is_final);
    let v = serde_json::to_value(&last).unwrap();
    assert_eq!(v["final"], true);
}

#[tokio::test]
async fn scripts_set_variables_and_next_request() {
    let base = server().await;
    let dir = collection(&base);
    let c = dir.path().join("api");
    write(
        &c.join("01-root.toml"),
        r#"name = "Root"
url = "{{base}}/root"
[scripts]
tests = '''
pm.test("ok", () => pm.response.to.have.status(200));
pm.environment.set("id", "42");
postman.setNextRequest("Get");
'''
"#,
    );
    let targets = resolve_target(&c).unwrap();
    let report = run_collection(
        &Engine::new(),
        &targets,
        &RunConfig::default(),
        &CancellationToken::new(),
        |_| {},
    )
    .await
    .unwrap();
    let names: Vec<_> = report.results.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["Root", "Get", "Admin"]);
    assert_eq!(report.summary.tests, 1);
    assert_eq!(report.summary.failed_tests, 0);
    assert_eq!(report.environment["id"], "42");
}

#[tokio::test]
async fn local_variables_flow_from_pre_request_to_tests() {
    let base = server().await;
    let mut store = VarStore {
        environment: [("base".to_string(), base.clone())].into_iter().collect(),
        local: [("preset".to_string(), "p".to_string())]
            .into_iter()
            .collect(),
        ..Default::default()
    };
    let req: ilixpatch_core::Request = toml::from_str(
        r#"
name = "x"
url = "{{base}}/items/{{item}}"
[scripts]
pre_request = '''
pm.variables.set("item", "42");
pm.test("pre sees store local", () => pm.expect(pm.variables.get("preset")).to.equal("p"));
'''
tests = '''
pm.test("tests see pre-request local", () => {
  pm.expect(pm.variables.get("item")).to.equal("42");
  pm.expect(pm.variables.has("preset")).to.be.true;
  pm.expect(pm.variables.replaceIn("{{item}}")).to.equal("42");
  pm.expect(pm.response.json().path).to.equal("/items/42");
});
'''
"#,
    )
    .unwrap();
    let res = execute_request(&Engine::new(), &req, &mut store, &ExecContext::default()).await;
    assert!(res.script_error.is_none(), "{:?}", res.script_error);
    assert_eq!(res.tests.len(), 2);
    assert!(res.tests.iter().all(|t| t.passed), "{:?}", res.tests);
    assert_eq!(store.local.get("item").map(String::as_str), Some("42"));
}

/// Wrap the `collection` fixture in a project with a project-level `dev`
/// environment; the fixture's legacy `api/environments/dev.toml` stays.
fn project(base: &str) -> tempfile::TempDir {
    let dir = collection(base);
    write(
        &dir.path().join(".ilixpatch/config.toml"),
        "version = 1\nname = \"P\"\n",
    );
    ilixpatch_core::environments::EnvStore::project(dir.path())
        .save(
            &ilixpatch_core::environments::EnvironmentDoc {
                name: "Dev".into(),
                variables: vec![ilixpatch_core::environments::EnvVariable {
                    secret: true,
                    ..ilixpatch_core::environments::EnvVariable::new("who", "project")
                }],
            },
            None,
        )
        .unwrap();
    write(
        &dir.path().join("api/environments/legacy.toml"),
        "[variables]\nwho = \"legacy\"\n",
    );
    dir
}

#[test]
fn project_environment_wins_over_legacy_collection_env() {
    let dir = project("http://x");
    // Both via the project path and via the collection path.
    for path in [dir.path().to_path_buf(), dir.path().join("api/users")] {
        let t = resolve_target(&path).unwrap();
        assert_eq!(t[0].environment("dev").unwrap().variables["who"], "project");
        assert_eq!(t[0].environment("DEV").unwrap().name, "Dev");
        assert_eq!(
            t[0].environment("legacy").unwrap().variables["who"],
            "legacy"
        );
        assert!(t[0].environment("prod").is_none());
    }
    // Without a project, the legacy collection env is still used.
    let plain = collection("http://x");
    let t = resolve_target(plain.path().join("api")).unwrap();
    assert!(t[0].project_environments.is_empty());
    assert_eq!(t[0].environment("dev").unwrap().variables["who"], "env");
}

#[test]
fn cli_env_resolution_order() {
    let dir = project("http://x");
    let targets = resolve_target(dir.path()).unwrap();
    // 1. project env, 2. legacy collection env: both by name.
    assert!(
        matches!(select_environment(&targets, "dev"), Ok(EnvSelection::Named(n)) if n == "dev")
    );
    assert!(matches!(
        select_environment(&targets, "legacy"),
        Ok(EnvSelection::Named(_))
    ));
    // 3. a file path.
    let file = dir.path().join("qa.toml");
    fs::write(&file, "[variables]\nwho = \"file\"\n").unwrap();
    match select_environment(&targets, file.to_str().unwrap()).unwrap() {
        EnvSelection::Inline(e) => assert_eq!(e.variables["who"], "file"),
        other => panic!("{other:?}"),
    }
    assert!(select_environment(&targets, "nope").is_err());
}

#[test]
fn global_scope_and_globals_from_config_dir() {
    use ilixpatch_core::environments::{EnvStore, EnvVariable, EnvironmentDoc, save_globals};
    let cfg = tempfile::tempdir().unwrap();
    EnvStore::global(cfg.path())
        .save(
            &EnvironmentDoc {
                name: "Shared".into(),
                variables: vec![EnvVariable::new("who", "global-env")],
            },
            None,
        )
        .unwrap();
    save_globals(cfg.path(), &[EnvVariable::new("g", "1")]).unwrap();

    let opts = RunOptions {
        environment: Some("shared".into()),
        environment_scope: Some(EnvScope::Global),
        ..Default::default()
    };
    let config = RunConfig::from_options_in(&opts, Some(cfg.path())).unwrap();
    match &config.environment {
        Some(EnvSelection::Inline(e)) => assert_eq!(e.variables["who"], "global-env"),
        other => panic!("{other:?}"),
    }
    assert_eq!(config.globals["g"], "1");
    // Global scope without a config dir is an error; unknown names too.
    assert!(RunConfig::from_options(&opts).is_err());
    let missing = RunOptions {
        environment: Some("nope".into()),
        ..opts
    };
    assert!(RunConfig::from_options_in(&missing, Some(cfg.path())).is_err());
}

#[tokio::test]
async fn run_uses_project_environment() {
    let base = server().await;
    let dir = project(&base);
    let targets = resolve_target(dir.path().join("api/01-root.toml")).unwrap();
    let config = RunConfig {
        environment: Some(EnvSelection::Named("dev".into())),
        globals: [("who".to_string(), "globals".to_string())]
            .into_iter()
            .collect(),
        ..Default::default()
    };
    let report = run_collection(
        &Engine::new(),
        &targets,
        &config,
        &CancellationToken::new(),
        |_| {},
    )
    .await
    .unwrap();
    assert!(
        report.results[0].url.ends_with("/root/project"),
        "{}",
        report.results[0].url
    );
    assert_eq!(report.environment_name.as_deref(), Some("Dev"));
}
