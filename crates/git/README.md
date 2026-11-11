# ilixpatch-git

Git integration for ilixPatch (decision D16). It has no UI code: the Tauri
commands in `app/src-tauri/src/git_commands.rs` are thin wrappers that add
the "inside a registered project" authorization checks.

## Several repos per project

Any directory in a project (the project itself, a collection or a folder) can
be its own repository with its own branch. `repos(project_root)` returns:

- the repo **enclosing** the project root (the nearest ancestor with `.git`, if any)
- every repo **nested** inside it: directories containing `.git`, either a
  directory or a file (submodules, worktrees). Hidden directories,
  `node_modules`, `target` and symlinks are not searched.

Discovery only checks the filesystem for `.git`, so it takes a few ms even
with dozens of folders. Each repo's status is then gathered in parallel.

`status(repo_root)` never reports files that belong to a nested repo (the
nested repo reports them itself), even when the outer repo tracks them.

## gix for reads, the `git` CLI for everything else

| Operation | Implementation |
|---|---|
| Repo discovery | filesystem (`.git` dir or file) |
| HEAD, branch, upstream, ahead/behind, branches, log, file at HEAD (`diff_file`) | `gix` 0.90, features `sha1` + `revision` only (no network, no status) |
| Status (and so `dirty`/`conflicted`) | `git status --porcelain=v2 -z --untracked-files=all --ignore-submodules=all` |
| checkout, init, commit, discard, fetch, pull, push | `git` CLI |

**Why status uses the CLI:** gix's status needs a large set of features
(dirwalk, attributes, filters, index/tree diff) and still differs from git on
details that matter here: rename detection between HEAD/index/worktree, merge
conflicts, `core.fsmonitor`, the untracked cache, sparse checkouts and every
ignore rule. `git status --porcelain=v2` is the stable, documented machine
format, and the cost is one process per call (around 10 ms). If that becomes a
bottleneck, it can move to `gix::status` behind the same function.

**Why writes and network use the CLI:** this way the user's credential
helpers, SSH agent and keys, hooks, `includeIf` config and GPG/SSH signing work
exactly as in their terminal (the same approach IntelliJ takes).

Every CLI call:

- sets `GIT_TERMINAL_PROMPT=0` so it never waits for input on a terminal
- sets `GIT_LITERAL_PATHSPECS=1` so file names are never treated as globs
- sets `GIT_OPTIONAL_LOCKS=0` so status doesn't take the index lock
- removes inherited `GIT_DIR`/`GIT_WORK_TREE`/`GIT_INDEX_FILE`
- passes file lists on stdin (`--pathspec-from-file=- --pathspec-file-nul`),
  so there is no command-line length limit
- returns git's stderr as the error message, or `Error::GitNotFound` when the
  binary is missing (set `ILIXPATCH_GIT` to use a specific binary).

Requires git 2.26 or newer.

## Behaviour of write operations

- **commit(root, paths, message)**: takes the status entries at or under
  `paths` (files or directories), adds the old path of renames, runs
  `git add -A` on them and then `git commit -m <message> <paths>` ("only"
  mode). Other staged changes stay staged and are not committed. During a
  merge, git doesn't allow partial commits, so the whole index is committed.
- **discard(root, paths)**: files that exist at HEAD are restored in both
  index and worktree (`git restore --source=HEAD --staged --worktree`). Files
  not at HEAD (untracked or newly added) are unstaged and **deleted**. Paths
  without changes are ignored.
- **checkout(root, branch, create)**: `git switch [-c] <branch>`. A
  remote-tracking name like `origin/feature` creates a local tracking branch.
  Git refuses when local changes would be overwritten.
- **pull**: `git pull --ff-only`. It never creates a merge commit; a
  diverged branch returns git's error.
- **push**: `git push`. A branch without upstream is pushed to `origin` (or
  the only remote) with `--set-upstream`.
- **init(dir)**: `git init`; fails if `dir` already has `.git`.

## Merge and conflict resolution (D24)

- `pull_with(mode)`: `ff_only` = `pull --ff-only`, `merge` = `pull --no-rebase --no-edit`,
  `rebase` = `pull --rebase`. `merge(branch)` = `merge --no-edit <branch>`. Stopping on
  conflicts is **not** an error: `pull_with` returns the repo info with `conflicted = true`
  and `merge` returns the merge state. Both refuse to start while an operation is in progress.
- `merge_state`: the operation is read from the git dir (`rebase-merge`/`rebase-apply`,
  `MERGE_HEAD`, `CHERRY_PICK_HEAD`, `REVERT_HEAD`; the git dir comes from gix, so linked
  worktrees work). Conflicted files are the unmerged status entries. The message comes from
  `MERGE_MSG` (or `rebase-merge/message`) with `#` comment lines removed. Branch names in the
  labels are resolved by matching ref ids, preferring the upstream.
- **"ours" always means the user's work, "theirs" the incoming change**, as the labels say:

  | operation | ours (git stage) | theirs (git stage) | labels |
  |---|---|---|---|
  | merge / cherry-pick / revert | 2 (HEAD) | 3 | `main` / `feature`, `abc1234 (cherry-pick)`, `revert of abc1234` |
  | rebase | 3 (commit being replayed) | 2 (upstream) | `feature (abc1234)` / `main (upstream)` |

  Git swaps stages 2 and 3 during a rebase. `conflict_versions` and `resolve_with` undo the
  swap (taking "ours" during a rebase runs `git checkout --theirs`).
