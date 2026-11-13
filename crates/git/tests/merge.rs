//! Merge, pull modes and conflict resolution against real temp repos.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::*;
use ilixpatch_git::{self as git, Operation, PullMode, Side};

/// Repo whose `main` and `feature` both changed line 2 of `req.toml`.
fn diverged(base: &Path) -> (PathBuf, PathBuf) {
    let r = new_repo(&base.join("r"));
    let f = r.join("req.toml");
    write(&f, "a\nb\nc\n");
    commit_all(&r, "base");
    sh(&r, &["switch", "-q", "-c", "feature"]);
    write(&f, "a\nfeature\nc\n");
    commit_all(&r, "feature change");
    sh(&r, &["switch", "-q", "main"]);
    write(&f, "a\nmain\nc\n");
    commit_all(&r, "main change");
    (r, f)
}

#[test]
fn content_conflict_resolve_file_and_continue() {
    require_git!();
    let (_t, base) = tmp();
    let (r, f) = diverged(&base);

    let st = git::merge(&r, "feature").unwrap();
    assert_eq!(st.operation, Some(Operation::Merge));
    assert_eq!(st.conflicted, std::slice::from_ref(&f));
    assert_eq!(
        (st.ours_label.as_str(), st.theirs_label.as_str()),
        ("main", "feature")
    );
    assert!(
        st.message
            .as_deref()
            .unwrap()
            .starts_with("Merge branch 'feature'")
    );
    assert!(git::repo_info(&r).unwrap().conflicted);

    let v = git::conflict_versions(&r, &f).unwrap();
    assert_eq!(v.base.as_deref(), Some("a\nb\nc\n"));
    assert_eq!(v.ours.as_deref(), Some("a\nmain\nc\n"));
    assert_eq!(v.theirs.as_deref(), Some("a\nfeature\nc\n"));
    assert!(v.merged.unwrap().contains("<<<<<<<"));
    assert!(!v.binary);

    // Can't continue or merge again while conflicted.
    assert!(git::continue_operation(&r, None).is_err());
    assert!(git::merge(&r, "feature").is_err());

    let st = git::resolve_file(&r, &f, "a\nboth\nc\n").unwrap();
    assert!(st.conflicted.is_empty());
    assert_eq!(st.operation, Some(Operation::Merge));
    assert!(git::conflict_versions(&r, &f).is_err());

    let st = git::continue_operation(&r, Some("merged feature")).unwrap();
    assert_eq!(st.operation, None);
    assert_eq!(read(&f), "a\nboth\nc\n");
    let head = &git::log(&r, None, 1).unwrap()[0];
    assert_eq!(head.summary, "merged feature");
    assert!(git::status(&r).unwrap().is_empty());
    assert!(git::continue_operation(&r, None).is_err());
    assert!(git::abort_operation(&r).is_err());
}

#[test]
fn abort_restores_the_branch() {
    require_git!();
    let (_t, base) = tmp();
    let (r, f) = diverged(&base);
    git::merge(&r, "feature").unwrap();
    let info = git::abort_operation(&r).unwrap();
    assert!(!info.conflicted && !info.dirty);
    assert_eq!(read(&f), "a\nmain\nc\n");
    assert_eq!(git::merge_state(&r).unwrap().operation, None);
}

#[test]
fn clean_merge_and_bad_branch() {
    require_git!();
    let (_t, base) = tmp();
    let r = new_repo(&base.join("r"));
    write(&r.join("a.toml"), "1");
    commit_all(&r, "base");
    sh(&r, &["switch", "-q", "-c", "feature"]);
    write(&r.join("b.toml"), "1");
    commit_all(&r, "feature");
    sh(&r, &["switch", "-q", "main"]);
    write(&r.join("c.toml"), "1");
    commit_all(&r, "main");

    let st = git::merge(&r, "feature").unwrap();
    assert_eq!(st.operation, None);
    assert!(st.conflicted.is_empty());
    assert!(r.join("b.toml").exists());
    assert!(git::merge(&r, "does-not-exist").is_err());
    assert!(git::merge(&r, "--abort").is_err());
}

#[test]
fn modify_delete_conflict_resolved_with_each_side() {
    require_git!();
    for side in [Side::Ours, Side::Theirs] {
        let (_t, base) = tmp();
        let r = new_repo(&base.join("r"));
        let f = r.join("req.toml");
        write(&f, "a\n");
        commit_all(&r, "base");
        sh(&r, &["switch", "-q", "-c", "feature"]);
        fs::remove_file(&f).unwrap();
        commit_all(&r, "delete");
        sh(&r, &["switch", "-q", "main"]);
        write(&f, "modified\n");
        commit_all(&r, "modify");

        let st = git::merge(&r, "feature").unwrap();
        assert_eq!(st.conflicted, std::slice::from_ref(&f));
        let v = git::conflict_versions(&r, &f).unwrap();
        assert_eq!(v.base.as_deref(), Some("a\n"));
        assert_eq!(v.ours.as_deref(), Some("modified\n"));
        assert_eq!(v.theirs, None);

        let st = git::resolve_with(&r, &f, side).unwrap();
        assert!(st.conflicted.is_empty());
        match side {
            Side::Ours => assert_eq!(read(&f), "modified\n"),
            Side::Theirs => assert!(!f.exists()),
        }
        assert_eq!(git::continue_operation(&r, None).unwrap().operation, None);
        assert!(git::status(&r).unwrap().is_empty());
    }
}

