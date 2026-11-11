//! Merge, pull modes and conflict resolution (D24).
//!
//! ## "ours" and "theirs"
//! In this API, **ours** is always the user's own work and **theirs** the
//! incoming change, matching the labels in [`MergeState`]:
//!
//! | operation | ours (git stage) | theirs (git stage) |
//! |---|---|---|
//! | merge, cherry-pick, revert | current branch (2) | merged/picked commit (3) |
//! | rebase | the commit being replayed (3) | the upstream being rebased onto (2) |
//!
//! Git swaps the stages during a rebase; [`conflict_versions`] and
//! [`resolve_with`] undo that swap so the UI never has to care.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use gix::ObjectId;
use serde::{Deserialize, Serialize};

use crate::{
    Error, FileState, RepoInfo, Result, cli, in_repo, info_at, io_err, pathspec, read, repo_root,
    status,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Merge,
    Rebase,
    CherryPick,
    Revert,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeState {
    /// Operation in progress, if any.
    pub operation: Option<Operation>,
    /// Absolute paths of files with unresolved conflicts.
    pub conflicted: Vec<PathBuf>,
    /// Prepared commit message (comment lines removed), if any.
    pub message: Option<String>,
    pub ours_label: String,
    pub theirs_label: String,
}

/// The versions of a conflicted file. Stage contents are `None` when the file
/// doesn't exist on that side. For binary files present contents are empty strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConflictVersions {
    pub path: PathBuf,
    pub base: Option<String>,
    pub ours: Option<String>,
    pub theirs: Option<String>,
    pub merged: Option<String>,
    pub binary: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PullMode {
    FfOnly,
    Merge,
    Rebase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Ours,
    Theirs,
}

/// A commit id stored in a file of the git dir (`MERGE_HEAD`, `rebase-merge/onto`…).
fn id_file(git_dir: &Path, name: &str) -> Option<ObjectId> {
    let s = fs::read_to_string(git_dir.join(name)).ok()?;
    ObjectId::from_hex(s.lines().next()?.trim().as_bytes()).ok()
}

fn rebase_dir(git_dir: &Path) -> Option<PathBuf> {
    ["rebase-merge", "rebase-apply"]
        .iter()
        .map(|d| git_dir.join(d))
        .find(|d| d.is_dir())
}

fn operation(git_dir: &Path) -> Option<Operation> {
    if rebase_dir(git_dir).is_some() {
        Some(Operation::Rebase)
    } else if git_dir.join("MERGE_HEAD").exists() {
        Some(Operation::Merge)
    } else if git_dir.join("CHERRY_PICK_HEAD").exists() {
        Some(Operation::CherryPick)
    } else if git_dir.join("REVERT_HEAD").exists() {
        Some(Operation::Revert)
    } else {
        None
    }
}

fn read_message(path: &Path) -> Option<String> {
    let raw = fs::read_to_string(path).ok()?;
    let msg = raw
        .lines()
        .filter(|l| !l.starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();
    (!msg.is_empty()).then_some(msg)
}

/// Is the stage order swapped relative to our ours/theirs (i.e. a rebase)?
fn swapped(op: Option<Operation>) -> bool {
    op == Some(Operation::Rebase)
}

pub(crate) fn state_at(root: &Path) -> Result<MergeState> {
    let repo = read::open(root)?;
    let git_dir = repo.git_dir().to_path_buf();
    let op = operation(&git_dir);
    let conflicted = status::status(root)?
        .into_iter()
        .filter(|e| e.state == FileState::Conflicted)
        .map(|e| e.path)
        .collect();
    let head = read::head_info(&repo)?;
    let current = head
        .branch
        .clone()
        .or(head.short_id.clone())
        .unwrap_or_else(|| "HEAD".into());
    let upstream = head
        .branch
        .as_ref()
        .and_then(|b| read::tracking_of(&repo, &format!("refs/heads/{b}")));
    let short_or = |name: &str, fallback: &str| {
        id_file(&git_dir, name)
            .map(|id| read::short(&repo, id))
            .unwrap_or_else(|| fallback.to_string())
    };

    let (ours_label, theirs_label, message) = match op {
        Some(Operation::Merge) => {
            let theirs = id_file(&git_dir, "MERGE_HEAD")
                .map(|id| read::name_for(&repo, id, upstream.as_ref()))
                .unwrap_or_else(|| "MERGE_HEAD".into());
            (current, theirs, read_message(&git_dir.join("MERGE_MSG")))
        }
        Some(Operation::Rebase) => {
            let dir = rebase_dir(&git_dir).expect("rebase dir");
            let branch_ref = fs::read_to_string(dir.join("head-name"))
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| s.starts_with("refs/"));
            let branch = branch_ref
                .as_deref()
                .map(|s| s.trim_start_matches("refs/heads/").to_string());
            let prefer = branch_ref
                .as_deref()
                .and_then(|r| read::tracking_of(&repo, r));
            let onto = id_file(&dir, "onto")
                .map(|id| read::name_for(&repo, id, prefer.as_ref()))
                .unwrap_or_else(|| "upstream".into());
            let applying = id_file(&git_dir, "REBASE_HEAD");
            let ours = match (branch, applying) {
                (Some(b), Some(id)) => format!("{b} ({})", read::short(&repo, id)),
                (Some(b), None) => b,
                (None, Some(id)) => read::short(&repo, id),
                (None, None) => "your commit".into(),
            };
            let message = read_message(&dir.join("message"))
                .or_else(|| applying.and_then(|id| read::summary_of(&repo, id)));
            (ours, format!("{onto} (upstream)"), message)
        }
        Some(Operation::CherryPick) => (
            current,
            format!("{} (cherry-pick)", short_or("CHERRY_PICK_HEAD", "commit")),
            read_message(&git_dir.join("MERGE_MSG")),
        ),
        Some(Operation::Revert) => (
            current,
            format!("revert of {}", short_or("REVERT_HEAD", "commit")),
            read_message(&git_dir.join("MERGE_MSG")),
        ),
        None => (current, String::new(), None),
    };
    Ok(MergeState {
        operation: op,
        conflicted,
        message,
        ours_label,
        theirs_label,
    })
}

/// The operation in progress, its conflicted files and labels for the panes.
pub fn merge_state(root: impl AsRef<Path>) -> Result<MergeState> {
    state_at(&repo_root(root.as_ref())?)
}

pub(crate) fn refuse_if_busy(root: &Path) -> Result<()> {
    let repo = read::open(root)?;
    if let Some(op) = operation(repo.git_dir()) {
        return Err(Error::Git(format!(
            "a {} is in progress; finish or abort it first",
            match op {
                Operation::Merge => "merge",
                Operation::Rebase => "rebase",
                Operation::CherryPick => "cherry-pick",
                Operation::Revert => "revert",
            }
        )));
    }
    Ok(())
}

/// Treat a failed git command as success when it stopped on conflicts.
fn ok_if_conflicted(root: &Path, res: Result<Vec<u8>>) -> Result<()> {
    match res {
        Ok(_) => Ok(()),
        Err(Error::Git(msg)) => {
            let st = state_at(root)?;
            if st.operation.is_some() && !st.conflicted.is_empty() {
                Ok(())
            } else {
                Err(Error::Git(msg))
            }
        }
        Err(e) => Err(e),
    }
}

/// Pull with the given strategy. Stopping on conflicts is not an error: the
/// returned info has `conflicted = true`.
pub fn pull_with(root: impl AsRef<Path>, mode: PullMode) -> Result<RepoInfo> {
    let root = repo_root(root.as_ref())?;
    refuse_if_busy(&root)?;
    let args: &[&str] = match mode {
        PullMode::FfOnly => &["pull", "--ff-only", "--quiet"],
        PullMode::Merge => &["pull", "--no-rebase", "--no-edit", "--quiet"],
        PullMode::Rebase => &["pull", "--rebase", "--quiet"],
    };
    let res = cli::git_env(&root, args, &[("GIT_EDITOR", "true")]);
    ok_if_conflicted(&root, res)?;
    info_at(&root)
}

/// Merge `branch` (local or remote-tracking) into the current branch.
/// Conflicts are not an error: inspect the returned state.
pub fn merge(root: impl AsRef<Path>, branch: &str) -> Result<MergeState> {
    let root = repo_root(root.as_ref())?;
    crate::check_branch_name(branch)?;
    refuse_if_busy(&root)?;
    let res = cli::git(&root, ["merge", "--no-edit", "--quiet", branch]);
    ok_if_conflicted(&root, res)?;
    state_at(&root)
}

/// Unmerged index entries of `rel`: blob ids for stages 1, 2, 3.
fn stages(root: &Path, rel: &Path) -> Result<[Option<String>; 3]> {
    let out = cli::git(root, ["ls-files", "-u", "-z", "--", &pathspec(rel)])?;
    let mut st: [Option<String>; 3] = Default::default();
    for rec in out.split(|b| *b == 0).filter(|r| !r.is_empty()) {
        // <mode> SP <object> SP <stage> TAB <file>
        let rec = String::from_utf8_lossy(rec);
        let meta = rec.split('\t').next().unwrap_or("");
        let mut f = meta.split(' ');
        let (_, Some(id), Some(stage)) = (f.next(), f.next(), f.next()) else {
            continue;
        };
        if let Ok(n @ 1..=3) = stage.parse::<usize>() {
            st[n - 1] = Some(id.to_string());
        }
    }
    Ok(st)
}

fn conflicted_rel(root: &Path, path: &Path) -> Result<(PathBuf, PathBuf, MergeState)> {
    let (abs, rel) = in_repo(root, path)?;
    let st = state_at(root)?;
    if !st.conflicted.contains(&abs) {
        return Err(Error::Git(format!("{}: not in conflict", abs.display())));
    }
    Ok((abs, rel, st))
}

pub(crate) fn looks_binary(bytes: &[u8]) -> bool {
    bytes[..bytes.len().min(8000)].contains(&0)
}

/// `diff` attribute unset (e.g. via `binary`) marks a file as binary.
fn binary_attr(root: &Path, rel: &Path) -> bool {
    cli::git(root, ["check-attr", "-z", "diff", "--", &pathspec(rel)])
        .map(|out| out.split(|b| *b == 0).nth(2).is_some_and(|v| v == b"unset"))
        .unwrap_or(false)
}

/// Base, ours, theirs and working-tree versions of a conflicted file.
pub fn conflict_versions(
    root: impl AsRef<Path>,
    path: impl AsRef<Path>,
) -> Result<ConflictVersions> {
    let root = repo_root(root.as_ref())?;
    let (abs, rel, st) = conflicted_rel(&root, path.as_ref())?;
    let ids = stages(&root, &rel)?;
    let mut blobs: Vec<Option<Vec<u8>>> = Vec::with_capacity(3);
    for id in &ids {
        blobs.push(match id {
            Some(id) => Some(cli::git(&root, ["cat-file", "blob", id.as_str()])?),
            None => None,
        });
    }
    let merged = match fs::read(&abs) {
        Ok(b) => Some(b),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(io_err(&abs)(e)),
    };
    let binary = blobs
        .iter()
        .chain(std::iter::once(&merged))
        .flatten()
        .any(|b| looks_binary(b))
        || binary_attr(&root, &rel);
    let text = |b: Option<Vec<u8>>| {
        b.map(|b| {
            if binary {
                String::new()
            } else {
                read::lossy(b)
            }
        })
    };
    let mut it = blobs.into_iter();
    let (base, stage2, stage3) = (
        text(it.next().flatten()),
        text(it.next().flatten()),
        text(it.next().flatten()),
    );
    let (ours, theirs) = if swapped(st.operation) {
        (stage3, stage2)
    } else {
        (stage2, stage3)
    };
    Ok(ConflictVersions {
        path: abs,
        base,
        ours,
        theirs,
        merged: text(merged),
        binary,
    })
}

/// Write `content` atomically (temp file + rename), keeping permissions.
fn write_atomic(path: &Path, content: &[u8]) -> Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| crate::invalid(path, "no parent directory"))?;
    fs::create_dir_all(dir).map_err(io_err(dir))?;
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let tmp = dir.join(format!(".{name}.ilixpatch-{}.tmp", std::process::id()));
    let res = (|| {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(content)?;
        f.sync_all()?;
        if let Ok(meta) = fs::metadata(path) {
            fs::set_permissions(&tmp, meta.permissions())?;
        }
        fs::rename(&tmp, path)
    })();
    if res.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    res.map_err(io_err(path))
}

