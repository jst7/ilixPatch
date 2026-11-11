// Git UI state (D16): per-repo file status (for tree colours and the Changes list) and the per-repo
// state of the Git tab, kept here so switching tabs (which unmounts GitView) doesn't lose it.
import {
  api,
  type GitCommit,
  type GitCommitDetails,
  type GitFileState,
  type GitFileStatus,
  type GitMergeState,
  type GitResetMode,
} from "$lib/api";
import { untrack } from "svelte";
import { t } from "$lib/i18n/index.svelte";
import { ws, isWithin } from "./workspace.svelte";

export type GitSubTab = "changes" | "history";
export type GitOp =
  | "fetch"
  | "pull"
  | "push"
  | "commit"
  | "checkout"
  | "discard"
  | "merge"
  | "abort"
  | "continue"
  | "resolve"
  | "history";

/** The fields of a commit that history actions need. */
export type CommitRef = Pick<GitCommit, "id" | "short_id" | "summary">;

/** History action that needs a dialog (the others use a native confirmation). */
export interface CommitAction {
  kind: "revert" | "reset" | "branch";
  commit: CommitRef;
}

export interface DiffPrefs {
  /** Show the line diff below the semantic diff of request files. */
  showText: boolean;
  mode: "unified" | "split";
}

const LS_DIFF = "ilixpatch.gitDiff";

function loadDiffPrefs(): DiffPrefs {
  const prefs: DiffPrefs = { showText: true, mode: "unified" };
  try {
    const raw = JSON.parse(localStorage.getItem(LS_DIFF) ?? "{}") as Partial<DiffPrefs>;
    if (typeof raw.showText === "boolean") prefs.showText = raw.showText;
    if (raw.mode === "split" || raw.mode === "unified") prefs.mode = raw.mode;
  } catch {}
  return prefs;
}

const LOG_PAGE = 100;

/** State of one repo's Git tab (a class so it can be created lazily, even while rendering). */
export class RepoView {
  tab = $state<GitSubTab>("changes");
  /** Files the user unticked (everything else in the status is selected for commit, like IntelliJ). */
  unchecked = $state<Record<string, true>>({});
  message = $state("");
  /** File shown in the diff pane. */
  activeFile = $state<string | null>(null);
  /** Width of the changes list, in px. */
  listWidth = $state(360);
  // History
  log = $state<GitCommit[]>([]);
  /** Show the history of this file only (null = whole repo). */
  logPath = $state<string | null>(null);
  logLoading = $state(false);
  logError = $state<string | null>(null);
  /** Whether the last page was full (there may be more commits). */
  logMore = $state(false);
  /** Selected commit (full id) and its details. */
  selected = $state<string | null>(null);
  details = $state<GitCommitDetails | null>(null);
  detailsLoading = $state(false);
  detailsError = $state<string | null>(null);
  /** File of the selected commit shown in the diff. */
  commitFile = $state<string | null>(null);
  fileFilter = $state("");
  /** Width of the history list, in px. */
  historyWidth = $state(420);
  /** Dialog-based history action in progress (revert / reset / new branch). */
  action = $state<CommitAction | null>(null);
  /** Operation (merge/rebase…) whose MERGE_MSG was already put in the commit box. */
  prefilledFor = $state<string | null>(null);
  /** Operations in progress. */
  busy = $state<Partial<Record<GitOp, boolean>>>({});
}

export interface GitToast {
  id: number;
  kind: "ok" | "err";
  text: string;
}

/** Tree colour priority when a folder contains several kinds of change. */
const PRIORITY: Record<GitFileState, number> = {
  untracked: 1,
  added: 2,
  deleted: 3,
  renamed: 3,
  modified: 3,
  conflicted: 4,
};

/** Readable message for a failed git command (the backend returns git's stderr). */
export function gitError(e: unknown): string {
  const raw = e instanceof Error ? e.message : typeof e === "string" ? e : JSON.stringify(e);
  const text = (raw ?? "").trim();
  return text || t("git.error.unknown");
}

