//! Cloning against local remotes (file:// and a tiny dumb-HTTP server).

mod common;

use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::*;
use ilixpatch_git::{self as git, CloneCancel, CloneEvent, CloneOptions};

/// Bare repo `remote.git` with commits on `main` and `dev`.
fn bare_remote(base: &Path) -> PathBuf {
    let work = new_repo(&base.join("work"));
    for i in 0..20 {
        write(
            &work.join(format!("req{i}.toml")),
            format!("name = \"{i}\"\n").repeat(50),
        );
    }
    commit_all(&work, "one");
    sh(&work, &["switch", "-q", "-c", "dev"]);
    for i in 0..3 {
        write(&work.join("dev.toml"), format!("{i}"));
        commit_all(&work, &format!("dev {i}"));
    }
    sh(&work, &["switch", "-q", "main"]);
    sh(base, &["clone", "-q", "--bare", "work", "remote.git"]);
    let bare = dunce::canonicalize(base.join("remote.git")).unwrap();
    sh(&bare, &["update-server-info"]);
    bare
}

fn opts(url: &str, parent: &Path) -> CloneOptions {
    CloneOptions {
        url: url.into(),
        parent_dir: parent.into(),
        ..Default::default()
    }
}

#[test]
fn clones_with_progress_branch_and_depth() {
    require_git!();
    let (_t, base) = tmp();
    let bare = bare_remote(&base);
    let url = format!("file://{}", bare.display());
    let dest = base.join("dest");
    fs::create_dir_all(&dest).unwrap();

    let mut events = Vec::new();
    let path =
        git::clone_repo(&opts(&url, &dest), &CloneCancel::new(), |e| events.push(e)).unwrap();
    assert_eq!(path, dest.join("remote"));
    assert!(path.join("req0.toml").exists());
    assert_eq!(
        events.last(),
        Some(&CloneEvent::Done { path: path.clone() })
    );
    assert!(
        events.iter().any(|e| matches!(
            e,
            CloneEvent::Progress {
                percent: Some(_),
                ..
            }
        )),
        "{events:?}"
    );
    let info = git::repo_info(&path).unwrap();
    assert_eq!(info.upstream.as_deref(), Some("origin/main"));

    let o = CloneOptions {
        dir_name: Some("shallow".into()),
        branch: Some("dev".into()),
        depth: Some(1),
        ..opts(&url, &dest)
    };
    let path = git::clone_repo(&o, &CloneCancel::new(), |_| {}).unwrap();
    assert_eq!(
        git::repo_info(&path).unwrap().branch.as_deref(),
        Some("dev")
    );
    assert_eq!(sh(&path, &["rev-list", "--count", "HEAD"]).trim(), "1");

    // Into an existing empty directory.
    fs::create_dir_all(dest.join("empty")).unwrap();
    let o = CloneOptions {
        dir_name: Some("empty".into()),
        ..opts(&url, &dest)
    };
    assert!(git::clone_repo(&o, &CloneCancel::new(), |_| {}).is_ok());
}

#[test]
fn rejects_bad_names_targets_and_urls() {
    require_git!();
    let (_t, base) = tmp();
    let bare = bare_remote(&base);
    let url = format!("file://{}", bare.display());
    let none = CloneCancel::new();

    for name in ["a/b", "..", "."] {
        let o = CloneOptions {
            dir_name: Some(name.into()),
            ..opts(&url, &base)
        };
        assert!(git::clone_repo(&o, &none, |_| {}).is_err(), "{name}");
    }
    // Existing non-empty target is left untouched.
    write(&base.join("taken/keep.txt"), "x");
    let o = CloneOptions {
        dir_name: Some("taken".into()),
        ..opts(&url, &base)
    };
    assert!(git::clone_repo(&o, &none, |_| {}).is_err());
    assert!(base.join("taken/keep.txt").exists());
    // Missing parent.
    assert!(git::clone_repo(&opts(&url, &base.join("nope")), &none, |_| {}).is_err());

    let marker = base.join("pwned");
    for evil in [
        format!("--upload-pack=touch {}", marker.display()),
        format!("ext::sh -c touch% {}", marker.display()),
        "-oProxyCommand=touch /tmp/x".to_string(),
    ] {
        let o = CloneOptions {
            dir_name: Some("evil".into()),
            ..opts(&evil, &base)
        };
        assert!(git::clone_repo(&o, &none, |_| {}).is_err(), "{evil}");
    }
    assert!(!marker.exists());
    // Credentials only over HTTPS.
    let o = CloneOptions {
        token: Some("t".into()),
        ..opts(&url, &base)
    };
    assert!(git::clone_repo(&o, &none, |_| {}).is_err());
}

