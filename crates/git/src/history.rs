//! Commit details and history actions (D27): browse a commit, revert,
//! reset, cherry-pick, restore a file, branch from or check out a commit.
//!
//! Every `commit` argument is resolved with gix to a full commit id first,
//! so user strings are never handed to git.

use std::path::{Path, PathBuf};

use gix::ObjectId;
use serde::{Deserialize, Serialize};

use crate::merge::{MergeState, looks_binary, refuse_if_busy, state_at};
use crate::read::{self, gx};
use crate::{
    Commit, Error, FileDiff, RepoInfo, Result, cli, in_repo, info_at, pathspec, repo_root,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CommitFileState {
    Added,
    Modified,
    Deleted,
    Renamed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitFile {
    /// Absolute path (as in that commit).
    pub path: PathBuf,
    pub old_path: Option<PathBuf>,
    pub state: CommitFileState,
    /// `None` for binary files.
    pub additions: Option<u32>,
    pub deletions: Option<u32>,
    pub binary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitDetails {
    #[serde(flatten)]
    pub commit: Commit,
    /// Full message (summary + body).
    pub message: String,
    pub author_email: String,
    pub committer: String,
    pub committer_email: String,
    pub committer_time: i64,
    /// Full parent ids; empty for a root commit.
    pub parents: Vec<String>,
    /// Changes against the first parent (the empty tree for a root commit).
    pub files: Vec<CommitFile>,
    /// Branch and tag names pointing at this commit.
    pub refs: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResetMode {
    Soft,
    Mixed,
    Hard,
}

/// Resolve `spec` (a full or abbreviated hex id, or any revision gix
/// understands such as `HEAD~1`) to a commit id.
fn resolve(repo: &gix::Repository, spec: &str) -> Result<ObjectId> {
    let spec = spec.trim();
    let bad = || Error::Git(format!("invalid commit: {spec:?}"));
    if spec.is_empty() || spec.starts_with('-') || spec.chars().any(|c| c.is_control() || c == ' ')
    {
        return Err(bad());
    }
    let is_hex = (4..=64).contains(&spec.len()) && spec.bytes().all(|b| b.is_ascii_hexdigit());
    let id = repo
        .rev_parse_single(spec)
        .map_err(|e| Error::Git(format!("unknown commit {spec:?}: {e}")))?;
    let commit = id.object().map_err(gx)?.peel_to_commit().map_err(|_| {
        if is_hex {
            Error::Git(format!("{spec} is not a commit"))
        } else {
            bad()
        }
    })?;
    Ok(commit.id)
}

fn open_commit<'r>(repo: &'r gix::Repository, spec: &str) -> Result<gix::Commit<'r>> {
    let id = resolve(repo, spec)?;
    repo.find_commit(id).map_err(gx)
}

/// History page of the repo, or of one file/directory (`skip` + `limit`).
pub fn log_page(
    root: impl AsRef<Path>,
    path: Option<&Path>,
    skip: usize,
    limit: usize,
) -> Result<Vec<Commit>> {
    let root = repo_root(root.as_ref())?;
    let rel = path
        .map(|p| in_repo(&root, p).map(|(_, r)| r))
        .transpose()?;
    read::log(&read::open(&root)?, rel.as_deref(), skip, limit)
}

/// Names (`main`, `origin/main`, `v1.0`) of refs pointing at `id`.
fn refs_at(repo: &gix::Repository, id: ObjectId) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(platform) = repo.references() else {
        return out;
    };
    for iter in [
        platform.local_branches(),
        platform.remote_branches(),
        platform.tags(),
    ] {
        let Ok(iter) = iter else { continue };
        for r in iter.flatten() {
            let Some(target) = r.try_id() else { continue };
            let hit = target.detach() == id
                || (r.name().as_bstr().starts_with(b"refs/tags/")
                    && target
                        .object()
                        .ok()
                        .and_then(|o| o.peel_to_commit().ok())
                        .is_some_and(|c| c.id == id));
            if hit {
                out.push(r.name().shorten().to_string());
            }
        }
    }
    out
}

/// Files changed by `id` against its first parent (or the empty tree), with
/// renames detected. Paths are repo-relative.
fn changed_files(root: &Path, id: ObjectId, parent: Option<ObjectId>) -> Result<Vec<CommitFile>> {
    let id = id.to_string();
    let mut args = vec![
        "diff-tree",
        "-r",
        "-z",
        "-M",
        "--raw",
        "--numstat",
        "--no-commit-id",
    ];
    let parent = parent.map(|p| p.to_string());
    match &parent {
        Some(p) => args.push(p),
        None => args.push("--root"),
    }
    args.push(&id);
    let out = cli::git(root, &args)?;
    let mut tokens = out.split(|b| *b == 0).filter(|t| !t.is_empty()).peekable();
    let path = |t: &[u8]| root.join(PathBuf::from(String::from_utf8_lossy(t).into_owned()));
    let mut files = Vec::new();
    // Raw records first: ":<modes> <ids> <status>" NUL path [NUL path].
    while let Some(t) = tokens.next_if(|t| t.starts_with(b":")) {
        let status = String::from_utf8_lossy(t)
            .rsplit(' ')
            .next()
            .unwrap_or("")
            .to_string();
        let state = match status.chars().next() {
            Some('A') => CommitFileState::Added,
            Some('D') => CommitFileState::Deleted,
            Some('R' | 'C') => CommitFileState::Renamed,
            _ => CommitFileState::Modified,
        };
        let first = tokens.next().map(path);
        let (old_path, p) = if state == CommitFileState::Renamed {
            (first, tokens.next().map(path))
        } else {
            (None, first)
        };
        let Some(p) = p else { break };
        files.push(CommitFile {
            path: p,
            old_path,
            state,
            additions: None,
            deletions: None,
            binary: false,
        });
    }
    // Then numstat records, in the same order: "add\tdel\tpath" (or
    // "add\tdel\t" NUL old NUL new for renames); "-" marks binary.
    for f in files.iter_mut() {
        let Some(t) = tokens.next() else { break };
        let s = String::from_utf8_lossy(t);
        let mut parts = s.splitn(3, '\t');
        let (a, d, rest) = (parts.next(), parts.next(), parts.next().unwrap_or(""));
        if rest.is_empty() {
            tokens.next();
            tokens.next();
        }
        let (a, d) = (
            a.and_then(|x| x.parse().ok()),
            d.and_then(|x| x.parse().ok()),
        );
        f.binary = a.is_none() && d.is_none();
        f.additions = a;
        f.deletions = d;
    }
    Ok(files)
}

/// Metadata, message, refs and changed files of a commit.
pub fn commit_details(root: impl AsRef<Path>, commit: &str) -> Result<CommitDetails> {
    let root = repo_root(root.as_ref())?;
    let repo = read::open(&root)?;
    let c = open_commit(&repo, commit)?;
    let base = read::to_commit(&c)?;
    let author = c.author().map_err(gx)?;
    let committer = c.committer().map_err(gx)?;
    let parents: Vec<ObjectId> = c.parent_ids().map(|p| p.detach()).collect();
    let message = read::lossy(c.message_raw().map_err(gx)?.to_vec())
        .trim_end()
        .to_string();
    Ok(CommitDetails {
        message,
        author_email: author.email.to_string(),
        committer: committer.name.to_string(),
        committer_email: committer.email.to_string(),
        committer_time: committer.time().map(|t| t.seconds).unwrap_or(0),
        parents: parents.iter().map(ToString::to_string).collect(),
        files: changed_files(&root, c.id, parents.first().copied())?,
        refs: refs_at(&repo, c.id),
        commit: base,
    })
}

/// A file before (`old`, first parent, at its old path for renames) and
/// after (`new`) a commit. Binary versions that exist are returned as `""`.
pub fn commit_file_diff(
    root: impl AsRef<Path>,
    commit: &str,
    path: impl AsRef<Path>,
) -> Result<FileDiff> {
    let root = repo_root(root.as_ref())?;
    let (abs, rel) = in_repo(&root, path.as_ref())?;
    let repo = read::open(&root)?;
    let c = open_commit(&repo, commit)?;
    let parent = c.parent_ids().next().map(|p| p.detach());
    let old_rel = changed_files(&root, c.id, parent)?
        .into_iter()
        .find(|f| f.path == abs)
        .and_then(|f| f.old_path)
        .and_then(|p| p.strip_prefix(&root).ok().map(Path::to_path_buf))
        .unwrap_or_else(|| rel.clone());
    let old = match parent {
        Some(p) => read::blob_in(&repo, &repo.find_commit(p).map_err(gx)?, &old_rel)?,
        None => None,
    };
    let new = read::blob_in(&repo, &c, &rel)?;
    let binary = old.iter().chain(new.iter()).any(|b| looks_binary(b));
    let text = |b: Option<Vec<u8>>| {
        b.map(|b| {
            if binary {
                String::new()
            } else {
                read::lossy(b)
            }
        })
    };
    Ok(FileDiff {
        path: abs,
        old: text(old),
        new: text(new),
    })
}

/// `-m 1` for merge commits (revert/cherry-pick relative to the first parent).
fn mainline(c: &gix::Commit<'_>) -> Vec<&'static str> {
    if c.parent_ids().count() > 1 {
        vec!["-m", "1"]
    } else {
        vec![]
    }
}

/// Treat a revert/cherry-pick that stopped on conflicts as success (the
/// returned state shows them); `--no-commit` leaves no REVERT_HEAD, so only
/// the conflicted files are checked.
fn ok_or_conflicts(root: &Path, res: Result<Vec<u8>>) -> Result<()> {
    match res {
        Ok(_) => Ok(()),
        Err(Error::Git(_)) if !state_at(root)?.conflicted.is_empty() => Ok(()),
        Err(e) => Err(e),
    }
}

/// Create a commit undoing `commit` (`no_commit`: only apply the changes).
/// Merge commits are reverted against their first parent. Conflicts are
/// not an error: inspect the returned state.
pub fn revert_commit(root: impl AsRef<Path>, commit: &str, no_commit: bool) -> Result<MergeState> {
    let root = repo_root(root.as_ref())?;
    refuse_if_busy(&root)?;
    let repo = read::open(&root)?;
    let c = open_commit(&repo, commit)?;
    let id = c.id.to_string();
    let mut args = vec!["revert", "--no-edit"];
    if no_commit {
        args.push("--no-commit");
    }
    args.extend(mainline(&c));
    args.push(&id);
    let res = cli::git_env(&root, &args, &[("GIT_EDITOR", "true")]);
    ok_or_conflicts(&root, res)?;
    state_at(&root)
}

/// Apply `commit` on top of the current branch. Conflicts are not an error.
pub fn cherry_pick(root: impl AsRef<Path>, commit: &str) -> Result<MergeState> {
    let root = repo_root(root.as_ref())?;
    refuse_if_busy(&root)?;
    let repo = read::open(&root)?;
    let c = open_commit(&repo, commit)?;
    let id = c.id.to_string();
    let mut args = vec!["cherry-pick"];
    args.extend(mainline(&c));
    args.push(&id);
    let res = cli::git_env(&root, &args, &[("GIT_EDITOR", "true")]);
    ok_or_conflicts(&root, res)?;
    state_at(&root)
}

/// Move the current branch to `commit`. `Hard` discards local changes.
pub fn reset_to(root: impl AsRef<Path>, commit: &str, mode: ResetMode) -> Result<RepoInfo> {
    let root = repo_root(root.as_ref())?;
    refuse_if_busy(&root)?;
    let id = resolve(&read::open(&root)?, commit)?.to_string();
    let flag = match mode {
        ResetMode::Soft => "--soft",
        ResetMode::Mixed => "--mixed",
        ResetMode::Hard => "--hard",
    };
    cli::git(&root, ["reset", "--quiet", flag, id.as_str()])?;
    info_at(&root)
}

/// Overwrite one file (index and worktree) with its version at `commit`.
pub fn restore_file(root: impl AsRef<Path>, path: impl AsRef<Path>, commit: &str) -> Result<()> {
    let root = repo_root(root.as_ref())?;
    let (abs, rel) = in_repo(&root, path.as_ref())?;
    let repo = read::open(&root)?;
    let c = open_commit(&repo, commit)?;
    if read::blob_in(&repo, &c, &rel)?.is_none() {
        return Err(Error::Git(format!(
            "{} doesn't exist in commit {}",
            abs.display(),
            c.id().shorten_or_id()
        )));
    }
    let source = format!("--source={}", c.id);
    cli::git(
        &root,
        [
            "restore",
            source.as_str(),
            "--staged",
            "--worktree",
            "--",
            &pathspec(&rel),
        ],
    )?;
    Ok(())
}

/// Create branch `name` at `commit`, switching to it when `checkout`.
pub fn create_branch_at(
    root: impl AsRef<Path>,
    name: &str,
    commit: &str,
    checkout: bool,
) -> Result<RepoInfo> {
    let root = repo_root(root.as_ref())?;
    crate::check_branch_name(name)?;
    let id = resolve(&read::open(&root)?, commit)?.to_string();
    if checkout {
        cli::git(&root, ["switch", "--quiet", "-c", name, id.as_str()])?;
    } else {
        cli::git(&root, ["branch", name, id.as_str()])?;
    }
    info_at(&root)
}

/// Detached checkout of `commit`; git refuses to overwrite local changes.
pub fn checkout_commit(root: impl AsRef<Path>, commit: &str) -> Result<RepoInfo> {
    let root = repo_root(root.as_ref())?;
    let id = resolve(&read::open(&root)?, commit)?.to_string();
    cli::git(&root, ["switch", "--quiet", "--detach", id.as_str()])?;
    info_at(&root)
}
