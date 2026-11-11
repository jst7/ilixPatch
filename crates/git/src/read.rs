//! In-process reads with gix.

use std::path::Path;

use gix::ObjectId;
use gix::refs::FullNameRef;
use gix::revision::walk::Sorting;
use gix::traverse::commit::simple::CommitTimeOrder;

use crate::{Branch, Commit, Error, Result};

pub(crate) fn gx(e: impl std::fmt::Display) -> Error {
    Error::Gix(e.to_string())
}

pub(crate) fn open(root: &Path) -> Result<gix::Repository> {
    gix::open(root).map_err(|e| Error::Gix(format!("{}: {e}", root.display())))
}

pub(crate) fn lossy(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

pub(crate) struct HeadInfo {
    pub branch: Option<String>,
    pub short_id: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
}

/// Remote-tracking ref of a local branch (`refs/remotes/origin/main`), from config.
fn tracking(repo: &gix::Repository, name: &FullNameRef) -> Option<gix::refs::FullName> {
    repo.branch_remote_tracking_ref_name(name, gix::remote::Direction::Fetch)?
        .ok()
}

fn ref_id(repo: &gix::Repository, name: &FullNameRef) -> Option<ObjectId> {
    let mut r = repo.try_find_reference(name).ok()??;
    r.peel_to_id().ok().map(|id| id.detach())
}

/// Commits reachable from `tip` but not from `hidden`.
fn count_only(repo: &gix::Repository, tip: ObjectId, hidden: ObjectId) -> Result<u32> {
    let walk = repo
        .rev_walk([tip])
        .with_hidden([hidden])
        .all()
        .map_err(gx)?;
    let mut n = 0;
    for info in walk {
        info.map_err(gx)?;
        n += 1;
    }
    Ok(n)
}

pub(crate) fn head_info(repo: &gix::Repository) -> Result<HeadInfo> {
    let head_name = repo.head_name().map_err(gx)?;
    let head_id = repo.head().map_err(gx)?.id();
    let tracking_name = head_name.as_ref().and_then(|n| tracking(repo, n.as_ref()));
    let (mut ahead, mut behind) = (0, 0);
    if let (Some(head), Some(up)) = (
        head_id.as_ref().map(|id| id.detach()),
        tracking_name
            .as_ref()
            .and_then(|t| ref_id(repo, t.as_ref())),
    ) {
        ahead = count_only(repo, head, up)?;
        behind = count_only(repo, up, head)?;
    }
    Ok(HeadInfo {
        branch: head_name.map(|n| n.shorten().to_string()),
        short_id: head_id.map(|id| id.shorten_or_id().to_string()),
        upstream: tracking_name.map(|t| t.shorten().to_string()),
        ahead,
        behind,
    })
}

pub(crate) fn branches(repo: &gix::Repository) -> Result<Vec<Branch>> {
    let head_name = repo.head_name().map_err(gx)?;
    let platform = repo.references().map_err(gx)?;
    let mut out = Vec::new();
    for r in platform.local_branches().map_err(gx)? {
        let r = r.map_err(gx)?;
        let name = r.name();
        out.push(Branch {
            name: name.shorten().to_string(),
            current: head_name.as_ref().is_some_and(|h| h.as_ref() == name),
            remote: false,
            upstream: tracking(repo, name).map(|t| t.shorten().to_string()),
        });
    }
    for r in platform.remote_branches().map_err(gx)? {
        let r = r.map_err(gx)?;
        // Skip symbolic refs such as origin/HEAD.
        if r.target().try_id().is_none() {
            continue;
        }
        out.push(Branch {
            name: r.name().shorten().to_string(),
            current: false,
            remote: true,
            upstream: None,
        });
    }
    out.sort_by(|a, b| (a.remote, &a.name).cmp(&(b.remote, &b.name)));
    Ok(out)
}

/// Id of the tree entry at `rel` in `commit`, if present.
fn entry_in(commit: &gix::Commit<'_>, rel: &Path) -> Result<Option<ObjectId>> {
    let tree = commit.tree().map_err(gx)?;
    Ok(tree
        .lookup_entry_by_path(rel)
        .map_err(gx)?
        .map(|e| e.object_id()))
}

fn head_commit_opt(repo: &gix::Repository) -> Result<Option<gix::Commit<'_>>> {
    match repo.head().map_err(gx)?.id() {
        Some(id) => Ok(Some(repo.find_commit(id).map_err(gx)?)),
        None => Ok(None),
    }
}

/// Id of whatever is at `rel` in HEAD (None in an empty repo or when absent).
pub(crate) fn blob_id_at_head(repo: &gix::Repository, rel: &Path) -> Result<Option<ObjectId>> {
    match head_commit_opt(repo)? {
        Some(c) => entry_in(&c, rel),
        None => Ok(None),
    }
}

