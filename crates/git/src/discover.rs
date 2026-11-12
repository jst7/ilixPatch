//! Finding repo roots: the repo enclosing a project plus the ones nested in it.
//! Pure filesystem checks (no git objects are read), so it stays fast.

use std::fs;
use std::path::{Path, PathBuf};

use crate::{Result, io_err};

/// Directories never searched for nested repos.
const SKIP_DIRS: &[&str] = &["node_modules", "target"];

/// `dir` contains `.git` (a directory, or a file as in submodules/worktrees).
pub fn is_repo_root(dir: impl AsRef<Path>) -> bool {
    dir.as_ref().join(".git").symlink_metadata().is_ok()
}

/// The nearest repo root at or above `path`.
pub fn enclosing_repo(path: impl AsRef<Path>) -> Option<PathBuf> {
    path.as_ref()
        .ancestors()
        .find(|p| is_repo_root(p))
        .map(Path::to_path_buf)
}

/// Repo roots relevant to `project_root` (canonicalized): the repo enclosing it
/// (if any) followed by every repo nested inside it, sorted by path. Hidden
/// directories, `node_modules`, `target` and symlinks are not descended into.
pub fn discover_repos(project_root: impl AsRef<Path>) -> Result<Vec<PathBuf>> {
    let root = project_root.as_ref();
    let root = dunce::canonicalize(root).map_err(io_err(root))?;
    let mut out = Vec::new();
    if let Some(enclosing) = enclosing_repo(&root) {
        out.push(enclosing);
    }
    scan(&root, &mut out);
    out.sort();
    out.dedup();
    Ok(out)
}

fn scan(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        // `file_type` doesn't follow symlinks: no cycles, no escaping the project.
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || SKIP_DIRS.contains(&name.as_ref()) {
            continue;
        }
        let path = entry.path();
        if is_repo_root(&path) {
            out.push(path.clone());
        }
        scan(&path, out);
    }
}
