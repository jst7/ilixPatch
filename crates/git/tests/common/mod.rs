//! Helpers shared by the merge and blame tests.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

pub fn have_git() -> bool {
    Command::new("git").arg("--version").output().is_ok()
}

#[macro_export]
macro_rules! require_git {
    () => {
        if !common::have_git() {
            eprintln!("git not installed; skipping");
            return;
        }
    };
}

/// Run git in `dir` with test identity/config; returns the raw output.
pub fn try_sh_env(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new("git");
    cmd.current_dir(dir)
        .args([
            "-c",
            "user.name=Tester",
            "-c",
            "user.email=t@example.com",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .env("GIT_EDITOR", "true")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE");
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.output().unwrap()
}

pub fn sh_env(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> String {
    let out = try_sh_env(dir, args, env);
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

pub fn sh(dir: &Path, args: &[&str]) -> String {
    sh_env(dir, args, &[])
}

pub fn try_sh(dir: &Path, args: &[&str]) -> Output {
    try_sh_env(dir, args, &[])
}

pub fn configure(dir: &Path) {
    sh(dir, &["config", "user.name", "Tester"]);
    sh(dir, &["config", "user.email", "t@example.com"]);
    sh(dir, &["config", "commit.gpgsign", "false"]);
    sh(dir, &["config", "core.hooksPath", "/dev/null"]);
    sh(dir, &["config", "merge.conflictStyle", "merge"]);
}

pub fn new_repo(dir: &Path) -> PathBuf {
    fs::create_dir_all(dir).unwrap();
    sh(dir, &["init", "-q", "-b", "main"]);
    configure(dir);
    dunce::canonicalize(dir).unwrap()
}

pub fn write(path: &Path, s: impl AsRef<[u8]>) {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p).unwrap();
    }
    fs::write(path, s).unwrap();
}

pub fn commit_all(dir: &Path, msg: &str) {
    sh(dir, &["add", "-A"]);
    sh(dir, &["commit", "-q", "-m", msg]);
}

pub fn tmp() -> (tempfile::TempDir, PathBuf) {
    let t = tempfile::tempdir().unwrap();
    let p = dunce::canonicalize(t.path()).unwrap();
    (t, p)
}

pub fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap()
}