- `conflict_versions`: stages are read with `ls-files -u` + `cat-file`, and `merged` is the
  working-tree file. `binary` is set when any version has a NUL byte in its first 8000
  bytes, or when the `diff` attribute is unset (`binary`). Binary contents are returned as
  empty strings, and `null` still means "absent on that side".
- `resolve_file` writes atomically (temp file + rename, keeping permissions) then
  `git add`. `resolve_with` runs `checkout --ours|--theirs` + `add`, or `git rm` when that
  side deleted the file. Both only accept files that are currently conflicted.
- `abort_operation`: `merge|rebase|cherry-pick|revert --abort`.
- `continue_operation`: refused while conflicts remain. For a merge it runs `commit --no-edit`
  (or `-m message`). For a rebase it runs `rebase --continue`, and `message` is ignored. For
  cherry-pick or revert, `message` replaces `MERGE_MSG` before `--continue`. These run with
  `GIT_EDITOR=true`. If the next rebase step stops on conflicts, the new state is returned
  instead of an error.

## Annotate / blame (D25)

- `blame_file(path)`: `git blame --incremental --contents - -- <path>`, fed with the file as
  it is on disk. Uncommitted lines (all-zero id) have every field set to `None`. Files not
  at HEAD (untracked, newly added, empty repo) are entirely uncommitted. Results are cached
  per (path, size, mtime, HEAD) in a small in-process cache.
- `blame_request(path)`: the TOML is parsed with `toml_edit` (keeping spans) and blame lines
  are mapped onto fields:
  - `name`, `method`, `url`, and each `headers` / `query` / `body.fields` key: the newest
    line from the key to the end of its value. This handles multi-line values, inline
    tables and dotted keys.
  - `body.content` and `scripts.pre_request` / `tests`: one entry per content line (the
    content split on `\n`, as the editors show it). A string literal is walked byte by
    byte: the newline right after an opening `'''`/`"""` is skipped, `\n` escapes start a new
    content line on the same TOML line (so a single-line string gives every content line
    that one line's blame), and line-ending backslashes join lines (the newest one wins).

## Clone (D26)

`clone_repo(options, cancel, on_event)` runs
`git -c protocol.ext.allow=never clone --progress [--branch b] [--depth n --single-branch] -- <url> <parent>/<dir>`.

- **URLs:** only `https://`, `http://`, `ssh://`, `file://`, scp-like `user@host:path` and
  absolute local paths are accepted. Anything containing `::` (transport helpers such as
  `ext::`), starting with `-`, or containing control characters is rejected.
- **Directory name:** defaults to the last URL segment without `.git`. It must not contain
  separators and must not be `.` or `..`. The target must not exist, or must be an empty
  directory.
- **Progress:** stderr is split on `\r`/`\n` into `progress` events (phase, percent and
  message), at most about 10 per second plus every phase change and 100%. A final `done`
  event follows.
- **Credentials:** a username/token are only allowed with `http(s)` URLs. They are sent as
  `http.extraHeader: Authorization: Basic …` through `GIT_CONFIG_COUNT`/`KEY_0`/`VALUE_0`
  (git 2.31 or newer), so they never appear in argv, on disk or in `.git/config`. When only
  a token is given, the username defaults to `x-access-token`. Tokens, the encoded header
  and `user:pass@` parts of URLs are scrubbed from every message.
- **Cancel:** `CloneCancel::cancel()` kills git's whole process group (git and its helpers;
  `taskkill /T` on Windows) and removes the partial clone. A directory that existed before
  (empty) is emptied again instead of being removed. Failed clones are cleaned up the same
  way.
- **Errors:** auth, not found, SSH host key, SSH public key and network failures get a short
  hint before git's own message.

## Commit details and history actions (D27)

- **Commit arguments:** every `commit` is resolved with gix `rev_parse_single`, which accepts
  full or abbreviated hex ids and revisions like `HEAD~1`, then peeled to a commit. Strings
  that start with `-` or contain spaces or control characters are rejected. Only the
  resolved full id is ever passed to git.
- `commit_details` (gix): the full message, author and committer with emails and times,
  parents, and the branches and tags pointing at the commit (annotated tags are peeled).
  Files come from one `git diff-tree -r -z -M --raw --numstat <first-parent> <id>`
  (`--root` for a root commit), so renames are detected. Binary files have `null` counts and
  `binary: true`. For merges, files are listed against the first parent.
- `commit_file_diff`: `old` is the blob in the first parent (at the old path for a rename)
  and `new` the blob in the commit. Binary versions that exist are returned as `""`, the
  same convention as conflicts.
- `revert_commit` (`--no-edit [--no-commit]`) and `cherry_pick`: merge commits use `-m 1`.
  Stopping on conflicts returns the state rather than an error. Both refuse while another
  operation is in progress, and git's own refusals (e.g. local changes) come back as errors.
- `reset_to` (`--soft|--mixed|--hard`): refused while an operation is in progress.
  `restore_file`: `git restore --source=<id> --staged --worktree`, and a clear error if the
  file isn't in that commit. `create_branch_at`: `git branch`, or `switch -c` when
  checking out. `checkout_commit`: `git switch --detach`. `log_page`: `log` with `skip`.

## Limitations

- SSH passphrases are only available through an agent or askpass helper
  (there is no terminal to type them in).
- `log` with a path doesn't follow renames.
- `diff_file` returns the raw blob at HEAD (no smudge filters such as LFS),
  with non-UTF-8 content decoded lossily.

## Tests

`cargo test -p ilixpatch-git` builds temp repos with the `git` CLI (including a
local bare `file://` remote for fetch, pull and push). Every test passes
without doing anything when `git` isn't installed.