function parentDir(p: string): string {
  const i = Math.max(p.lastIndexOf("/"), p.lastIndexOf("\\"));
  return i > 0 ? p.slice(0, i) : "";
}

class GitStore {
  /** Status entries by repo root. */
  status = $state<Record<string, GitFileStatus[]>>({});
  statusError = $state<Record<string, string | null>>({});
  /** Bumped after every status refresh of a repo (the diff pane reloads on it). */
  statusVersion = $state<Record<string, number>>({});
  /** Merge/rebase/cherry-pick/revert state by repo root (null when unknown). */
  merge = $state<Record<string, GitMergeState | null>>({});
  private views = new Map<string, RepoView>();
  toasts = $state<GitToast[]>([]);
  diffPrefs = $state<DiffPrefs>(loadDiffPrefs());

  setDiffPref<K extends keyof DiffPrefs>(key: K, value: DiffPrefs[K]) {
    this.diffPrefs[key] = value;
    try {
      localStorage.setItem(LS_DIFF, JSON.stringify(this.diffPrefs));
    } catch {}
  }

  /** Git state of every changed file, by absolute path. */
  fileState = $derived.by(() => {
    const map = new Map<string, GitFileState>();
    for (const list of Object.values(this.status)) for (const f of list) map.set(f.path, f.state);
    return map;
  });

  /** Strongest change state below each directory (up to and including its repo root). */
  dirState = $derived.by(() => {
    const map = new Map<string, GitFileState>();
    for (const [root, list] of Object.entries(this.status)) {
      for (const f of list) {
        // A folder containing deleted or renamed files is shown as modified.
        const state: GitFileState = f.state === "deleted" || f.state === "renamed" ? "modified" : f.state;
        let dir = parentDir(f.path);
        while (dir && isWithin(dir, root)) {
          const cur = map.get(dir);
          // Invariant: an ancestor's priority is never lower than a descendant's, so we can stop here.
          if (cur && PRIORITY[cur] >= PRIORITY[state]) break;
          map.set(dir, state);
          dir = parentDir(dir);
        }
      }
    }
    return map;
  });

  // ----- status -----
  private inflight = new Map<string, Promise<void>>();
  private again = new Set<string>();

  /** Re-read the status of a repo. Concurrent calls are coalesced into at most one extra run. */
  refreshStatus(repo: string): Promise<void> {
    const running = this.inflight.get(repo);
    if (running) {
      this.again.add(repo);
      return running;
    }
    const run = (async () => {
      do {
        this.again.delete(repo);
        const [status, merge] = await Promise.allSettled([api.gitStatus(repo), api.gitMergeState(repo)]);
        if (status.status === "fulfilled") {
          this.status[repo] = status.value;
          this.statusError[repo] = null;
        } else {
          this.status[repo] = [];
          this.statusError[repo] = gitError(status.reason);
        }
        this.merge[repo] = merge.status === "fulfilled" ? merge.value : null;
        this.statusVersion[repo] = (this.statusVersion[repo] ?? 0) + 1;
      } while (this.again.has(repo));
      this.inflight.delete(repo);
    })();
    this.inflight.set(repo, run);
    return run;
  }

  /** Forget repos that no longer exist in any project. */
  prune(known: Set<string>) {
    for (const repo of Object.keys(this.status)) {
      if (!known.has(repo)) {
        delete this.status[repo];
        delete this.statusError[repo];
        delete this.merge[repo];
      }
    }
  }

  // ----- tab state -----
  view(repo: string): RepoView {
    let v = this.views.get(repo);
    if (!v) this.views.set(repo, (v = new RepoView()));
    return v;
  }

