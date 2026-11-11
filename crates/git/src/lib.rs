//! Git integration for ilixPatch (D16), independent of any UI.
//!
//! Any directory of a project (the project itself, a collection or a folder)
//! may be its own repository. [`repos`] finds all of them; every other call is
//! scoped to one repo root.
//!
//! * **Reads** (discovery, HEAD/branches, upstream ahead/behind, log, file
//!   contents at HEAD) use `gix`, in process.
//! * **Status, writes and network** use the system `git` binary so the user's
//!   credentials, SSH agent, hooks, config and signing apply. See `README.md`.
//!
//! All paths returned are absolute and canonical. Callers are responsible for
//! authorization (e.g. "is this inside a registered project"); this crate only
//! makes sure paths stay inside the repo they are used with.

mod blame;
mod cli;
mod clone;
mod discover;
mod history;
mod merge;
mod read;
mod status;

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use blame::{BlameInfo, FileBlame, RequestBlame, blame_file, blame_request};
pub use clone::{
    CloneCancel, CloneEvent, CloneOptions, clone_repo, dir_name_from_url, validate_url,
};
pub use discover::{discover_repos, enclosing_repo, is_repo_root};
pub use history::{
    CommitDetails, CommitFile, CommitFileState, ResetMode, checkout_commit, cherry_pick,
    commit_details, commit_file_diff, create_branch_at, log_page, reset_to, restore_file,
    revert_commit,
};
pub use merge::{
    ConflictVersions, MergeState, Operation, PullMode, Side, abort_operation, conflict_versions,
    continue_operation, merge, merge_state, pull_with, resolve_file, resolve_with,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("git is not installed or not on PATH (needed for this operation)")]
    GitNotFound,
    /// A `git` command failed; the message is its stderr.
    #[error("{0}")]
    Git(String),
    #[error("{0}")]
    Gix(String),
    #[error("{path}: {reason}")]
    InvalidPath { path: PathBuf, reason: String },
    #[error("{path}: {source}")]
    Io { path: PathBuf, source: io::Error },
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn invalid(path: impl Into<PathBuf>, reason: impl Into<String>) -> Error {
    Error::InvalidPath {
        path: path.into(),
        reason: reason.into(),
    }
}

