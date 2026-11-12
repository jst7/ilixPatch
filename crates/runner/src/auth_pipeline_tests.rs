//! D39: auth applied by the runner pipeline, with inheritance, as seen by a
//! local echo server.

use std::fs;
use std::path::Path;

use ilixpatch_engine::Engine;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::{CancellationToken, EnvSelection, RunConfig, resolve_target, run_collection};

/// Answers `<target>|<authorization header or ->`.
async fn echo_server() -> String {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = l.accept().await {
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    match s.read(&mut chunk).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    }
                }
                let head = String::from_utf8_lossy(&buf).to_string();
                let target = head.split_whitespace().nth(1).unwrap_or("/").to_string();
                let auth = head
                    .lines()
                    .find_map(|l| {
                        let (k, v) = l.split_once(':')?;
                        k.eq_ignore_ascii_case("authorization")
                            .then(|| v.trim().to_string())
                    })
                    .unwrap_or_else(|| "-".into());
                let body = format!("{target}|{auth}");
                let resp = format!(
                    "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = s.write_all(resp.as_bytes()).await;
            });
        }
    });
    format!("http://{addr}")
}

fn write(path: &Path, content: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

#[tokio::test]
async fn auth_is_inherited_resolved_and_applied() {
    let base = echo_server().await;
    let dir = tempfile::tempdir().unwrap();
    let c = dir.path().join("api");
    write(
        &c.join("collection.toml"),
        &format!(
            "version = 1\nname = \"API\"\n[variables]\nbase = \"{base}\"\n[auth]\ntype = \"bearer\"\ntoken = \"{{{{token}}}}\"\n"
        ),
    );
    write(
        &c.join("environments/dev.toml"),
        "[variables]\ntoken = \"t0k\"\n",
    );
    write(
        &c.join("01-inherit.toml"),
        "name = \"Inherit\"\nurl = \"{{base}}/a\"\n",
    );
    write(
        &c.join("02-key.toml"),
        "name = \"Key\"\nurl = \"{{base}}/b?x=1\"\n[auth]\ntype = \"api_key\"\nkey = \"api_key\"\nvalue = \"{{token}}\"\nin = \"query\"\n",
    );
    write(
        &c.join("03-basic.toml"),
        "name = \"Basic\"\nurl = \"{{base}}/c\"\n[headers]\nAuthorization = \"manual\"\n[auth]\ntype = \"basic\"\nusername = \"ann\"\npassword = \"pw\"\n",
    );
    write(&c.join("open/folder.toml"), "[auth]\ntype = \"no_auth\"\n");
    write(
        &c.join("open/01-public.toml"),
        "name = \"Public\"\nurl = \"{{base}}/d\"\n",
    );

    let targets = resolve_target(&c).unwrap();
    let config = RunConfig {
        environment: Some(EnvSelection::Named("dev".into())),
        keep_bodies: true,
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
    let bodies: Vec<_> = report
        .results
        .iter()
        .map(|r| r.extra.response_body.clone().unwrap_or_default())
        .collect();
    assert_eq!(
        bodies,
        [
            "/a|Bearer t0k",
            "/b?x=1&api_key=t0k|-",
            "/c|Basic YW5uOnB3",
            "/d|-",
        ]
    );
    assert!(report.results[2].logs[0].starts_with("[warn] Basic Auth replaces"));
}