  /** Load the first page of the history (or, with `more`, the next page: infinite scroll). */
  async loadLog(repo: string, more = false) {
    const v = this.view(repo);
    if (more && (v.logLoading || !v.logMore)) return;
    const path = v.logPath;
    const skip = more ? v.log.length : 0;
    // A refresh re-reads as many commits as were loaded, so the list doesn't shrink under the user.
    const limit = more ? LOG_PAGE : Math.max(LOG_PAGE, v.log.length);
    v.logLoading = true;
    try {
      const page = await api.gitLogPage(repo, path, skip, limit);
      if (v.logPath !== path) return;
      if (more) {
        const seen = new Set(v.log.map((c) => c.id));
        v.log = [...v.log, ...page.filter((c) => !seen.has(c.id))];
      } else v.log = page;
      v.logMore = page.length >= limit;
      v.logError = null;
    } catch (e) {
      v.logError = gitError(e);
    } finally {
      v.logLoading = false;
    }
  }

  /** Switch to the History sub-tab showing one file's commits (or the whole repo for null). */
  showHistory(repo: string, path: string | null) {
    const v = this.view(repo);
    v.tab = "history";
    v.logPath = path;
    v.log = [];
    v.logMore = false;
    void this.loadLog(repo);
  }

  private detailsSeq = 0;

  /** Select a commit in the History and load its details (works for commits outside the loaded pages). */
  async selectCommit(repo: string, id: string) {
    const v = this.view(repo);
    if (v.selected !== id) {
      v.selected = id;
      v.commitFile = null;
      v.fileFilter = "";
    }
    const seq = ++this.detailsSeq;
    v.detailsLoading = true;
    try {
      const d = await api.gitCommitDetails(repo, id);
      if (seq !== this.detailsSeq) return;
      v.details = d;
      v.detailsError = null;
      // Annotate passes short ids: keep the full one so the list highlights it.
      if (v.selected === id) v.selected = d.id;
      if (!v.commitFile && d.files.length) v.commitFile = d.files[0].path;
    } catch (e) {
      if (seq !== this.detailsSeq) return;
      v.details = null;
      v.detailsError = gitError(e);
    } finally {
      if (seq === this.detailsSeq) v.detailsLoading = false;
    }
  }

  /** Open the Git tab of `repo` on the History, with `commit` selected (e.g. from the editor's annotations). */
  showCommit(repo: string, commit: string) {
    const v = this.view(repo);
    v.tab = "history";
    if (v.logPath !== null || v.log.length === 0) {
      v.logPath = null;
      v.log = [];
      v.logMore = false;
      void this.loadLog(repo);
    }
    ws.openTab({ kind: "git", repo });
    void this.selectCommit(repo, commit);
  }

  /** Re-read the history and the selected commit's details after HEAD moved. */
  refreshHistory(repo: string) {
    const v = this.view(repo);
    if (v.log.length || v.tab === "history") void this.loadLog(repo);
    if (v.selected) void this.selectCommit(repo, v.selected);
  }

  // ----- history actions (D27) -----

  /** After an operation that may stop on conflicts: show them in the Changes sub-tab with the merge banner. */
  private afterSequencer(repo: string, st: GitMergeState, okKey: "git.hist.reverted" | "git.hist.cherryPicked" | "git.hist.revertStaged", c: CommitRef) {
    this.merge[repo] = st;
    if (st.conflicted.length) {
      this.toast("err", t("git.merge.conflicts", { count: st.conflicted.length }));
      this.view(repo).tab = "changes";
    } else this.toast("ok", t(okKey, { id: c.short_id }));
  }

  private async finish(repo: string) {
    await this.afterChange(repo);
    this.refreshHistory(repo);
  }

  async revertCommit(repo: string, c: CommitRef, noCommit: boolean) {
    const st = await this.run(repo, "history", () => api.gitRevertCommit(repo, c.id, noCommit));
    if (st) {
      this.afterSequencer(repo, st, noCommit ? "git.hist.revertStaged" : "git.hist.reverted", c);
      if (noCommit && !st.conflicted.length) this.view(repo).tab = "changes";
    }
    await this.finish(repo);
  }

  async cherryPick(repo: string, c: CommitRef) {
    const st = await this.run(repo, "history", () => api.gitCherryPick(repo, c.id));
    if (st) this.afterSequencer(repo, st, "git.hist.cherryPicked", c);
    await this.finish(repo);
  }