pub(crate) fn io_err(path: &Path) -> impl FnOnce(io::Error) -> Error + '_ {
    move |source| Error::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// One repository: where it is, what is checked out and how it relates to its upstream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoInfo {
    /// Absolute repo root (the dir containing `.git`).
    pub root: PathBuf,
    /// Current branch (also for an unborn branch); `None` when HEAD is detached.
    pub branch: Option<String>,
    /// Short HEAD commit id; `None` in an empty repo.
    pub head: Option<String>,
    /// Upstream tracking branch, e.g. `origin/main`.
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    /// Any staged, unstaged or untracked change.
    pub dirty: bool,
    /// Merge/rebase/cherry-pick in progress, or conflicted files.
    pub conflicted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileState {
    Modified,
    Added,
    Deleted,
    Renamed,
    Untracked,
    Conflicted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileStatus {
    /// Absolute path.
    pub path: PathBuf,
    pub state: FileState,
    /// The change (or part of it) is in the index.
    pub staged: bool,
    /// Previous path, for renames.
    pub old_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Branch {
    /// `main` for local branches, `origin/main` for remote-tracking ones.
    pub name: String,
    pub current: bool,
    pub remote: bool,
    pub upstream: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Commit {
    pub id: String,
    pub short_id: String,
    pub summary: String,
    pub author: String,
    /// Author time, Unix seconds.
    pub time: i64,
}

/// Contents of one file at HEAD (`old`) and in the working tree (`new`).
/// `None` when the file doesn't exist on that side.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileDiff {
    pub path: PathBuf,
    pub old: Option<String>,
    pub new: Option<String>,
}

/// Canonicalize a path that may not exist (e.g. a deleted file): the deepest
/// existing ancestor is canonicalized and the rest is appended. `..` is rejected.
pub fn canonicalize_lenient(path: impl AsRef<Path>) -> Result<PathBuf> {
    let path = path.as_ref();
    if path.components().any(|c| c == Component::ParentDir) {
        return Err(invalid(path, "`..` is not allowed"));
    }
    if !path.is_absolute() {
        return Err(invalid(path, "path must be absolute"));
    }
    let mut missing = Vec::new();
    let mut cur = path;
    loop {
        match fs::canonicalize(cur) {
            Ok(mut base) => {
                for name in missing.iter().rev() {
                    base.push(name);
                }
                return Ok(base);
            }
            Err(e) => {
                let (Some(parent), Some(name)) = (cur.parent(), cur.file_name()) else {
                    return Err(io_err(path)(e));
                };
                missing.push(name.to_os_string());
                cur = parent;
            }
        }
    }
}

/// Canonical repo root, which must contain `.git`.
fn repo_root(root: &Path) -> Result<PathBuf> {
    let root = fs::canonicalize(root).map_err(io_err(root))?;
    if !is_repo_root(&root) {
        return Err(invalid(root, "not a git repository root"));
    }
    Ok(root)
}

/// `path` (absolute) made canonical and checked to be strictly inside `root`
/// and outside `.git`. Returns `(absolute, relative)`.
fn in_repo(root: &Path, path: &Path) -> Result<(PathBuf, PathBuf)> {
    let abs = canonicalize_lenient(path)?;
    let rel = abs
        .strip_prefix(root)
        .map_err(|_| invalid(path, format!("not inside repository {}", root.display())))?
        .to_path_buf();
    match rel.components().next() {
        None => Err(invalid(path, "expected a path inside the repository")),
        Some(Component::Normal(n)) if n == ".git" => Err(invalid(path, "refusing to touch .git")),
        _ => Ok((abs, rel)),
    }
}

/// A repo-relative path as a git pathspec (forward slashes).
fn pathspec(rel: &Path) -> String {
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn nul_joined<'a>(rels: impl IntoIterator<Item = &'a PathBuf>) -> Vec<u8> {
    let mut out = Vec::new();
    for r in rels {
        out.extend_from_slice(pathspec(r).as_bytes());
        out.push(0);
    }
    out
}

/// All repos relevant to a project: the one containing the project root (if
/// any) plus every repo nested inside it, sorted by path. Directories with a
/// `.git` that can't be opened are skipped.
pub fn repos(project_root: impl AsRef<Path>) -> Result<Vec<RepoInfo>> {
    let roots = discover_repos(project_root)?;
    // Status spawns `git` once per repo: do them in parallel.
    let infos: Vec<Option<RepoInfo>> = std::thread::scope(|s| {
        let handles: Vec<_> = roots
            .iter()
            .map(|r| s.spawn(move || info_at(r).ok()))
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().ok().flatten())
            .collect()
    });
    Ok(infos.into_iter().flatten().collect())
}

/// Info about the repo at `root`. When `git` is missing, `dirty`/`conflicted`
/// fall back to what can be read without it.
pub fn repo_info(root: impl AsRef<Path>) -> Result<RepoInfo> {
    info_at(&repo_root(root.as_ref())?)
}

fn info_at(root: &Path) -> Result<RepoInfo> {
    let repo = read::open(root)?;
    let head = read::head_info(&repo)?;
    let git_dir = repo.git_dir();
    let mut conflicted = [
        "MERGE_HEAD",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "rebase-merge",
        "rebase-apply",
    ]
    .iter()
    .any(|f| git_dir.join(f).exists());
    let mut dirty = false;
    match status::status(root) {
        Ok(entries) => {
            dirty = !entries.is_empty();
            conflicted |= entries.iter().any(|e| e.state == FileState::Conflicted);
        }
        Err(Error::GitNotFound) => {}
        Err(e) => return Err(e),
    }
    Ok(RepoInfo {
        root: root.to_path_buf(),
        branch: head.branch,
        head: head.short_id,
        upstream: head.upstream,
        ahead: head.ahead,
        behind: head.behind,
        dirty,
        conflicted,
    })
}

/// Changed files of the repo at `root` (absolute paths). Files that belong to
/// a nested repo are not reported here.
pub fn status(root: impl AsRef<Path>) -> Result<Vec<FileStatus>> {
    status::status(&repo_root(root.as_ref())?)
}

/// Local branches first, then remote-tracking ones, each sorted by name.
pub fn branches(root: impl AsRef<Path>) -> Result<Vec<Branch>> {
    read::branches(&read::open(&repo_root(root.as_ref())?)?)
}

fn check_branch_name(branch: &str) -> Result<()> {
    if branch.trim().is_empty() || branch.starts_with('-') {
        return Err(Error::Git(format!("invalid branch name: {branch:?}")));
    }
    Ok(())
}

/// Switch to `branch`, creating it from HEAD first when `create`. A
/// remote-tracking name like `origin/feature` creates a local tracking branch.
/// Git refuses when local changes would be overwritten.
pub fn checkout(root: impl AsRef<Path>, branch: &str, create: bool) -> Result<RepoInfo> {
    let root = repo_root(root.as_ref())?;
    check_branch_name(branch)?;
    if create {
        cli::git(&root, ["switch", "-c", branch])?;
    } else {
        let repo = read::open(&root)?;
        let has = |r: String| matches!(repo.try_find_reference(r.as_str()), Ok(Some(_)));
        if !has(format!("refs/heads/{branch}")) && has(format!("refs/remotes/{branch}")) {
            cli::git(&root, ["switch", "--track", branch])?;
        } else {
            cli::git(&root, ["switch", branch])?;
        }
    }
    info_at(&root)
}

/// Make `dir` its own repository (`git init`).
pub fn init(dir: impl AsRef<Path>) -> Result<RepoInfo> {
    let dir = dir.as_ref();
    let dir = fs::canonicalize(dir).map_err(io_err(dir))?;
    if !dir.is_dir() {
        return Err(invalid(dir, "not a directory"));
    }
    if is_repo_root(&dir) {
        return Err(invalid(dir, "already a git repository"));
    }
    cli::git(&dir, ["init", "--quiet"])?;
    info_at(&dir)
}

/// HEAD and working-tree contents of `path`, from its nearest enclosing repo.
/// Non-UTF-8 content is decoded lossily. Outside any repo, `old` is `None`.
pub fn diff_file(path: impl AsRef<Path>) -> Result<FileDiff> {
    let path = canonicalize_lenient(path)?;
    let new = match fs::read(&path) {
        Ok(b) => Some(read::lossy(b)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) if path.is_dir() => return Err(invalid(&path, format!("is a directory: {e}"))),
        Err(e) => return Err(io_err(&path)(e)),
    };
    let old = match path.parent().and_then(enclosing_repo) {
        Some(root) => {
            let (_, rel) = in_repo(&root, &path)?;
            read::blob_at_head(&read::open(&root)?, &rel)?.map(read::lossy)
        }
        None => None,
    };
    Ok(FileDiff { path, old, new })
}

/// Status entries matching `paths` (exactly or below a directory).
fn select(root: &Path, paths: &[PathBuf]) -> Result<Vec<FileStatus>> {
    let wanted = paths
        .iter()
        .map(|p| in_repo(root, p).map(|(abs, _)| abs))
        .collect::<Result<Vec<_>>>()?;
    Ok(status::status(root)?
        .into_iter()
        .filter(|e| wanted.iter().any(|w| e.path.starts_with(w)))
        .collect())
}

fn rel(root: &Path, abs: &Path) -> PathBuf {
    abs.strip_prefix(root).unwrap_or(abs).to_path_buf()
}

/// Stage exactly the changes under `paths` (absolute files or directories)
/// and commit only them; other staged changes stay staged but uncommitted.
/// Renames include their old path. During a merge, the whole index is
/// committed (git doesn't allow partial merge commits).
pub fn commit(root: impl AsRef<Path>, paths: &[PathBuf], message: &str) -> Result<Commit> {
    let root = repo_root(root.as_ref())?;
    if message.trim().is_empty() {
        return Err(Error::Git("commit message is empty".into()));
    }
    let entries = select(&root, paths)?;
    if entries.is_empty() {
        return Err(Error::Git("nothing to commit in the selected files".into()));
    }
    // Everything except already-staged deletions (no longer in index nor
    // worktree, so `git add` would reject the pathspec).
    let to_add: BTreeSet<PathBuf> = entries
        .iter()
        .filter(|e| !(e.staged && e.state == FileState::Deleted))
        .map(|e| rel(&root, &e.path))
        .collect();
    let mut all: BTreeSet<PathBuf> = entries.iter().map(|e| rel(&root, &e.path)).collect();
    all.extend(
        entries
            .iter()
            .filter_map(|e| e.old_path.as_deref())
            .map(|p| rel(&root, p)),
    );
    if !to_add.is_empty() {
        cli::git_stdin(
            &root,
            ["add", "-A", "--pathspec-from-file=-", "--pathspec-file-nul"],
            nul_joined(&to_add),
        )?;
    }
    let merging = read::open(&root)?.git_dir().join("MERGE_HEAD").exists();
    if merging {
        cli::git(&root, ["commit", "--quiet", "-m", message])?;
    } else {
        cli::git_stdin(
            &root,
            [
                "commit",
                "--quiet",
                "-m",
                message,
                "--pathspec-from-file=-",
                "--pathspec-file-nul",
            ],
            nul_joined(&all),
        )?;
    }
    read::head_commit(&read::open(&root)?)
}

/// Throw away the changes under `paths`: files known at HEAD are restored (index
/// and worktree), files not at HEAD (untracked or newly added) are deleted.
/// Paths without changes are ignored.
pub fn discard(root: impl AsRef<Path>, paths: &[PathBuf]) -> Result<()> {
    let root = repo_root(root.as_ref())?;
    let entries = select(&root, paths)?;
    let mut targets: BTreeSet<PathBuf> = entries.iter().map(|e| rel(&root, &e.path)).collect();
    targets.extend(
        entries
            .iter()
            .filter_map(|e| e.old_path.as_deref())
            .map(|p| rel(&root, p)),
    );
    let repo = read::open(&root)?;
    let mut restore = Vec::new();
    let mut remove = Vec::new();
    for t in targets {
        if read::blob_id_at_head(&repo, &t)?.is_some() {
            restore.push(t);
        } else {
            remove.push(t);
        }
    }
    if !restore.is_empty() {
        cli::git_stdin(
            &root,
            [
                "restore",
                "--source=HEAD",
                "--staged",
                "--worktree",
                "--pathspec-from-file=-",
                "--pathspec-file-nul",
            ],
            nul_joined(&restore),
        )?;
    }
    if !remove.is_empty() {
        cli::git_stdin(
            &root,
            [
                "rm",
                "--cached",
                "--quiet",
                "--ignore-unmatch",
                "--pathspec-from-file=-",
                "--pathspec-file-nul",
            ],
            nul_joined(&remove),
        )?;
        for r in &remove {
            let abs = root.join(r);
            match fs::remove_file(&abs) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(io_err(&abs)(e)),
                _ => {}
            }
        }
    }
    Ok(())
}

