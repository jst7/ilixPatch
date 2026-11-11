//! Commit details and history actions against real temp repos.

mod common;

use std::fs;
use std::path::Path;

use common::*;
use ilixpatch_git::{self as git, CommitFileState, Operation, ResetMode};

fn head(r: &Path) -> String {
    sh(r, &["rev-parse", "HEAD"]).trim().to_string()
}

fn summaries(r: &Path) -> Vec<String> {
    git::log(r, None, 50)
        .unwrap()
        .into_iter()
        .map(|c| c.summary)
        .collect()
}

#[test]
fn root_commit_details_refs_and_binary() {
    require_git!();
    let (_t, base) = tmp();
    let r = new_repo(&base.join("r"));
    write(&r.join("a.toml"), "1\n2\n3\n");
    write(&r.join("blob.bin"), b"\x00\x01\x02");
    commit_all(&r, "init\n\nlonger body");
    sh(&r, &["tag", "-a", "v1", "-m", "release"]);
    sh(&r, &["tag", "light"]);
    let id = head(&r);

    let d = git::commit_details(&r, &id).unwrap();
    assert_eq!(d.commit.id, id);
    assert_eq!(d.commit.summary, "init");
    assert_eq!(d.message, "init\n\nlonger body");
    assert_eq!(d.commit.author, "Tester");
    assert_eq!(d.author_email, "t@example.com");
    assert_eq!(d.committer, "Tester");
    assert!(d.committer_time > 0);
    assert!(d.parents.is_empty());
    let mut refs = d.refs.clone();
    refs.sort();
    assert_eq!(refs, ["light", "main", "v1"]);
    assert_eq!(d.files.len(), 2);
    let a = d.files.iter().find(|f| f.path == r.join("a.toml")).unwrap();
    assert_eq!(
        (a.state, a.additions, a.deletions, a.binary),
        (CommitFileState::Added, Some(3), Some(0), false)
    );
    let b = d
        .files
        .iter()
        .find(|f| f.path == r.join("blob.bin"))
        .unwrap();
    assert_eq!((b.additions, b.deletions, b.binary), (None, None, true));

    // JSON: GitCommit fields are flattened in.
    let v = serde_json::to_value(&d).unwrap();
    assert_eq!(v["short_id"], d.commit.short_id.as_str());
    assert_eq!(v["files"][0]["state"], "added");

    let diff = git::commit_file_diff(&r, &id, r.join("blob.bin")).unwrap();
    assert_eq!((diff.old, diff.new.as_deref()), (None, Some("")));
    let diff = git::commit_file_diff(&r, &id[..7], r.join("a.toml")).unwrap();
    assert_eq!((diff.old, diff.new.as_deref()), (None, Some("1\n2\n3\n")));

    for bad in ["-n", "--all", "nope", "", "HEAD ; rm"] {
        assert!(git::commit_details(&r, bad).is_err(), "{bad}");
    }
    // A tree id is not a commit.
    let tree = sh(&r, &["rev-parse", "HEAD^{tree}"]).trim().to_string();
    assert!(git::commit_details(&r, &tree).is_err());
}

#[test]
fn renames_and_file_diff() {
    require_git!();
    let (_t, base) = tmp();
    let r = new_repo(&base.join("r"));
    let body: String = (0..20).map(|i| format!("line {i}\n")).collect();
    write(&r.join("a.toml"), &body);
    write(&r.join("gone.toml"), "x\n");
    commit_all(&r, "init");
    fs::create_dir_all(r.join("dir")).unwrap();
    sh(&r, &["mv", "a.toml", "dir/b.toml"]);
    write(&r.join("dir/b.toml"), format!("{body}line 20\n"));
    fs::remove_file(r.join("gone.toml")).unwrap();
    write(&r.join("c.toml"), "c\n");
    commit_all(&r, "rename");

    let d = git::commit_details(&r, "HEAD").unwrap();
    assert_eq!(d.parents.len(), 1);
    let mut files: Vec<_> = d
        .files
        .iter()
        .map(|f| {
            (
                f.path
                    .strip_prefix(&r)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/"),
                f.state,
                f.old_path.clone(),
                f.additions,
                f.deletions,
            )
        })
        .collect();
    files.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        files,
        [
            (
                "c.toml".into(),
                CommitFileState::Added,
                None,
                Some(1),
                Some(0)
            ),
            (
                "dir/b.toml".into(),
                CommitFileState::Renamed,
                Some(r.join("a.toml")),
                Some(1),
                Some(0)
            ),
            (
                "gone.toml".into(),
                CommitFileState::Deleted,
                None,
                Some(0),
                Some(1)
            ),
        ]
    );

    let diff = git::commit_file_diff(&r, "HEAD", r.join("dir/b.toml")).unwrap();
    assert_eq!(diff.old.as_deref(), Some(body.as_str()));
    assert_eq!(diff.new, Some(format!("{body}line 20\n")));
    let diff = git::commit_file_diff(&r, "HEAD", r.join("gone.toml")).unwrap();
    assert_eq!((diff.old.as_deref(), diff.new), (Some("x\n"), None));
}

