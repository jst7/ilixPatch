<script lang="ts">
  // Git tab of one repository (D16, IntelliJ-like): branch switcher, fetch/pull/push, the Changes list with
  // commit box and diff pane, and the History list. Per-repo state lives in the git store.
  import { onMount } from "svelte";
  import { ask } from "@tauri-apps/plugin-dialog";
  import { api, type GitFileStatus, type GitPullMode } from "$lib/api";
  import { t } from "$lib/i18n/index.svelte";
  import { ws, type MenuItem } from "$lib/stores/workspace.svelte";
  import { git, gitError } from "$lib/stores/git.svelte";
  import { basename, relativePath, stateColor, stateLetter } from "$lib/git/status";
  import { methodColor, methodLabel } from "./methods";
  import BranchSelect from "./BranchSelect.svelte";
  import GitDiff from "./GitDiff.svelte";
  import GitIcon from "./GitIcon.svelte";
  import GitHistory from "./GitHistory.svelte";

  let { repo }: { repo: string } = $props();

  const view = $derived(git.view(repo));
  const info = $derived(ws.repoByRoot.get(repo) ?? null);
  const files = $derived(git.status[repo] ?? []);
  const statusError = $derived(git.statusError[repo] ?? null);
  const loaded = $derived(repo in git.status);

  /** Changes grouped by directory relative to the repo root, sorted. */
  const groups = $derived.by(() => {
    const map = new Map<string, GitFileStatus[]>();
    for (const f of files) {
      const rel = relativePath(f.path, repo);
      const i = rel.lastIndexOf("/");
      const dir = i >= 0 ? rel.slice(0, i) : "";
      const list = map.get(dir) ?? [];
      list.push(f);
      map.set(dir, list);
    }
    return [...map.entries()]
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([dir, list]) => ({ dir, files: list.sort((x, y) => x.path.localeCompare(y.path)) }));
  });

  const isChecked = (path: string) => !view.unchecked[path];
  const selected = $derived(files.filter((f) => isChecked(f.path)));
  const activeFile = $derived(files.find((f) => f.path === view.activeFile) ?? null);
  const canCommit = $derived(selected.length > 0 && view.message.trim() !== "" && !view.busy.commit);

  // ----- merge / rebase in progress (D24) -----
  const merge = $derived(git.merge[repo] ?? null);
  const operation = $derived(merge?.operation ?? null);
  const conflictedSet = $derived(new Set(merge?.conflicted ?? []));
  const isConflicted = (f: GitFileStatus) => f.state === "conflicted" || conflictedSet.has(f.path);

  // Put the prepared MERGE_MSG in the commit box once per operation (without overwriting the user's text).
  $effect(() => {
    const key = operation ? `${operation}:${merge?.theirs_label ?? ""}` : null;
    if (!key) {
      if (view.prefilledFor) view.prefilledFor = null;
      return;
    }
    if (view.prefilledFor === key) return;
    view.prefilledFor = key;
    if (merge?.message && !view.message.trim()) view.message = merge.message.trim();
  });

  async function resolveWith(path: string, side: "ours" | "theirs") {
    const st = await git.run(repo, "resolve", () => api.gitResolveWith(repo, path, side));
    if (!st) return;
    git.merge[repo] = st;
    await git.afterChange(repo);
  }

  async function continueOperation() {
    if (!merge || merge.conflicted.length > 0) return;
    const message = view.message.trim() || null;
    const st = await git.run(repo, "continue", () => api.gitContinueOperation(repo, message));
    if (!st) {
      await git.afterChange(repo);
      return;
    }
    git.merge[repo] = st;
    if (!st.operation) {
      view.message = "";
      git.toast("ok", t("git.merge.done"));
    } else if (st.conflicted.length) git.toast("err", t("git.merge.conflicts", { count: st.conflicted.length }));
    await git.afterChange(repo);
    git.refreshHistory(repo);
  }

  async function abortOperation() {
    let ok = false;
    try {
      ok = await ask(t("git.merge.abortConfirm"), {
        title: t("git.merge.abortTitle"),
        kind: "warning",
        okLabel: t("git.merge.abort"),
        cancelLabel: t("tabs.cancel"),
      });
    } catch (e) {
      git.toast("err", gitError(e));
    }
    if (!ok) return;
    const res = await git.run(repo, "abort", () => api.gitAbortOperation(repo));
    if (res !== undefined) {
      view.message = "";
      git.toast("ok", t("git.merge.aborted"));
    }
    await git.afterChange(repo);
  }

  const PULL_MODES: [GitPullMode, () => string][] = [
    ["ff_only", () => t("git.pullFfOnly")],
    ["merge", () => t("git.pullMerge")],
    ["rebase", () => t("git.pullRebase")],
  ];

  function pullMenu(e: MouseEvent) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    ws.menu = {
      x: r.left,
      y: r.bottom + 2,
      items: PULL_MODES.map(([mode, label]) => ({ label: label(), action: () => void network("pull", mode) })),
    };
  }

  onMount(() => {
    void git.refreshStatus(repo);
    if (view.tab === "history" && view.log.length === 0) void git.loadLog(repo);
  });

  // ----- selection -----
  function setChecked(paths: string[], checked: boolean) {
    for (const p of paths) {
      if (checked) delete view.unchecked[p];
      else view.unchecked[p] = true;
    }
  }

  function groupState(list: GitFileStatus[]): "all" | "none" | "some" {
    const n = list.filter((f) => isChecked(f.path)).length;
    return n === list.length ? "all" : n === 0 ? "none" : "some";
  }

  /** Sets the tri-state checkbox's indeterminate flag (not an attribute). */
  function indeterminate(el: HTMLInputElement, value: boolean) {
    el.indeterminate = value;
    return { update: (v: boolean) => (el.indeterminate = v) };
  }

  // ----- display -----
  function fileLabel(f: GitFileStatus) {
    const loc = ws.index.get(f.path);
    const draft = ws.drafts[f.path];
    if (!loc) return null;
    return { name: draft?.name || loc.file.request.name, method: draft?.method ?? loc.file.request.method };
  }

  const stateNames: Record<GitFileStatus["state"], () => string> = {
    modified: () => t("git.state.modified"),
    added: () => t("git.state.added"),
    deleted: () => t("git.state.deleted"),
    renamed: () => t("git.state.renamed"),
    untracked: () => t("git.state.untracked"),
    conflicted: () => t("git.state.conflicted"),
  };

  // ----- actions -----
  async function refresh() {
    await git.afterChange(repo, false);
    git.refreshHistory(repo);
  }

  async function network(op: "fetch" | "pull" | "push", mode: GitPullMode = "ff_only") {
    const res = await git.run(repo, op, () =>
      op === "fetch" ? api.gitFetch(repo) : op === "pull" ? api.gitPullWith(repo, mode) : api.gitPush(repo),
    );
    if (res === undefined) {
      // A pull may have stopped half-way (conflicts): show the new merge/rebase state.
      if (op === "pull") await git.afterChange(repo);
      return false;
    }
    git.toast("ok", t(`git.${op}Done`));
    await git.afterChange(repo, op === "pull");
    git.refreshHistory(repo);
    return true;
  }

  async function commit(push: boolean) {
    if (!canCommit) return;
    const paths = selected.map((f) => f.path);
    const message = view.message.trim();
    const c = await git.run(repo, "commit", () => api.gitCommit(repo, paths, message));
    if (!c) return;
    view.message = "";
    view.unchecked = {};
    git.toast("ok", t("git.committed", { id: c.short_id, summary: c.summary }));
    await git.afterChange(repo);
    git.refreshHistory(repo);
    if (push) await network("push");
  }

  async function discard(list: GitFileStatus[]) {
    if (list.length === 0) return;
    let ok = false;
    try {
      ok = await ask(
        list.length === 1
          ? t("git.discardOne", { name: fileLabel(list[0])?.name ?? basename(list[0].path) })
          : t("git.discardMany", { count: list.length }),
        { title: t("git.discardTitle"), kind: "warning", okLabel: t("git.discard"), cancelLabel: t("tabs.cancel") },
      );
    } catch (e) {
      git.toast("err", gitError(e));
    }
    if (!ok) return;
    const done = await git.run(repo, "discard", () => api.gitDiscard(repo, list.map((f) => f.path)));
    if (done === undefined) return;
    git.toast("ok", t("git.discarded", { count: list.length }));
    await git.afterChange(repo);
  }

  function openMenu(e: MouseEvent, f: GitFileStatus) {
    e.preventDefault();
    view.activeFile = f.path;
    const items: MenuItem[] = [];
    if (isConflicted(f)) {
      items.push(
        { label: t("git.merge.resolve"), action: () => git.openMerge(repo, f.path) },
        { label: t("git.merge.acceptYours"), action: () => resolveWith(f.path, "ours") },
        { label: t("git.merge.acceptTheirs"), action: () => resolveWith(f.path, "theirs") },
      );
    }
    if (ws.index.has(f.path)) items.push({ label: t("git.openRequest"), action: () => ws.select(f.path) });
    items.push({ label: t("git.showHistory"), action: () => git.showHistory(repo, f.path) });
    const checked = selected.filter((s) => s.path !== f.path);
    // Discard applies to every ticked file when the clicked one is ticked, like IntelliJ's selection.
    const targets = isChecked(f.path) && checked.length > 0 ? [f, ...checked] : [f];
    items.push({
      label: targets.length > 1 ? t("git.discardSelected", { count: targets.length }) : t("git.discardChanges"),
      danger: true,
      action: () => discard(targets),
    });
    ws.menu = { x: e.clientX, y: e.clientY, items };
  }

  function onrowkeydown(e: KeyboardEvent, f: GitFileStatus) {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const order = groups.flatMap((g) => g.files);
      const i = order.findIndex((x) => x.path === f.path) + (e.key === "ArrowDown" ? 1 : -1);
      const next = order[i];
      if (next) {
        view.activeFile = next.path;
        listEl?.querySelector<HTMLElement>(`[data-path="${CSS.escape(next.path)}"]`)?.focus();
      }
    } else if (e.key === " ") {
      e.preventDefault();
      setChecked([f.path], !isChecked(f.path));
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (ws.index.has(f.path)) ws.select(f.path);
    } else if (e.key === "ContextMenu" || (e.shiftKey && e.key === "F10")) {
      const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
      openMenu(new MouseEvent("contextmenu", { clientX: r.left + 24, clientY: r.bottom }), f);
    }
  }

  let listEl = $state<HTMLDivElement>();
  let textarea = $state<HTMLTextAreaElement>();

  // Cmd/Ctrl+Enter in the commit message commits. The page's global shortcut handler (window, capture phase)
  // stops propagation for Mod+Enter, so listen on window too: same-target listeners still run.
  $effect(() => {
    const onkey = (e: KeyboardEvent) => {
      if (e.key === "Enter" && (e.metaKey || e.ctrlKey) && textarea && e.target === textarea) {
        e.preventDefault();
        void commit(e.shiftKey);
      }
    };
    window.addEventListener("keydown", onkey, true);
    return () => window.removeEventListener("keydown", onkey, true);
  });

  // Resizable split between the list and the diff pane.
  let changesEl = $state<HTMLDivElement>();
  function resize(e: PointerEvent) {
    e.preventDefault();
    const target = e.currentTarget as HTMLElement;
    target.setPointerCapture(e.pointerId);
    const move = (ev: PointerEvent) => {
      if (!changesEl) return;
      const r = changesEl.getBoundingClientRect();
      view.listWidth = Math.round(Math.min(Math.max(ev.clientX - r.left, 220), Math.max(260, r.width - 240)));
    };
    const up = () => {
      target.removeEventListener("pointermove", move);
      target.removeEventListener("pointerup", up);
    };
    target.addEventListener("pointermove", move);
    target.addEventListener("pointerup", up);
  }

  // ----- history -----
  function switchTab(tab: "changes" | "history") {
    view.tab = tab;
    if (tab === "history" && view.log.length === 0 && !view.logLoading) void git.loadLog(repo);
  }

