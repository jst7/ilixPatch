//! Working tree status via `git status --porcelain=v2 -z` (see README for why
//! this one read goes through the CLI).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::discover::is_repo_root;
use crate::{FileState, FileStatus, Result, cli};

pub(crate) fn status(root: &Path) -> Result<Vec<FileStatus>> {
    let out = cli::git(
        root,
        [
            "status",
            "--porcelain=v2",
            "-z",
            "--untracked-files=all",
            // Submodules are repos of their own and report themselves.
            "--ignore-submodules=all",
        ],
    )?;
    let mut nested = NestedCheck::new(root);
    Ok(parse(&out)
        .into_iter()
        .filter(|e| !nested.is_nested(&e.path))
        .map(|e| FileStatus {
            path: root.join(&e.path),
            old_path: e.old_path.map(|p| root.join(p)),
            state: e.state,
            staged: e.staged,
        })
        .collect())
}

/// One entry with repo-relative paths.
#[derive(Debug, PartialEq)]
pub(crate) struct Entry {
    pub path: PathBuf,
    pub state: FileState,
    pub staged: bool,
    pub old_path: Option<PathBuf>,
}

fn to_path(bytes: &[u8]) -> PathBuf {
    let s = String::from_utf8_lossy(bytes);
    PathBuf::from(if cfg!(windows) {
        s.replace('/', "\\")
    } else {
        s.into_owned()
    })
}

fn state_of(c: u8) -> Option<FileState> {
    Some(match c {
        b'M' | b'T' => FileState::Modified,
        b'A' => FileState::Added,
        b'D' => FileState::Deleted,
        b'R' | b'C' => FileState::Renamed,
        b'U' => FileState::Conflicted,
        _ => return None,
    })
}

/// Parse `git status --porcelain=v2 -z` output.
pub(crate) fn parse(out: &[u8]) -> Vec<Entry> {
    let mut entries = Vec::new();
    let mut tokens = out.split(|b| *b == 0).filter(|t| !t.is_empty());
    while let Some(tok) = tokens.next() {
        let fields = |n: usize| tok.splitn(n, |b| *b == b' ').collect::<Vec<_>>();
        match tok[0] {
            // 1 XY sub mH mI mW hH hI path
            // 2 XY sub mH mI mW hH hI Xscore path\0origPath
            b'1' | b'2' => {
                let renamed = tok[0] == b'2';
                let f = fields(if renamed { 10 } else { 9 });
                let (Some(xy), Some(sub), Some(path)) = (f.get(1), f.get(2), f.last()) else {
                    continue;
                };
                let old = if renamed { tokens.next() } else { None };
                if xy.len() != 2 || sub.first() == Some(&b'S') {
                    continue;
                }
                let (x, y) = (xy[0], xy[1]);
                let staged = x != b'.';
                let Some(state) = state_of(if staged { x } else { y }) else {
                    continue;
                };
                entries.push(Entry {
                    path: to_path(path),
                    state,
                    staged,
                    old_path: old.map(to_path),
                });
            }
            // u XY sub m1 m2 m3 mW h1 h2 h3 path
            b'u' => {
                let f = fields(11);
                if f.len() == 11 && !f[2].starts_with(b"S") {
                    entries.push(Entry {
                        path: to_path(f[10]),
                        state: FileState::Conflicted,
                        staged: false,
                        old_path: None,
                    });
                }
            }
            // ? path   (a trailing `/` is a nested repo: it reports itself)
            b'?' if tok.len() > 2 && !tok.ends_with(b"/") => entries.push(Entry {
                path: to_path(&tok[2..]),
                state: FileState::Untracked,
                staged: false,
                old_path: None,
            }),
            _ => {}
        }
    }
    entries
}

/// Whether a repo-relative path lies inside a nested repo (cached per dir).
struct NestedCheck<'a> {
    root: &'a Path,
    cache: HashMap<PathBuf, bool>,
}

impl<'a> NestedCheck<'a> {
    fn new(root: &'a Path) -> Self {
        Self {
            root,
            cache: HashMap::new(),
        }
    }

    fn is_nested(&mut self, rel: &Path) -> bool {
        let Some(parent) = rel.parent() else {
            return false;
        };
        self.dir_nested(parent)
    }

    fn dir_nested(&mut self, rel_dir: &Path) -> bool {
        if rel_dir.as_os_str().is_empty() {
            return false;
        }
        if let Some(&v) = self.cache.get(rel_dir) {
            return v;
        }
        let v = is_repo_root(self.root.join(rel_dir))
            || rel_dir.parent().is_some_and(|p| self.dir_nested(p));
        self.cache.insert(rel_dir.to_path_buf(), v);
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_porcelain_v2() {
        let out = b"1 .M N... 100644 100644 100644 a a req.toml\0\
1 A. N... 000000 100644 100644 0 b new.toml\0\
1 .D N... 100644 100644 000000 a a gone.toml\0\
1 MM N... 100644 100644 100644 a b both.toml\0\
2 R. N... 100644 100644 100644 a a R100 dir/new name.toml\0dir/old name.toml\0\
u UU N... 100644 100644 100644 100644 a b c conflict.toml\0\
1 .M SC.. 160000 160000 160000 a a submodule\0\
? untracked.toml\0\
? nested/\0";
        let e = parse(out);
        let got: Vec<_> = e
            .iter()
            .map(|e| (e.path.to_string_lossy().into_owned(), e.state, e.staged))
            .collect();
        assert_eq!(
            got,
            [
                ("req.toml".into(), FileState::Modified, false),
                ("new.toml".into(), FileState::Added, true),
                ("gone.toml".into(), FileState::Deleted, false),
                ("both.toml".into(), FileState::Modified, true),
                (
                    to_path(b"dir/new name.toml").to_string_lossy().into_owned(),
                    FileState::Renamed,
                    true
                ),
                ("conflict.toml".into(), FileState::Conflicted, false),
                ("untracked.toml".into(), FileState::Untracked, false),
            ]
        );
        assert_eq!(e[4].old_path, Some(to_path(b"dir/old name.toml")));
    }
}
