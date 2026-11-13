//! Integration tests against real temp repos, built with the git CLI.
//! Every test is skipped (passes) when `git` isn't installed.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use ilixpatch_git::{self as git, FileState};

fn have_git() -> bool {
    Command::new("git").arg("--version").output().is_ok()
}

macro_rules! require_git {
    () => {
        if !have_git() {
            eprintln!("git not installed; skipping");
            return;
        }
    };
}

/// Run git in `dir` with a deterministic identity; panics on failure.
fn sh(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
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
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Local config so commits made by the crate (plain `git`) work anywhere.
fn configure(dir: &Path) {
    sh(dir, &["config", "user.name", "Tester"]);
    sh(dir, &["config", "user.email", "t@example.com"]);
    sh(dir, &["config", "commit.gpgsign", "false"]);
    sh(dir, &["config", "core.hooksPath", "/dev/null"]);
}

fn new_repo(dir: &Path) -> PathBuf {
    fs::create_dir_all(dir).unwrap();
    sh(dir, &["init", "-q", "-b", "main"]);
    configure(dir);
    dunce::canonicalize(dir).unwrap()
}

fn write(path: &Path, s: &str) {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p).unwrap();
    }
    fs::write(path, s).unwrap();
}

fn commit_all(dir: &Path, msg: &str) {
    sh(dir, &["add", "-A"]);
    sh(dir, &["commit", "-q", "-m", msg]);
}

fn tmp() -> (tempfile::TempDir, PathBuf) {
    let t = tempfile::tempdir().unwrap();
    let p = dunce::canonicalize(t.path()).unwrap();
    (t, p)
}