#[test]
fn log_pages() {
    require_git!();
    let (_t, base) = tmp();
    let r = new_repo(&base.join("r"));
    for i in 0..5 {
        write(&r.join("a.toml"), format!("{i}"));
        write(&r.join(format!("other{i}.toml")), "x");
        commit_all(&r, &format!("c{i}"));
    }
    let page: Vec<_> = git::log_page(&r, None, 2, 2)
        .unwrap()
        .into_iter()
        .map(|c| c.summary)
        .collect();
    assert_eq!(page, ["c2", "c1"]);
    let page: Vec<_> = git::log_page(&r, Some(&r.join("other0.toml")), 0, 10)
        .unwrap()
        .into_iter()
        .map(|c| c.summary)
        .collect();
    assert_eq!(page, ["c0"]);
    assert!(git::log_page(&r, None, 10, 5).unwrap().is_empty());
}

#[test]
fn revert_clean_conflicting_and_no_commit() {
    require_git!();
    let (_t, base) = tmp();
    let r = new_repo(&base.join("r"));
    let mut ids = Vec::new();
    for i in 1..=3 {
        write(&r.join("a.toml"), format!("{i}\n"));
        commit_all(&r, &format!("c{i}"));
        ids.push(head(&r));
    }

    // Reverting c2 conflicts with c3.
    let st = git::revert_commit(&r, &ids[1], false).unwrap();
    assert_eq!(st.operation, Some(Operation::Revert));
    assert_eq!(st.conflicted, std::slice::from_ref(&r.join("a.toml")));
    assert!(git::reset_to(&r, &ids[0], ResetMode::Hard).is_err());
    assert!(git::revert_commit(&r, &ids[2], false).is_err());
    git::abort_operation(&r).unwrap();

    let st = git::revert_commit(&r, &ids[2], false).unwrap();
    assert_eq!(st.operation, None);
    assert_eq!(read(&r.join("a.toml")), "2\n");
    assert!(summaries(&r)[0].starts_with("Revert \"c3\""));

    // --no-commit only stages the change.
    let before = head(&r);
    git::revert_commit(&r, &before, true).unwrap();
    assert_eq!(head(&r), before);
    assert_eq!(read(&r.join("a.toml")), "3\n");
    assert!(git::status(&r).unwrap()[0].staged);
}

#[test]
fn revert_merge_commit_uses_first_parent() {
    require_git!();
    let (_t, base) = tmp();
    let r = new_repo(&base.join("r"));
    write(&r.join("base.toml"), "b");
    commit_all(&r, "base");
    sh(&r, &["switch", "-q", "-c", "feature"]);
    write(&r.join("f.toml"), "f");
    commit_all(&r, "feature");
    sh(&r, &["switch", "-q", "main"]);
    write(&r.join("m.toml"), "m");
    commit_all(&r, "main");
    sh(&r, &["merge", "-q", "--no-ff", "--no-edit", "feature"]);
    let merge = head(&r);
    assert_eq!(git::commit_details(&r, &merge).unwrap().parents.len(), 2);
    // Changes of a merge are shown against its first parent.
    let files = git::commit_details(&r, &merge).unwrap().files;
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, r.join("f.toml"));

    let st = git::revert_commit(&r, &merge, false).unwrap();
    assert_eq!(st.operation, None);
    assert!(!r.join("f.toml").exists());
    assert!(r.join("m.toml").exists());
}