#[test]
fn binary_conflict() {
    require_git!();
    let (_t, base) = tmp();
    let r = new_repo(&base.join("r"));
    let f = r.join("blob.bin");
    write(&f, b"\x00base");
    commit_all(&r, "base");
    sh(&r, &["switch", "-q", "-c", "feature"]);
    write(&f, b"\x00feature");
    commit_all(&r, "feature");
    sh(&r, &["switch", "-q", "main"]);
    write(&f, b"\x00main");
    commit_all(&r, "main");

    let st = git::merge(&r, "feature").unwrap();
    assert_eq!(st.conflicted, std::slice::from_ref(&f));
    let v = git::conflict_versions(&r, &f).unwrap();
    assert!(v.binary);
    assert!(v.ours.is_some() && v.theirs.is_some());
    git::resolve_with(&r, &f, Side::Theirs).unwrap();
    assert_eq!(fs::read(&f).unwrap(), b"\x00feature");
}

#[test]
fn rebase_conflict_uses_user_facing_sides() {
    require_git!();
    let (_t, base) = tmp();
    let (r, f) = diverged(&base);
    sh(&r, &["switch", "-q", "feature"]);
    assert!(!try_sh(&r, &["rebase", "main"]).status.success());

    let st = git::merge_state(&r).unwrap();
    assert_eq!(st.operation, Some(Operation::Rebase));
    assert_eq!(st.conflicted, std::slice::from_ref(&f));
    assert!(st.ours_label.starts_with("feature ("), "{}", st.ours_label);
    assert_eq!(st.theirs_label, "main (upstream)");
    assert_eq!(st.message.as_deref(), Some("feature change"));

    // "ours" is the user's commit being replayed, "theirs" the upstream.
    let v = git::conflict_versions(&r, &f).unwrap();
    assert_eq!(v.ours.as_deref(), Some("a\nfeature\nc\n"));
    assert_eq!(v.theirs.as_deref(), Some("a\nmain\nc\n"));
    git::resolve_with(&r, &f, Side::Ours).unwrap();
    assert_eq!(read(&f), "a\nfeature\nc\n");

    let st = git::continue_operation(&r, None).unwrap();
    assert_eq!(st.operation, None);
    let info = git::repo_info(&r).unwrap();
    assert_eq!(info.branch.as_deref(), Some("feature"));
    let log: Vec<_> = git::log(&r, None, 3)
        .unwrap()
        .into_iter()
        .map(|c| c.summary)
        .collect();
    assert_eq!(log, ["feature change", "main change", "base"]);
}

#[test]
fn pull_with_merge_and_rebase_against_local_remote() {
    require_git!();
    let (_t, base) = tmp();
    sh(&base, &["init", "-q", "--bare", "-b", "main", "remote.git"]);
    let url = format!("file://{}", base.join("remote.git").display());

    let a = new_repo(&base.join("a"));
    sh(&a, &["remote", "add", "origin", &url]);
    write(&a.join("req.toml"), "a\nb\nc\n");
    commit_all(&a, "base");
    git::push(&a).unwrap();

    sh(&base, &["clone", "-q", &url, "b"]);
    let b = dunce::canonicalize(base.join("b")).unwrap();
    configure(&b);
    write(&b.join("req.toml"), "a\nremote\nc\n");
    commit_all(&b, "remote change");
    git::push(&b).unwrap();

    write(&a.join("req.toml"), "a\nlocal\nc\n");
    commit_all(&a, "local change");

    // Fast-forward only: diverged → error, nothing in progress.
    assert!(git::pull_with(&a, PullMode::FfOnly).is_err());
    assert_eq!(git::merge_state(&a).unwrap().operation, None);

    let info = git::pull_with(&a, PullMode::Merge).unwrap();
    assert!(info.conflicted);
    let st = git::merge_state(&a).unwrap();
    assert_eq!(st.operation, Some(Operation::Merge));
    assert_eq!(st.theirs_label, "origin/main");
    assert!(git::pull_with(&a, PullMode::Merge).is_err());
    git::abort_operation(&a).unwrap();

    let info = git::pull_with(&a, PullMode::Rebase).unwrap();
    assert!(info.conflicted);
    let st = git::merge_state(&a).unwrap();
    assert_eq!(st.operation, Some(Operation::Rebase));
    assert_eq!(st.theirs_label, "origin/main (upstream)");
    let f = a.join("req.toml");
    git::resolve_file(&a, &f, "a\nlocal+remote\nc\n").unwrap();
    assert_eq!(git::continue_operation(&a, None).unwrap().operation, None);
    let info = git::repo_info(&a).unwrap();
    assert_eq!((info.ahead, info.behind, info.dirty), (1, 0, false));
}