/// Contents of the blob at `rel` in HEAD (None if absent or not a file).
pub(crate) fn blob_at_head(repo: &gix::Repository, rel: &Path) -> Result<Option<Vec<u8>>> {
    match head_commit_opt(repo)? {
        Some(c) => blob_in(repo, &c, rel),
        None => Ok(None),
    }
}

/// Contents of the blob at `rel` in `commit` (None if absent or not a file).
pub(crate) fn blob_in(
    repo: &gix::Repository,
    commit: &gix::Commit<'_>,
    rel: &Path,
) -> Result<Option<Vec<u8>>> {
    let tree = commit.tree().map_err(gx)?;
    let Some(entry) = tree.lookup_entry_by_path(rel).map_err(gx)? else {
        return Ok(None);
    };
    if !entry.mode().is_blob() {
        return Ok(None);
    }
    let mut blob = repo.find_blob(entry.object_id()).map_err(gx)?;
    Ok(Some(std::mem::take(&mut blob.data)))
}

pub(crate) fn to_commit(c: &gix::Commit<'_>) -> Result<Commit> {
    let author = c.author().map_err(gx)?;
    let time = author.time().map(|t| t.seconds).unwrap_or(0);
    let summary = lossy(c.message_raw_sloppy().to_vec());
    Ok(Commit {
        id: c.id().to_string(),
        short_id: c.id().shorten_or_id().to_string(),
        summary: summary.lines().next().unwrap_or("").trim().to_string(),
        author: author.name.to_string(),
        time,
    })
}

pub(crate) fn head_commit(repo: &gix::Repository) -> Result<Commit> {
    let c = head_commit_opt(repo)?.ok_or_else(|| Error::Gix("HEAD has no commit".into()))?;
    to_commit(&c)
}

pub(crate) fn log(
    repo: &gix::Repository,
    rel: Option<&Path>,
    mut skip: usize,
    limit: usize,
) -> Result<Vec<Commit>> {
    let Some(head) = repo.head().map_err(gx)?.id() else {
        return Ok(Vec::new());
    };
    let walk = repo
        .rev_walk([head])
        .sorting(Sorting::ByCommitTime(CommitTimeOrder::NewestFirst))
        .all()
        .map_err(gx)?;
    let mut out = Vec::new();
    for info in walk {
        if out.len() >= limit {
            break;
        }
        let info = info.map_err(gx)?;
        let commit = info.object().map_err(gx)?;
        if let Some(rel) = rel {
            let mine = entry_in(&commit, rel)?;
            let mut parents = info.parent_ids().peekable();
            let changed = if parents.peek().is_none() {
                mine.is_some()
            } else {
                // Like git's default history simplification: skip a commit
                // when the path is identical in any parent.
                let mut changed = true;
                for p in parents {
                    let pc = repo.find_commit(p).map_err(gx)?;
                    if entry_in(&pc, rel)? == mine {
                        changed = false;
                        break;
                    }
                }
                changed
            };
            if !changed {
                continue;
            }
        }
        if skip > 0 {
            skip -= 1;
            continue;
        }
        out.push(to_commit(&commit)?);
    }
    Ok(out)
}

/// Remote to publish a new branch to: `origin`, else the only remote.
pub(crate) fn default_remote(repo: &gix::Repository) -> Option<String> {
    let names = repo.remote_names();
    if names.iter().any(|n| *n == "origin") {
        return Some("origin".into());
    }
    if names.len() == 1 {
        return names.iter().next().map(|n| n.to_string());
    }
    None
}

/// Upstream tracking ref of a local branch name like `refs/heads/main`.
pub(crate) fn tracking_of(repo: &gix::Repository, full_name: &str) -> Option<gix::refs::FullName> {
    let name: gix::refs::FullName = full_name.try_into().ok()?;
    tracking(repo, name.as_ref())
}

/// A readable name for commit `id`: `prefer` if it points there, else a local
/// branch, else a remote-tracking branch, else the short id.
pub(crate) fn name_for(
    repo: &gix::Repository,
    id: ObjectId,
    prefer: Option<&gix::refs::FullName>,
) -> String {
    if let Some(p) = prefer
        && ref_id(repo, p.as_ref()) == Some(id)
    {
        return p.shorten().to_string();
    }
    if let Ok(platform) = repo.references() {
        for iter in [platform.local_branches(), platform.remote_branches()] {
            let Ok(iter) = iter else { continue };
            for r in iter.flatten() {
                if r.target().try_id() == Some(id.as_ref()) {
                    return r.name().shorten().to_string();
                }
            }
        }
    }
    short(repo, id)
}

pub(crate) fn short(repo: &gix::Repository, id: ObjectId) -> String {
    use gix::prelude::ObjectIdExt;
    id.attach(repo).shorten_or_id().to_string()
}

/// First line of a commit message.
pub(crate) fn summary_of(repo: &gix::Repository, id: ObjectId) -> Option<String> {
    let c = repo.find_commit(id).ok()?;
    let msg = lossy(c.message_raw_sloppy().to_vec());
    Some(msg.lines().next().unwrap_or("").trim().to_string())
}