#[test]
fn reset_modes_and_restore_file() {
    require_git!();
    let (_t, base) = tmp();
    let r = new_repo(&base.join("r"));
    write(&r.join("a.toml"), "1");
    commit_all(&r, "c1");
    let c1 = head(&r);
    write(&r.join("a.toml"), "2");
    write(&r.join("b.toml"), "b");
    commit_all(&r, "c2");
    let c2 = head(&r);

    let info = git::reset_to(&r, &c1, ResetMode::Soft).unwrap();
    assert!(c1.starts_with(info.head.as_deref().unwrap()));
    assert!(git::status(&r).unwrap().iter().all(|e| e.staged));
    git::reset_to(&r, &c2, ResetMode::Hard).unwrap();
    write(&r.join("a.toml"), "dirty");
    let info = git::reset_to(&r, &c1, ResetMode::Hard).unwrap();
    assert!(!info.dirty);
    assert_eq!(read(&r.join("a.toml")), "1");
    // Mixed: HEAD moves, the working tree (still at c1) shows as unstaged changes.
    git::reset_to(&r, &c2, ResetMode::Mixed).unwrap();
    let st = git::status(&r).unwrap();
    assert!(st.len() == 2 && st.iter().all(|e| !e.staged));
    git::reset_to(&r, &c2, ResetMode::Hard).unwrap();

    git::restore_file(&r, r.join("a.toml"), &c1).unwrap();
    assert_eq!(read(&r.join("a.toml")), "1");
    let st = git::status(&r).unwrap();
    assert!(st.len() == 1 && st[0].staged);
    let err = git::restore_file(&r, r.join("b.toml"), &c1).unwrap_err();
    assert!(err.to_string().contains("doesn't exist"), "{err}");
    assert!(git::restore_file(&r, base.join("x.toml"), &c1).is_err());
}

#[test]
fn cherry_pick_branch_and_detached_checkout() {
    require_git!();
    let (_t, base) = tmp();
    let r = new_repo(&base.join("r"));
    write(&r.join("a.toml"), "1\n");
    commit_all(&r, "base");
    let base_id = head(&r);
    sh(&r, &["switch", "-q", "-c", "other"]);
    write(&r.join("new.toml"), "n");
    commit_all(&r, "add new");
    let pick = head(&r);
    write(&r.join("a.toml"), "other\n");
    commit_all(&r, "other a");
    let conflicting = head(&r);
    sh(&r, &["switch", "-q", "main"]);
    write(&r.join("a.toml"), "main\n");
    commit_all(&r, "main a");

    let st = git::cherry_pick(&r, &pick).unwrap();
    assert_eq!(st.operation, None);
    assert!(r.join("new.toml").exists());
    assert_eq!(summaries(&r)[0], "add new");

    let st = git::cherry_pick(&r, &conflicting).unwrap();
    assert_eq!(st.operation, Some(Operation::CherryPick));
    assert_eq!(st.conflicted.len(), 1);
    git::abort_operation(&r).unwrap();

    let info = git::create_branch_at(&r, "from-base", &base_id, false).unwrap();
    assert_eq!(info.branch.as_deref(), Some("main"));
    assert!(
        git::branches(&r)
            .unwrap()
            .iter()
            .any(|b| b.name == "from-base")
    );
    let info = git::create_branch_at(&r, "fix", &base_id[..8], true).unwrap();
    assert_eq!(info.branch.as_deref(), Some("fix"));
    assert_eq!(head(&r), base_id);
    assert!(git::create_branch_at(&r, "-x", &base_id, false).is_err());

    let info = git::checkout_commit(&r, &pick).unwrap();
    assert_eq!(info.branch, None);
    assert_eq!(head(&r), pick);
    write(&r.join("new.toml"), "local change");
    assert!(git::checkout_commit(&r, &base_id).is_err());
}