/// `git fetch`.
pub fn fetch(root: impl AsRef<Path>) -> Result<RepoInfo> {
    let root = repo_root(root.as_ref())?;
    cli::git(&root, ["fetch", "--quiet"])?;
    info_at(&root)
}

/// `git pull --ff-only`: never creates a merge commit behind the user's back.
pub fn pull(root: impl AsRef<Path>) -> Result<RepoInfo> {
    let root = repo_root(root.as_ref())?;
    cli::git(&root, ["pull", "--ff-only", "--quiet"])?;
    info_at(&root)
}

/// `git push`. A branch without upstream is pushed to `origin` (or the only
/// remote) and set to track it.
pub fn push(root: impl AsRef<Path>) -> Result<RepoInfo> {
    let root = repo_root(root.as_ref())?;
    let repo = read::open(&root)?;
    let head = read::head_info(&repo)?;
    let remote = if head.branch.is_some() && head.upstream.is_none() {
        read::default_remote(&repo)
    } else {
        None
    };
    match remote {
        Some(remote) => cli::git(
            &root,
            ["push", "--quiet", "--set-upstream", remote.as_str(), "HEAD"],
        )?,
        None => cli::git(&root, ["push", "--quiet"])?,
    };
    info_at(&root)
}

/// History of the repo, or of one file/directory when `path` is given
/// (commits that changed it, compared with their parents; renames are not
/// followed). Newest first, at most `limit` entries.
pub fn log(root: impl AsRef<Path>, path: Option<&Path>, limit: usize) -> Result<Vec<Commit>> {
    let root = repo_root(root.as_ref())?;
    let rel = path
        .map(|p| in_repo(&root, p).map(|(_, r)| r))
        .transpose()?;
    read::log(&read::open(&root)?, rel.as_deref(), 0, limit)
}
