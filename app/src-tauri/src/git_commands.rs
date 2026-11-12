//! Git commands (D16): thin, authorization-checked wrappers over `ilixpatch-git`.
//!
//! Every repo root must be inside a registered project or be the repo that
//! encloses one; files must be inside that repo (checked by the crate). All
//! commands are `async` so git never runs on the main thread.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ilixpatch_core::{Project, project};
use ilixpatch_git as git;
use serde::Serialize;
use tauri::State;
use tauri::ipc::Channel;

use crate::registry::Registry;

type CmdResult<T> = Result<T, String>;

fn err(e: impl ToString) -> String {
    e.to_string()
}

/// Canonical roots of all registered projects that still exist.
fn project_roots(registry: &Registry) -> CmdResult<Vec<PathBuf>> {
    Ok(registry
        .paths()?
        .iter()
        .filter_map(|p| dunce::canonicalize(p).ok())
        .collect())
}

fn registered_project(registry: &Registry, project_path: &str) -> CmdResult<PathBuf> {
    let root = dunce::canonicalize(project_path).map_err(|e| format!("{project_path}: {e}"))?;
    if !project_roots(registry)?.contains(&root) {
        return Err(format!("{project_path}: not a registered project"));
    }
    Ok(root)
}

/// A repo root the UI may operate on: inside a registered project (or its
/// root), or the repo enclosing a registered project.
fn allowed_repo(registry: &Registry, repo_root: &str) -> CmdResult<PathBuf> {
    let root = dunce::canonicalize(repo_root).map_err(|e| format!("{repo_root}: {e}"))?;
    let ok = project_roots(registry)?.iter().any(|p| {
        root.starts_with(p) || git::enclosing_repo(p).is_some_and(|enclosing| enclosing == root)
    });
    if !ok {
        return Err(format!(
            "{repo_root}: not a repository of a registered project"
        ));
    }
    Ok(root)
}

/// `path` canonicalized (it may not exist), inside or equal to a registered
/// project root and not inside `.ilixpatch`.
fn in_project(registry: &Registry, path: &str, allow_root: bool) -> CmdResult<PathBuf> {
    let p = git::canonicalize_lenient(path).map_err(err)?;
    for root in project_roots(registry)? {
        if let Ok(rel) = p.strip_prefix(&root) {
            if rel.as_os_str().is_empty() && !allow_root {
                return Err(format!("{path}: expected a path inside the project"));
            }
            if rel.starts_with(".ilixpatch") {
                return Err(format!("{path}: refusing to touch .ilixpatch"));
            }
            return Ok(p);
        }
    }
    Err(format!("{path}: not inside a registered project"))
}

#[tauri::command(async)]
pub fn git_repos(
    registry: State<'_, Registry>,
    project_path: String,
) -> CmdResult<Vec<git::RepoInfo>> {
    let root = registered_project(&registry, &project_path)?;
    git::repos(root).map_err(err)
}

#[tauri::command(async)]
pub fn git_status(
    registry: State<'_, Registry>,
    repo_root: String,
) -> CmdResult<Vec<git::FileStatus>> {
    git::status(allowed_repo(&registry, &repo_root)?).map_err(err)
}

#[tauri::command(async)]
pub fn git_branches(
    registry: State<'_, Registry>,
    repo_root: String,
) -> CmdResult<Vec<git::Branch>> {
    git::branches(allowed_repo(&registry, &repo_root)?).map_err(err)
}

#[tauri::command(async)]
pub fn git_checkout(
    registry: State<'_, Registry>,
    repo_root: String,
    branch: String,
    create: bool,
) -> CmdResult<git::RepoInfo> {
    git::checkout(allowed_repo(&registry, &repo_root)?, &branch, create).map_err(err)
}

#[tauri::command(async)]
pub fn git_init(registry: State<'_, Registry>, dir: String) -> CmdResult<git::RepoInfo> {
    git::init(in_project(&registry, &dir, true)?).map_err(err)
}

#[tauri::command(async)]
pub fn git_diff_file(registry: State<'_, Registry>, path: String) -> CmdResult<git::FileDiff> {
    git::diff_file(in_project(&registry, &path, false)?).map_err(err)
}

fn to_paths(paths: Vec<String>) -> Vec<PathBuf> {
    paths.into_iter().map(PathBuf::from).collect()
}

#[tauri::command(async)]
pub fn git_commit(
    registry: State<'_, Registry>,
    repo_root: String,
    paths: Vec<String>,
    message: String,
) -> CmdResult<git::Commit> {
    let root = allowed_repo(&registry, &repo_root)?;
    git::commit(root, &to_paths(paths), &message).map_err(err)
}

#[tauri::command(async)]
pub fn git_discard(
    registry: State<'_, Registry>,
    repo_root: String,
    paths: Vec<String>,
) -> CmdResult<()> {
    let root = allowed_repo(&registry, &repo_root)?;
    git::discard(root, &to_paths(paths)).map_err(err)
}

#[tauri::command(async)]
pub fn git_fetch(registry: State<'_, Registry>, repo_root: String) -> CmdResult<git::RepoInfo> {
    git::fetch(allowed_repo(&registry, &repo_root)?).map_err(err)
}