fn states(root: &Path) -> Vec<(String, FileState, bool)> {
    let mut v: Vec<_> = git::status(root)
        .unwrap()
        .into_iter()
        .map(|e| {
            let rel = e.path.strip_prefix(root).unwrap();
            (rel.to_string_lossy().replace('\\', "/"), e.state, e.staged)
        })
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

#[test]
fn discovers_nested_and_enclosing_repos() {
    require_git!();
    let (_t, base) = tmp();

    // Project that isn't a repo, with two repo collections (one with a nested repo).
    let proj = base.join("proj");
    let a = new_repo(&proj.join("a"));
    let b = new_repo(&proj.join("b"));
    let a_sub = new_repo(&proj.join("a/folder/sub"));
    new_repo(&proj.join("node_modules/dep"));
    new_repo(&proj.join(".hidden/x"));
    new_repo(&proj.join("b/target/y"));
    // Worktree/submodule style: `.git` is a file.
    write(&proj.join("c/.git"), "gitdir: /nonexistent\n");
    fs::create_dir_all(proj.join("plain/deeper")).unwrap();

    let roots = git::discover_repos(&proj).unwrap();
    let c = dunce::canonicalize(proj.join("c")).unwrap();
    assert_eq!(roots, [a.clone(), a_sub.clone(), b.clone(), c]);
    // The broken `.git` file can't be opened and is left out of `repos`.
    let infos = git::repos(&proj).unwrap();
    let got: Vec<_> = infos.iter().map(|i| i.root.clone()).collect();
    assert_eq!(got, [a, a_sub, b]);
    assert!(infos.iter().all(|i| i.branch.as_deref() == Some("main")));

    // Project inside a repo, with a nested collection repo.
    let outer = new_repo(&base.join("outer"));
    let proj2 = outer.join("apis/proj2");
    let col = new_repo(&proj2.join("col"));
    write(&proj2.join("other/x.toml"), "x");
    assert_eq!(git::discover_repos(&proj2).unwrap(), [outer.clone(), col]);

    // The project root itself is a repo: listed once.
    assert_eq!(git::discover_repos(&outer).unwrap()[0], outer);
    assert_eq!(git::enclosing_repo(proj2.join("other/x.toml")), Some(outer));
}

#[test]
fn branch_detached_and_empty_repo() {
    require_git!();
    let (_t, base) = tmp();
    let r = new_repo(&base.join("r"));

    let info = git::repo_info(&r).unwrap();
    assert_eq!(info.branch.as_deref(), Some("main"));
    assert_eq!(info.head, None);
    assert!(!info.dirty);

    write(&r.join("a.toml"), "1");
    let info = git::repo_info(&r).unwrap();
    assert!(info.dirty);
    commit_all(&r, "one");
    write(&r.join("a.toml"), "2");
    commit_all(&r, "two");
    let info = git::repo_info(&r).unwrap();
    assert_eq!(info.branch.as_deref(), Some("main"));
    let full = sh(&r, &["rev-parse", "HEAD"]);
    assert!(full.starts_with(info.head.as_deref().unwrap()));
    assert!(!info.dirty && !info.conflicted);
    assert_eq!((info.upstream, info.ahead, info.behind), (None, 0, 0));

    sh(&r, &["checkout", "-q", "--detach", "HEAD~1"]);
    let info = git::repo_info(&r).unwrap();
    assert_eq!(info.branch, None);
    assert!(info.head.is_some());

    // Not a repo root.
    fs::create_dir_all(r.join("sub")).unwrap();
    assert!(git::repo_info(r.join("sub")).is_err());
}

#[test]
fn status_reports_every_state() {
    require_git!();
    let (_t, base) = tmp();
    let r = new_repo(&base.join("r"));
    write(&r.join("mod.toml"), "a");
    write(&r.join("del.toml"), "a");
    write(
        &r.join("old name.toml"),
        "rename me please, I am long enough",
    );
    write(&r.join("both.toml"), "a");
    write(&r.join("conf.toml"), "base");
    commit_all(&r, "init");

    write(&r.join("mod.toml"), "b");
    fs::remove_file(r.join("del.toml")).unwrap();
    write(&r.join("dir/untracked.toml"), "u");
    sh(&r, &["mv", "old name.toml", "dir/new name.toml"]);
    write(&r.join("added.toml"), "x");
    sh(&r, &["add", "added.toml"]);
    write(&r.join("both.toml"), "b");
    sh(&r, &["add", "both.toml"]);
    write(&r.join("both.toml"), "c");

    assert_eq!(
        states(&r),
        [
            ("added.toml".into(), FileState::Added, true),
            ("both.toml".into(), FileState::Modified, true),
            ("del.toml".into(), FileState::Deleted, false),
            ("dir/new name.toml".into(), FileState::Renamed, true),
            ("dir/untracked.toml".into(), FileState::Untracked, false),
            ("mod.toml".into(), FileState::Modified, false),
        ]
    );
    let renamed = git::status(&r)
        .unwrap()
        .into_iter()
        .find(|e| e.state == FileState::Renamed)
        .unwrap();
    assert_eq!(renamed.old_path, Some(r.join("old name.toml")));

    // Conflict: two branches edit the same line.
    let c = new_repo(&base.join("c"));
    write(&c.join("conf.toml"), "base");
    commit_all(&c, "base");
    sh(&c, &["switch", "-q", "-c", "other"]);
    write(&c.join("conf.toml"), "theirs");
    commit_all(&c, "theirs");
    sh(&c, &["switch", "-q", "main"]);
    write(&c.join("conf.toml"), "ours");
    commit_all(&c, "ours");
    let merge = Command::new("git")
        .current_dir(&c)
        .args(["merge", "other"])
        .output()
        .unwrap();
    assert!(!merge.status.success());
    assert_eq!(
        states(&c),
        [("conf.toml".into(), FileState::Conflicted, false)]
    );
    let info = git::repo_info(&c).unwrap();
    assert!(info.conflicted && info.dirty);

    // Resolve, then commit during a merge (whole index is committed).
    write(&c.join("conf.toml"), "resolved");
    git::commit(&c, &[c.join("conf.toml")], "merge").unwrap();
    assert!(states(&c).is_empty());
    assert!(!git::repo_info(&c).unwrap().conflicted);
}

#[test]
fn nested_repo_files_are_reported_only_by_the_nested_repo() {
    require_git!();
    let (_t, base) = tmp();
    let outer = new_repo(&base.join("outer"));
    write(&outer.join("top.toml"), "a");
    write(&outer.join("col/tracked-by-outer.toml"), "a");
    commit_all(&outer, "init");

    // `col` later becomes its own repo.
    let col = new_repo(&outer.join("col"));
    write(&col.join("req.toml"), "x");
    write(&col.join("tracked-by-outer.toml"), "changed");
    write(&outer.join("top.toml"), "b");

    assert_eq!(
        states(&outer),
        [("top.toml".into(), FileState::Modified, false)]
    );
    assert_eq!(
        states(&col),
        [
            ("req.toml".into(), FileState::Untracked, false),
            ("tracked-by-outer.toml".into(), FileState::Untracked, false),
        ]
    );
}

#[test]
fn checkout_creates_and_switches() {
    require_git!();
    let (_t, base) = tmp();
    let r = new_repo(&base.join("r"));
    write(&r.join("a.toml"), "1");
    commit_all(&r, "init");

    let info = git::checkout(&r, "feature/x", true).unwrap();
    assert_eq!(info.branch.as_deref(), Some("feature/x"));
    write(&r.join("a.toml"), "2");
    commit_all(&r, "on feature");

    let info = git::checkout(&r, "main", false).unwrap();
    assert_eq!(info.branch.as_deref(), Some("main"));
    let branches = git::branches(&r).unwrap();
    let names: Vec<_> = branches
        .iter()
        .map(|b| (b.name.as_str(), b.current))
        .collect();
    assert_eq!(names, [("feature/x", false), ("main", true)]);

    // Local change that the other branch would overwrite: refused.
    write(&r.join("a.toml"), "dirty");
    let err = git::checkout(&r, "feature/x", false).unwrap_err();
    assert!(err.to_string().contains("overwritten"), "{err}");
    assert!(git::checkout(&r, "--orphan", false).is_err());
    assert!(git::checkout(&r, "nope", false).is_err());
}

#[test]
fn commit_includes_only_selected_paths() {
    require_git!();
    let (_t, base) = tmp();
    let r = new_repo(&base.join("r"));

    // Initial commit of one file in an empty repo.
    write(&r.join("a.toml"), "1");
    write(&r.join("b.toml"), "1");
    let c = git::commit(&r, &[r.join("a.toml")], "first").unwrap();
    assert_eq!(c.summary, "first");
    assert_eq!(c.author, "Tester");
    assert!(c.id.starts_with(&c.short_id));
    assert_eq!(states(&r), [("b.toml".into(), FileState::Untracked, false)]);

    git::commit(&r, &[r.join("b.toml")], "second").unwrap();
    write(&r.join("a.toml"), "2");
    write(&r.join("b.toml"), "2");
    write(&r.join("dir/new.toml"), "n");
    // `b.toml` is staged but not selected: it must stay out of the commit.
    sh(&r, &["add", "b.toml"]);
    git::commit(&r, &[r.join("a.toml"), r.join("dir")], "third\n\nbody").unwrap();
    assert_eq!(states(&r), [("b.toml".into(), FileState::Modified, true)]);
    let files = sh(&r, &["show", "--name-only", "--format=", "HEAD"]);
    assert_eq!(
        files.lines().collect::<Vec<_>>(),
        ["a.toml", "dir/new.toml"]
    );

    // Deletions (staged and unstaged) and renames (old path comes along).
    sh(&r, &["commit", "-q", "-m", "b"]);
    fs::remove_file(r.join("a.toml")).unwrap();
    sh(&r, &["rm", "-q", "b.toml"]);
    sh(&r, &["mv", "dir/new.toml", "dir/renamed.toml"]);
    git::commit(
        &r,
        &[
            r.join("a.toml"),
            r.join("b.toml"),
            r.join("dir/renamed.toml"),
        ],
        "deletes and rename",
    )
    .unwrap();
    assert!(states(&r).is_empty());

    assert!(git::commit(&r, &[r.join("dir")], "nothing").is_err());
    assert!(git::commit(&r, &[r.join("x")], "  ").is_err());
    assert!(git::commit(&r, &[base.join("outside")], "m").is_err());
    assert!(git::commit(&r, &[r.join(".git/config")], "m").is_err());
}

#[test]
fn discard_restores_or_deletes() {
    require_git!();
    let (_t, base) = tmp();
    let r = new_repo(&base.join("r"));
    write(&r.join("mod.toml"), "orig");
    write(&r.join("del.toml"), "orig");
    write(&r.join("ren.toml"), "orig");
    write(&r.join("keep.toml"), "orig");
    commit_all(&r, "init");

    write(&r.join("mod.toml"), "changed");
    sh(&r, &["add", "mod.toml"]);
    write(&r.join("mod.toml"), "changed again");
    fs::remove_file(r.join("del.toml")).unwrap();
    sh(&r, &["mv", "ren.toml", "renamed.toml"]);
    write(&r.join("new/untracked.toml"), "u");
    write(&r.join("added.toml"), "a");
    sh(&r, &["add", "added.toml"]);
    write(&r.join("keep.toml"), "keep my change");

    git::discard(
        &r,
        &[
            r.join("mod.toml"),
            r.join("del.toml"),
            r.join("renamed.toml"),
            r.join("new"),
            r.join("added.toml"),
        ],
    )
    .unwrap();
    assert_eq!(
        states(&r),
        [("keep.toml".into(), FileState::Modified, false)]
    );
    assert_eq!(fs::read_to_string(r.join("mod.toml")).unwrap(), "orig");
    assert_eq!(fs::read_to_string(r.join("del.toml")).unwrap(), "orig");
    assert_eq!(fs::read_to_string(r.join("ren.toml")).unwrap(), "orig");
    assert!(!r.join("renamed.toml").exists());
    assert!(!r.join("new/untracked.toml").exists());
    assert!(!r.join("added.toml").exists());

    // Paths outside the repo are refused.
    assert!(git::discard(&r, &[base.join("elsewhere.toml")]).is_err());
    assert!(git::discard(&r, std::slice::from_ref(&r)).is_err());
}

#[test]
fn log_of_repo_and_of_one_file() {
    require_git!();
    let (_t, base) = tmp();
    let r = new_repo(&base.join("r"));
    assert!(git::log(&r, None, 10).unwrap().is_empty());
    write(&r.join("a.toml"), "1");
    commit_all(&r, "add a");
    write(&r.join("b.toml"), "1");
    commit_all(&r, "add b");
    write(&r.join("a.toml"), "2");
    commit_all(&r, "change a");

    let all: Vec<_> = git::log(&r, None, 10)
        .unwrap()
        .into_iter()
        .map(|c| c.summary)
        .collect();
    assert_eq!(all, ["change a", "add b", "add a"]);
    let a: Vec<_> = git::log(&r, Some(&r.join("a.toml")), 10)
        .unwrap()
        .into_iter()
        .map(|c| c.summary)
        .collect();
    assert_eq!(a, ["change a", "add a"]);
    assert_eq!(git::log(&r, None, 1).unwrap().len(), 1);
    assert!(git::log(&r, Some(&r.join("a.toml")), 1).unwrap()[0].time > 0);
}

#[test]
fn diff_file_reads_head_and_worktree() {
    require_git!();
    let (_t, base) = tmp();
    let r = new_repo(&base.join("r"));
    write(&r.join("col/a.toml"), "old");
    commit_all(&r, "init");
    write(&r.join("col/a.toml"), "new");
    write(&r.join("col/b.toml"), "untracked");

    let d = git::diff_file(r.join("col/a.toml")).unwrap();
    assert_eq!(
        (d.old.as_deref(), d.new.as_deref()),
        (Some("old"), Some("new"))
    );
    let d = git::diff_file(r.join("col/b.toml")).unwrap();
    assert_eq!((d.old, d.new.as_deref()), (None, Some("untracked")));
    fs::remove_file(r.join("col/a.toml")).unwrap();
    let d = git::diff_file(r.join("col/a.toml")).unwrap();
    assert_eq!((d.old.as_deref(), d.new), (Some("old"), None));

    // Nearest repo wins.
    let inner = new_repo(&r.join("col/inner"));
    write(&inner.join("x.toml"), "inner");
    commit_all(&inner, "x");
    let d = git::diff_file(inner.join("x.toml")).unwrap();
    assert_eq!(d.old.as_deref(), Some("inner"));
}

#[test]
fn init_makes_a_repo() {
    require_git!();
    let (_t, base) = tmp();
    let dir = base.join("col");
    fs::create_dir_all(&dir).unwrap();
    let info = git::init(&dir).unwrap();
    assert_eq!(info.root, dir);
    assert!(info.branch.is_some());
    assert_eq!(info.head, None);
    assert!(git::init(&dir).is_err());
}

#[test]
fn fetch_pull_push_with_local_remote() {
    require_git!();
    let (_t, base) = tmp();
    let remote = base.join("remote.git");
    sh(&base, &["init", "-q", "--bare", "-b", "main", "remote.git"]);
    let url = format!("file://{}", remote.display());

    let a = new_repo(&base.join("a"));
    sh(&a, &["remote", "add", "origin", &url]);
    write(&a.join("x.toml"), "1");
    commit_all(&a, "one");
    // No upstream yet: push publishes to origin and sets tracking.
    let info = git::push(&a).unwrap();
    assert_eq!(info.upstream.as_deref(), Some("origin/main"));
    assert_eq!((info.ahead, info.behind), (0, 0));

    write(&a.join("x.toml"), "2");
    commit_all(&a, "two");
    let info = git::repo_info(&a).unwrap();
    assert_eq!((info.ahead, info.behind), (1, 0));
    git::push(&a).unwrap();

    sh(&base, &["clone", "-q", &url, "b"]);
    let b = dunce::canonicalize(base.join("b")).unwrap();
    configure(&b);
    write(&b.join("x.toml"), "3");
    commit_all(&b, "three");
    git::push(&b).unwrap();

    let info = git::fetch(&a).unwrap();
    assert_eq!((info.ahead, info.behind), (0, 1));
    let branches = git::branches(&a).unwrap();
    let main = branches.iter().find(|b| b.name == "main").unwrap();
    assert_eq!(main.upstream.as_deref(), Some("origin/main"));
    assert!(branches.iter().any(|b| b.remote && b.name == "origin/main"));
    let info = git::pull(&a).unwrap();
    assert_eq!((info.ahead, info.behind), (0, 0));
    assert_eq!(fs::read_to_string(a.join("x.toml")).unwrap(), "3");

    // Diverged: pull refuses to merge.
    write(&a.join("x.toml"), "4a");
    commit_all(&a, "four a");
    write(&b.join("y.toml"), "4b");
    commit_all(&b, "four b");
    git::push(&b).unwrap();
    let info = git::fetch(&a).unwrap();
    assert_eq!((info.ahead, info.behind), (1, 1));
    assert!(git::pull(&a).is_err());
    assert!(git::push(&a).is_err());

    // Checking out a remote-only branch creates a tracking branch.
    sh(&b, &["switch", "-q", "-c", "feature"]);
    git::push(&b).unwrap();
    git::fetch(&a).unwrap();
    sh(&a, &["reset", "-q", "--hard", "origin/main"]);
    let info = git::checkout(&a, "origin/feature", false).unwrap();
    assert_eq!(info.branch.as_deref(), Some("feature"));
    assert_eq!(info.upstream.as_deref(), Some("origin/feature"));
}

#[test]
fn json_shape_matches_frontend_contract() {
    let s = git::FileStatus {
        path: "/p/a.toml".into(),
        state: FileState::Untracked,
        staged: false,
        old_path: None,
    };
    let v = serde_json::to_value(&s).unwrap();
    assert_eq!(
        v,
        serde_json::json!({"path": "/p/a.toml", "state": "untracked", "staged": false, "old_path": null})
    );
    let info = git::RepoInfo {
        root: "/p".into(),
        branch: None,
        head: None,
        upstream: None,
        ahead: 0,
        behind: 0,
        dirty: false,
        conflicted: false,
    };
    let keys: Vec<_> = serde_json::to_value(&info)
        .unwrap()
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    let mut expected = [
        "root",
        "branch",
        "head",
        "upstream",
        "ahead",
        "behind",
        "dirty",
        "conflicted",
    ];
    expected.sort();
    assert_eq!(keys, expected);
}

/// Not an assertion on speed (CI machines vary); prints timings with --nocapture.
#[test]
fn timing_on_a_small_project() {
    require_git!();
    let (_t, base) = tmp();
    let proj = new_repo(&base.join("proj"));
    for c in 0..3 {
        for f in 0..10 {
            write(
                &proj.join(format!("col{c}/folder{f}/sub/req.toml")),
                "method = \"GET\"",
            );
        }
    }
    commit_all(&proj, "init");
    new_repo(&proj.join("col1"));
    write(&proj.join("col0/folder1/sub/req.toml"), "changed");

    let t = Instant::now();
    let repos = git::repos(&proj).unwrap();
    let repos_time = t.elapsed();
    assert_eq!(repos.len(), 2);
    let t = Instant::now();
    let st = git::status(&proj).unwrap();
    let status_time = t.elapsed();
    assert_eq!(st.len(), 1);
    let t = Instant::now();
    let roots = git::discover_repos(&proj).unwrap();
    let discover_time = t.elapsed();
    assert_eq!(roots.len(), 2);
    eprintln!("discover_repos: {discover_time:?}, repos: {repos_time:?}, status: {status_time:?}");
}