  async resetTo(repo: string, c: CommitRef, mode: GitResetMode): Promise<boolean> {
    const res = await this.run(repo, "history", () => api.gitResetTo(repo, c.id, mode));
    if (res) this.toast("ok", t("git.hist.resetDone", { id: c.short_id, mode }));
    await this.finish(repo);
    return !!res;
  }

  async checkoutCommit(repo: string, c: CommitRef) {
    const res = await this.run(repo, "history", () => api.gitCheckoutCommit(repo, c.id));
    if (res) this.toast("ok", t("git.hist.checkedOut", { id: c.short_id }));
    await this.finish(repo);
  }

  async createBranchAt(repo: string, name: string, c: CommitRef, checkout: boolean): Promise<boolean> {
    const res = await this.run(repo, "history", () => api.gitCreateBranchAt(repo, name, c.id, checkout));
    if (res) this.toast("ok", t(checkout ? "git.branchCreated" : "git.hist.branchCreated", { name }));
    await this.finish(repo);
    return !!res;
  }

  async restoreFile(repo: string, path: string, c: CommitRef) {
    const res = await this.run(repo, "history", () => api.gitRestoreFile(repo, path, c.id));
    if (res !== undefined) this.toast("ok", t("git.hist.restored", { id: c.short_id }));
    await this.finish(repo);
  }

  /** Open the 3-way merge resolver for a conflicted file (tab provided by the merge feature, D24). */
  openMerge(repo: string, path: string) {
    ws.openMerge(repo, path);
  }

  // ----- toasts -----
  private toastId = 0;

  toast(kind: GitToast["kind"], text: string) {
    const id = ++this.toastId;
    this.toasts.push({ id, kind, text });
    setTimeout(() => this.dismiss(id), kind === "err" ? 12000 : 5000);
  }

  dismiss(id: number) {
    this.toasts = this.toasts.filter((x) => x.id !== id);
  }

  // ----- after write operations -----
  /** Projects whose repo list includes `repo`. */
  projectsOf(repo: string): string[] {
    const found = Object.entries(ws.repos)
      .filter(([, list]) => list.some((r) => r.root === repo))
      .map(([p]) => p);
    if (found.length) return found;
    return ws.entries.filter((e) => isWithin(repo, e.path) || isWithin(e.path, repo)).map((e) => e.path);
  }

  /**
   * Files (and possibly branch/HEAD) changed: reload the affected projects from disk, which also refreshes
   * their repos and so this repo's status. `files=false` only refreshes repo info and status.
   */
  async afterChange(repo: string, files = true) {
    const projects = this.projectsOf(repo);
    await Promise.all(projects.map((p) => (files ? ws.reload(p) : ws.refreshRepos(p))));
    await this.refreshStatus(repo);
  }

  /** Run a git operation with a busy flag; errors become toasts. Returns the result or undefined on failure. */
  async run<T>(repo: string, op: GitOp, fn: () => Promise<T>): Promise<T | undefined> {
    const v = this.view(repo);
    if (v.busy[op]) return undefined;
    v.busy[op] = true;
    try {
      return await fn();
    } catch (e) {
      this.toast("err", gitError(e));
      return undefined;
    } finally {
      v.busy[op] = false;
    }
  }
}

export const git = new GitStore();

// Refresh the status of a project's repos every time that project's repo list is re-read
// (ws.refreshRepos runs after each project reload, on window focus and after saves).
if (typeof window !== "undefined") {
  $effect.root(() => {
    let seen = new WeakSet<object>();
    $effect(() => {
      const lists = Object.values(ws.repos);
      untrack(() => {
        const known = new Set<string>();
        const next = new WeakSet<object>();
        for (const list of lists) {
          next.add(list);
          for (const r of list) {
            known.add(r.root);
            if (!seen.has(list)) void git.refreshStatus(r.root);
          }
        }
        seen = next;
        git.prune(known);
      });
    });
  });
}