#[test]
fn cancel_kills_git_and_removes_partial_clone() {
    require_git!();
    let (_t, base) = tmp();
    // Accepts connections (via the backlog) but never answers: git hangs.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!(
        "http://127.0.0.1:{}/repo.git",
        listener.local_addr().unwrap().port()
    );
    let cancel = CloneCancel::new();
    let c2 = cancel.clone();
    let o = opts(&url, &base);
    let handle = std::thread::spawn(move || git::clone_repo(&o, &c2, |_| {}));
    std::thread::sleep(Duration::from_millis(500));
    let start = Instant::now();
    cancel.cancel();
    let res = handle.join().unwrap();
    assert!(start.elapsed() < Duration::from_secs(5));
    assert!(res.unwrap_err().to_string().contains("cancelled"));
    assert!(!base.join("repo").exists());
    drop(listener);
}

/// Minimal dumb-HTTP server for `bare`, recording request headers.
fn serve(bare: PathBuf) -> (u16, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen2 = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let mut req = Vec::new();
            let mut buf = [0u8; 1024];
            while !req.windows(4).any(|w| w == b"\r\n\r\n") {
                match s.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => req.extend_from_slice(&buf[..n]),
                }
            }
            let req = String::from_utf8_lossy(&req).into_owned();
            seen2.lock().unwrap().push(req.clone());
            let path = req
                .split(' ')
                .nth(1)
                .unwrap_or("/")
                .split('?')
                .next()
                .unwrap()
                .to_string();
            let file = path
                .strip_prefix("/repo.git/")
                .map(|rel| bare.join(rel))
                .filter(|p| p.is_file() && !rel_escapes(&path));
            let resp = match file.and_then(|f| fs::read(f).ok()) {
                Some(body) => {
                    let mut r = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                    .into_bytes();
                    r.extend(body);
                    r
                }
                None => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    .to_vec(),
            };
            let _ = s.write_all(&resp);
        }
    });
    (port, seen)
}

fn rel_escapes(path: &str) -> bool {
    path.contains("..")
}

#[test]
fn credentials_are_sent_but_never_persisted_or_shown() {
    require_git!();
    let (_t, base) = tmp();
    let bare = bare_remote(&base);
    let (port, seen) = serve(bare);
    let dest = base.join("dest");
    fs::create_dir_all(&dest).unwrap();

    let o = CloneOptions {
        username: Some("me".into()),
        token: Some("s3cret-token".into()),
        ..opts(&format!("http://127.0.0.1:{port}/repo.git"), &dest)
    };
    let path = git::clone_repo(&o, &CloneCancel::new(), |_| {}).unwrap();
    assert!(path.join("req0.toml").exists());
    // base64("me:s3cret-token")
    let header = "Authorization: Basic bWU6czNjcmV0LXRva2Vu";
    assert!(
        seen.lock().unwrap().iter().any(|r| r.contains(header)),
        "header not sent"
    );
    let config = fs::read_to_string(path.join(".git/config")).unwrap();
    assert!(
        !config.contains("s3cret")
            && !config.contains("bWU6")
            && !config.to_lowercase().contains("extraheader")
    );

    // A failing clone never echoes the token.
    let o = CloneOptions {
        username: Some("me".into()),
        token: Some("s3cret-token".into()),
        ..opts(&format!("http://127.0.0.1:{port}/missing.git"), &dest)
    };
    let err = git::clone_repo(&o, &CloneCancel::new(), |_| {})
        .unwrap_err()
        .to_string();
    assert!(!err.contains("s3cret") && !err.contains("bWU6"), "{err}");
    assert!(!dest.join("missing").exists());
}