/// Write the resolved content of a conflicted file and mark it resolved.
pub fn resolve_file(
    root: impl AsRef<Path>,
    path: impl AsRef<Path>,
    content: &str,
) -> Result<MergeState> {
    let root = repo_root(root.as_ref())?;
    let (abs, rel, _) = conflicted_rel(&root, path.as_ref())?;
    write_atomic(&abs, content.as_bytes())?;
    cli::git(&root, ["add", "--", &pathspec(&rel)])?;
    state_at(&root)
}

/// Resolve a conflicted file by taking one whole side. If that side deleted
/// the file, the file is removed.
pub fn resolve_with(
    root: impl AsRef<Path>,
    path: impl AsRef<Path>,
    side: Side,
) -> Result<MergeState> {
    let root = repo_root(root.as_ref())?;
    let (_, rel, st) = conflicted_rel(&root, path.as_ref())?;
    // Map our side to git's stage, undoing git's swap during a rebase.
    let git_ours = (side == Side::Ours) != swapped(st.operation);
    let ids = stages(&root, &rel)?;
    let spec = pathspec(&rel);
    if ids[if git_ours { 1 } else { 2 }].is_some() {
        let flag = if git_ours { "--ours" } else { "--theirs" };
        cli::git(&root, ["checkout", flag, "--", &spec])?;
        cli::git(&root, ["add", "--", &spec])?;
    } else {
        cli::git(&root, ["rm", "--quiet", "--force", "--", &spec])?;
    }
    state_at(&root)
}