#[tauri::command(async)]
pub fn git_pull(registry: State<'_, Registry>, repo_root: String) -> CmdResult<git::RepoInfo> {
    git::pull(allowed_repo(&registry, &repo_root)?).map_err(err)
}

#[tauri::command(async)]
pub fn git_push(registry: State<'_, Registry>, repo_root: String) -> CmdResult<git::RepoInfo> {
    git::push(allowed_repo(&registry, &repo_root)?).map_err(err)
}

#[tauri::command(async)]
pub fn git_log(
    registry: State<'_, Registry>,
    repo_root: String,
    path: Option<String>,
    limit: usize,
) -> CmdResult<Vec<git::Commit>> {
    let root = allowed_repo(&registry, &repo_root)?;
    git::log(root, path.as_deref().map(Path::new), limit).map_err(err)
}

// ---------- Merge & conflict resolution (D24) ----------

#[tauri::command(async)]
pub fn git_pull_with(
    registry: State<'_, Registry>,
    repo_root: String,
    mode: git::PullMode,
) -> CmdResult<git::RepoInfo> {
    git::pull_with(allowed_repo(&registry, &repo_root)?, mode).map_err(err)
}

#[tauri::command(async)]
pub fn git_merge(
    registry: State<'_, Registry>,
    repo_root: String,
    branch: String,
) -> CmdResult<git::MergeState> {
    git::merge(allowed_repo(&registry, &repo_root)?, &branch).map_err(err)
}

#[tauri::command(async)]
pub fn git_merge_state(
    registry: State<'_, Registry>,
    repo_root: String,
) -> CmdResult<git::MergeState> {
    git::merge_state(allowed_repo(&registry, &repo_root)?).map_err(err)
}

#[tauri::command(async)]
pub fn git_conflict_versions(
    registry: State<'_, Registry>,
    repo_root: String,
    path: String,
) -> CmdResult<git::ConflictVersions> {
    git::conflict_versions(allowed_repo(&registry, &repo_root)?, path).map_err(err)
}

#[tauri::command(async)]
pub fn git_resolve_file(
    registry: State<'_, Registry>,
    repo_root: String,
    path: String,
    content: String,
) -> CmdResult<git::MergeState> {
    git::resolve_file(allowed_repo(&registry, &repo_root)?, path, &content).map_err(err)
}

#[tauri::command(async)]
pub fn git_resolve_with(
    registry: State<'_, Registry>,
    repo_root: String,
    path: String,
    side: git::Side,
) -> CmdResult<git::MergeState> {
    git::resolve_with(allowed_repo(&registry, &repo_root)?, path, side).map_err(err)
}

#[tauri::command(async)]
pub fn git_abort_operation(
    registry: State<'_, Registry>,
    repo_root: String,
) -> CmdResult<git::RepoInfo> {
    git::abort_operation(allowed_repo(&registry, &repo_root)?).map_err(err)
}

#[tauri::command(async)]
pub fn git_continue_operation(
    registry: State<'_, Registry>,
    repo_root: String,
    message: Option<String>,
) -> CmdResult<git::MergeState> {
    git::continue_operation(allowed_repo(&registry, &repo_root)?, message.as_deref()).map_err(err)
}

// ---------- Annotate / blame (D25) ----------

#[tauri::command(async)]
pub fn git_blame_file(registry: State<'_, Registry>, path: String) -> CmdResult<git::FileBlame> {
    git::blame_file(in_project(&registry, &path, false)?).map_err(err)
}

#[tauri::command(async)]
pub fn git_blame_request(
    registry: State<'_, Registry>,
    path: String,
) -> CmdResult<git::RequestBlame> {
    git::blame_request(in_project(&registry, &path, false)?).map_err(err)
}

// ---------- Clone (D26) ----------

/// The clone in progress (one at a time), for `git_cancel_clone`.
static CLONE: Mutex<Option<git::CloneCancel>> = Mutex::new(None);

fn clone_slot() -> std::sync::MutexGuard<'static, Option<git::CloneCancel>> {
    CLONE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Clears the clone slot however `git_clone` returns.
struct CloneSlotGuard;

impl Drop for CloneSlotGuard {
    fn drop(&mut self) {
        *clone_slot() = None;
    }
}

#[derive(Serialize)]
pub struct GitCloneResult {
    path: PathBuf,
    project: Option<Project>,
}

/// Clone a repository into a user-chosen folder (same trust level as "Open
/// folder"), streaming progress; optionally register it as a project.
#[tauri::command(async)]
pub fn git_clone(
    registry: State<'_, Registry>,
    options: git::CloneOptions,
    on_event: Channel<git::CloneEvent>,
) -> CmdResult<GitCloneResult> {
    let cancel = {
        let mut slot = clone_slot();
        if slot.is_some() {
            return Err("a clone is already in progress".into());
        }
        let c = git::CloneCancel::new();
        *slot = Some(c.clone());
        c
    };
    let _guard = CloneSlotGuard;
    let path = git::clone_repo(&options, &cancel, |e| {
        let _ = on_event.send(e);
    })
    .map_err(err)?;
    let project = if options.add_as_project {
        project::init_project(&path).map_err(err)?;
        let p = project::load_project(&path).map_err(err)?;
        registry.add(&p.path)?;
        Some(p)
    } else {
        None
    };
    Ok(GitCloneResult { path, project })
}