</script>

<section class="git-view">
  <header>
    <span class="title" title={repo}><GitIcon size={15} /> <span class="repo-name">{basename(repo)}</span></span>
    <BranchSelect {repo} {info} />
    {#if info && (info.ahead || info.behind)}
      <span class="sync mono" title={t("tree.gitAheadBehind", { ahead: info.ahead, behind: info.behind })}>
        ↑{info.ahead} ↓{info.behind}
      </span>
    {/if}
    {#if info?.upstream}<span class="upstream mono" title={t("git.upstream")}>{info.upstream}</span>{/if}
    {#if info?.conflicted}<span class="conflict">{t("tree.gitConflicted")}</span>{/if}
    <span class="spacer"></span>
    {#each ["fetch", "pull", "push"] as const as op (op)}
      <span class="split">
        <button class="op" disabled={!info || !!view.busy[op]} onclick={() => network(op)} title={t(`git.${op}Hint`)}>
          {#if view.busy[op]}<span class="spinner" aria-hidden="true"></span>{/if}
          {t(`git.${op}`)}
        </button>
        {#if op === "pull"}
          <button
            class="op caret"
            disabled={!info || !!view.busy.pull}
            aria-haspopup="menu"
            aria-label={t("git.pullOptions")}
            title={t("git.pullOptions")}
            onclick={pullMenu}
          >
            <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true"
              ><path d="M4 6l4 4 4-4" fill="none" stroke="currentColor" stroke-width="1.6" /></svg
            >
          </button>
        {/if}
      </span>
    {/each}
    <button class="ghost" onclick={refresh} title={t("git.refresh")} aria-label={t("git.refresh")}>
      <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true"
        ><path d="M13 8a5 5 0 11-1.5-3.5M13 2.5v3h-3" fill="none" stroke="currentColor" stroke-width="1.5" /></svg
      >
    </button>
  </header>

  {#if merge && operation}
    <div class="merge-banner" role="status">
      <div class="merge-head">
        <strong>{t(`git.merge.op.${operation}`, { theirs: merge.theirs_label, ours: merge.ours_label })}</strong>
        <span class="spacer"></span>
        <button
          class="primary"
          disabled={merge.conflicted.length > 0 || !!view.busy.continue}
          title={merge.conflicted.length > 0 ? t("git.merge.resolveFirst") : undefined}
          onclick={continueOperation}
        >
          {#if view.busy.continue}<span class="spinner" aria-hidden="true"></span>{/if}
          {operation === "merge" ? t("git.merge.commit") : t("git.merge.continue")}
        </button>
        <button class="danger-btn" disabled={!!view.busy.abort} onclick={abortOperation}>{t("git.merge.abort")}</button>
      </div>
      {#if merge.conflicted.length}
        <p class="muted small-text">{t("git.merge.conflicts", { count: merge.conflicted.length })}</p>
        <ul class="conflicts">
          {#each merge.conflicted as path (path)}
            <li>
              <span class="letter mono" style:color={stateColor("conflicted")}>{stateLetter("conflicted")}</span>
              <span class="mono cpath" title={path}>{relativePath(path, repo)}</span>
              <button class="small" onclick={() => git.openMerge(repo, path)}>{t("git.merge.resolve")}</button>
              <button class="small" disabled={!!view.busy.resolve} onclick={() => resolveWith(path, "ours")}
                >{t("git.merge.acceptYours")}</button
              >
              <button class="small" disabled={!!view.busy.resolve} onclick={() => resolveWith(path, "theirs")}
                >{t("git.merge.acceptTheirs")}</button
              >
            </li>
          {/each}
        </ul>
      {:else}
        <p class="muted small-text">{t("git.merge.noConflicts")}</p>
      {/if}
    </div>
  {/if}

  <nav class="tabs" aria-label={t("git.title")}>
    <button class:active={view.tab === "changes"} onclick={() => switchTab("changes")}>
      {t("git.changes")}<span class="count">{files.length}</span>
    </button>
    <button class:active={view.tab === "history"} onclick={() => switchTab("history")}>{t("git.history")}</button>
  </nav>

  {#if !info && statusError}
    <div class="empty">
      <p>{t("git.notRepo")}</p>
      <p class="error">{statusError}</p>
    </div>
  {:else if view.tab === "changes"}
    <div class="changes" bind:this={changesEl}>
      <div class="left" style:width="{view.listWidth}px">
        <div class="toolbar">
          <button class="ghost small" disabled={files.length === 0} onclick={() => setChecked(files.map((f) => f.path), true)}
            >{t("git.selectAll")}</button
          >
          <button class="ghost small" disabled={files.length === 0} onclick={() => setChecked(files.map((f) => f.path), false)}
            >{t("git.selectNone")}</button
          >
          <span class="spacer"></span>
          <span class="muted small-text">{t("git.selectedCount", { count: selected.length, total: files.length })}</span>
        </div>
        <div class="list" bind:this={listEl} role="tree" aria-label={t("git.changes")}>
          {#if statusError}
            <p class="error pad">{statusError}</p>
          {:else if !loaded}
            <p class="muted pad">{t("git.loading")}</p>
          {:else if files.length === 0}
            <p class="muted pad">{t("git.noChanges")}</p>
          {/if}
          {#each groups as g (g.dir)}
            {@const st = groupState(g.files)}
            <div class="group" role="group">
              <label class="dir" title={g.dir || repo}>
                <input
                  type="checkbox"
                  checked={st === "all"}
                  use:indeterminate={st === "some"}
                  onchange={(e) => setChecked(g.files.map((f) => f.path), e.currentTarget.checked)}
                />
                <svg viewBox="0 0 16 16" width="13" height="13" aria-hidden="true"
                  ><path
                    d="M1.5 4.5a1 1 0 011-1h3.5l1.5 1.5h6a1 1 0 011 1v6.5a1 1 0 01-1 1h-11a1 1 0 01-1-1z"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="1.3"
                  /></svg
                >
                <span class="dir-name mono">{g.dir || basename(repo)}</span>
                <span class="count">{g.files.length}</span>
              </label>
              {#each g.files as f (f.path)}
                {@const req = fileLabel(f)}
                <div
                  class="file"
                  class:active={view.activeFile === f.path}
                  role="treeitem"
                  aria-selected={view.activeFile === f.path}
                  tabindex="0"
                  data-path={f.path}
                  title={`${relativePath(f.path, repo)} — ${stateNames[f.state]()}`}
                  onclick={() => (view.activeFile = f.path)}
                  ondblclick={() => ws.index.has(f.path) && ws.select(f.path)}
                  oncontextmenu={(e) => openMenu(e, f)}
                  onkeydown={(e) => onrowkeydown(e, f)}
                >
                  <input
                    type="checkbox"
                    tabindex="-1"
                    aria-label={t("git.includeInCommit")}
                    checked={isChecked(f.path)}
                    onclick={(e) => e.stopPropagation()}
                    onchange={(e) => setChecked([f.path], e.currentTarget.checked)}
                  />
                  <span class="letter mono" style:color={stateColor(f.state)}>{stateLetter(f.state)}</span>
                  {#if req}
                    <span class="badge mono" style:color={methodColor(req.method)}>{methodLabel(req.method)}</span>
                    <span class="fname" style:color={stateColor(f.state)}>{req.name}</span>
                    <span class="sub mono">{basename(f.path)}</span>
                  {:else}
                    <span class="fname" class:deleted={f.state === "deleted"} style:color={stateColor(f.state)}
                      >{basename(f.path)}</span
                    >
                  {/if}
                  {#if isConflicted(f)}
                    <span class="spacer"></span>
                    <button
                      class="small resolve"
                      onclick={(e) => {
                        e.stopPropagation();
                        git.openMerge(repo, f.path);
                      }}>{t("git.merge.resolve")}</button
                    >
                  {/if}
                </div>
              {/each}
            </div>
          {/each}
        </div>
        <div class="commit">
          <textarea
            bind:this={textarea}
            bind:value={view.message}
            rows="3"
            placeholder={t("git.commitMessage")}
            aria-label={t("git.commitMessage")}
          ></textarea>
          <div class="commit-actions">
            <span class="muted small-text">{t("git.commitHint")}</span>
            <span class="spacer"></span>
            <button class="primary" disabled={!canCommit} onclick={() => commit(false)}>
              {#if view.busy.commit}<span class="spinner" aria-hidden="true"></span>{/if}
              {t("git.commit")}
            </button>
            <button disabled={!canCommit || !!view.busy.push} onclick={() => commit(true)}>{t("git.commitAndPush")}</button>
          </div>
        </div>
      </div>
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="gutter" onpointerdown={resize}></div>
      <div class="right">
        {#if activeFile}
          <GitDiff {repo} file={activeFile} />
        {:else}
          <p class="muted pad center">{t("git.selectFile")}</p>
        {/if}
      </div>
    </div>
  {:else}
    <GitHistory {repo} />
  {/if}

</section>

<style>
  .git-view {
    position: relative;
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    overflow: hidden;
  }
  header {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    border-bottom: 1px solid var(--border);
    flex-wrap: wrap;
  }
  .title {
    display: flex;
    align-items: center;
    gap: 6px;
    font-weight: 600;
    min-width: 0;
  }
  .repo-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sync,
  .upstream {
    color: var(--muted);
    font-size: 12px;
    white-space: nowrap;
  }
  .conflict {
    color: var(--err);
    font-size: 12px;
  }
  .spacer {
    flex: 1;
  }
  .op {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 10px;
  }
  .ghost {
    display: flex;
    align-items: center;
    padding: 4px 6px;
  }
  .small {
    padding: 2px 8px;
    font-size: 12px;
  }
  .small-text {
    font-size: 12px;
  }
  .muted {
    color: var(--muted);
  }
  .error {
    color: var(--err);
    white-space: pre-wrap;
    word-break: break-word;
  }
  .pad {
    padding: 0 12px;
  }
  .center {
    text-align: center;
    margin-top: 32px;
  }
  .empty {
    padding: 16px;
  }
  .changes {
    flex: 1;
    display: flex;
    min-height: 0;
  }
  .left {
    flex: none;
    display: flex;
    flex-direction: column;
    min-width: 220px;
    min-height: 0;
  }
  .right {
    flex: 1;
    min-width: 0;
    min-height: 0;
  }
  .gutter {
    flex: none;
    width: 5px;
    cursor: col-resize;
    border-left: 1px solid var(--border);
  }
  .gutter:hover {
    background: var(--hover);
  }
  .toolbar {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 4px 8px;
    border-bottom: 1px solid var(--border);
  }
  .list {
    flex: 1;
    overflow: auto;
    padding: 4px 0;
    min-height: 0;
  }
  .dir {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 3px 10px;
    color: var(--muted);
    font-size: 12px;
    cursor: pointer;
  }
  .dir-name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .count {
    color: var(--muted);
    font-size: 11px;
    margin-left: 4px;
  }
  .file {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 26px;
    padding: 0 10px 0 28px;
    margin: 0 4px;
    border-radius: 5px;
    cursor: pointer;
    user-select: none;
    font-size: 13px;
  }
  .file:hover {
    background: var(--hover);
  }
  .file.active {
    background: var(--selected);
  }
  .file:focus-visible {
    outline: 1px solid var(--accent);
  }
  input[type="checkbox"] {
    margin: 0;
    accent-color: var(--accent);
  }
  .letter {
    flex: none;
    width: 12px;
    font-size: 11px;
    font-weight: 700;
  }
  .badge {
    flex: none;
    width: 30px;
    font-size: 10px;
    font-weight: 700;
  }
  .fname {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .fname.deleted {
    text-decoration: line-through;
  }
  .sub {
    flex: 1;
    min-width: 0;
    color: var(--muted);
    font-size: 11px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .commit {
    border-top: 1px solid var(--border);
    padding: 8px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  textarea {
    resize: vertical;
    min-height: 56px;
    width: 100%;
  }
  .commit-actions {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .commit-actions button {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 10px;
  }
  .spinner {
    flex: none;
    width: 11px;
    height: 11px;
    border: 2px solid var(--border);
    border-top-color: var(--accent);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .split {
    display: inline-flex;
  }
  .split .op:not(:last-child) {
    border-top-right-radius: 0;
    border-bottom-right-radius: 0;
  }
  .split .caret {
    border-left: none;
    border-top-left-radius: 0;
    border-bottom-left-radius: 0;
    padding: 4px 4px;
  }
  .merge-banner {
    border-bottom: 1px solid var(--border);
    background: color-mix(in srgb, var(--warn) 12%, var(--surface));
    padding: 8px 12px;
  }
  .merge-head {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  .merge-head button {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 10px;
  }
  .danger-btn {
    color: var(--err);
    border-color: var(--err);
  }
  .merge-banner p {
    margin: 6px 0 4px;
  }
  .conflicts {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
    max-height: 160px;
    overflow: auto;
  }
  .conflicts li {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
  }
  .cpath {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .resolve {
    flex: none;
  }
</style>