fn op_cmd(op: Operation) -> &'static str {
    match op {
        Operation::Merge => "merge",
        Operation::Rebase => "rebase",
        Operation::CherryPick => "cherry-pick",
        Operation::Revert => "revert",
    }
}

/// Abort the operation in progress.
pub fn abort_operation(root: impl AsRef<Path>) -> Result<RepoInfo> {
    let root = repo_root(root.as_ref())?;
    let repo = read::open(&root)?;
    let Some(op) = operation(repo.git_dir()) else {
        return Err(Error::Git(
            "no merge, rebase, cherry-pick or revert in progress".into(),
        ));
    };
    cli::git(&root, [op_cmd(op), "--abort"])?;
    info_at(&root)
}

/// Finish the operation once nothing is conflicted. For a merge, `message`
/// (when non-empty) replaces the prepared message; for cherry-pick/revert it
/// replaces `MERGE_MSG`; for a rebase it is ignored (each replayed commit keeps
/// its message). The returned state may still show an operation, e.g. when
/// the next rebase step stops on new conflicts.
pub fn continue_operation(root: impl AsRef<Path>, message: Option<&str>) -> Result<MergeState> {
    let root = repo_root(root.as_ref())?;
    let st = state_at(&root)?;
    let Some(op) = st.operation else {
        return Err(Error::Git(
            "no merge, rebase, cherry-pick or revert in progress".into(),
        ));
    };
    if !st.conflicted.is_empty() {
        return Err(Error::Git(format!(
            "resolve all conflicts first ({} file(s) left)",
            st.conflicted.len()
        )));
    }
    let message = message.map(str::trim).filter(|m| !m.is_empty());
    let editor = [("GIT_EDITOR", "true")];
    let res = match op {
        Operation::Merge => match message {
            Some(m) => cli::git(&root, ["commit", "--quiet", "-m", m]),
            None => cli::git(&root, ["commit", "--quiet", "--no-edit"]),
        },
        Operation::Rebase => cli::git_env(&root, ["rebase", "--continue"], &editor),
        Operation::CherryPick | Operation::Revert => {
            if let Some(m) = message {
                let git_dir = read::open(&root)?.git_dir().to_path_buf();
                write_atomic(&git_dir.join("MERGE_MSG"), m.as_bytes())?;
            }
            cli::git_env(&root, [op_cmd(op), "--continue"], &editor)
        }
    };
    ok_if_conflicted(&root, res)?;
    state_at(&root)
}