/// Cancel the running clone (no-op when none); the partial clone is removed.
#[tauri::command(async)]
pub fn git_cancel_clone() -> CmdResult<()> {
    if let Some(c) = clone_slot().as_ref() {
        c.cancel();
    }
    Ok(())
}

// ---------- Commit details & history actions (D27) ----------

#[tauri::command(async)]
pub fn git_log_page(
    registry: State<'_, Registry>,
    repo_root: String,
    path: Option<String>,
    skip: usize,
    limit: usize,
) -> CmdResult<Vec<git::Commit>> {
    let root = allowed_repo(&registry, &repo_root)?;
    git::log_page(root, path.as_deref().map(Path::new), skip, limit).map_err(err)
}

#[tauri::command(async)]
pub fn git_commit_details(
    registry: State<'_, Registry>,
    repo_root: String,
    commit: String,
) -> CmdResult<git::CommitDetails> {
    git::commit_details(allowed_repo(&registry, &repo_root)?, &commit).map_err(err)
}

#[tauri::command(async)]
pub fn git_commit_file_diff(
    registry: State<'_, Registry>,
    repo_root: String,
    commit: String,
    path: String,
) -> CmdResult<git::FileDiff> {
    git::commit_file_diff(allowed_repo(&registry, &repo_root)?, &commit, path).map_err(err)
}

#[tauri::command(async)]
pub fn git_revert_commit(
    registry: State<'_, Registry>,
    repo_root: String,
    commit: String,
    no_commit: bool,
) -> CmdResult<git::MergeState> {
    git::revert_commit(allowed_repo(&registry, &repo_root)?, &commit, no_commit).map_err(err)
}

#[tauri::command(async)]
pub fn git_reset_to(
    registry: State<'_, Registry>,
    repo_root: String,
    commit: String,
    mode: git::ResetMode,
) -> CmdResult<git::RepoInfo> {
    git::reset_to(allowed_repo(&registry, &repo_root)?, &commit, mode).map_err(err)
}

#[tauri::command(async)]
pub fn git_restore_file(
    registry: State<'_, Registry>,
    repo_root: String,
    path: String,
    commit: String,
) -> CmdResult<()> {
    git::restore_file(allowed_repo(&registry, &repo_root)?, path, &commit).map_err(err)
}

#[tauri::command(async)]
pub fn git_cherry_pick(
    registry: State<'_, Registry>,
    repo_root: String,
    commit: String,
) -> CmdResult<git::MergeState> {
    git::cherry_pick(allowed_repo(&registry, &repo_root)?, &commit).map_err(err)
}

#[tauri::command(async)]
pub fn git_create_branch_at(
    registry: State<'_, Registry>,
    repo_root: String,
    name: String,
    commit: String,
    checkout: bool,
) -> CmdResult<git::RepoInfo> {
    git::create_branch_at(
        allowed_repo(&registry, &repo_root)?,
        &name,
        &commit,
        checkout,
    )
    .map_err(err)
}

#[tauri::command(async)]
pub fn git_checkout_commit(
    registry: State<'_, Registry>,
    repo_root: String,
    commit: String,
) -> CmdResult<git::RepoInfo> {
    git::checkout_commit(allowed_repo(&registry, &repo_root)?, &commit).map_err(err)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn repo_and_path_checks() {
        let dir = tempfile::tempdir().unwrap();
        let base = dunce::canonicalize(dir.path()).unwrap();
        let outer = base.join("outer");
        let proj = outer.join("proj");
        fs::create_dir_all(proj.join("col")).unwrap();
        fs::create_dir_all(outer.join(".git")).unwrap();
        fs::create_dir_all(base.join("elsewhere/.git")).unwrap();
        let reg = Registry::new(base.join("cfg/projects.toml"));
        reg.add(&proj).unwrap();

        let s = |p: &Path| p.to_string_lossy().into_owned();
        assert_eq!(allowed_repo(&reg, &s(&outer)).unwrap(), outer);
        assert!(allowed_repo(&reg, &s(&proj.join("col"))).is_ok());
        assert!(allowed_repo(&reg, &s(&base.join("elsewhere"))).is_err());
        assert!(registered_project(&reg, &s(&proj)).is_ok());
        assert!(registered_project(&reg, &s(&outer)).is_err());
        assert!(in_project(&reg, &s(&proj), true).is_ok());
        assert!(in_project(&reg, &s(&proj), false).is_err());
        assert!(in_project(&reg, &s(&proj.join("col/gone.toml")), false).is_ok());
        assert!(in_project(&reg, &s(&proj.join(".ilixpatch")), true).is_err());
        assert!(in_project(&reg, &s(&outer.join("x")), true).is_err());
    }
}
