<script lang="ts">
  // Details of the commit selected in the History (D27): message, authorship, parents, refs, actions,
  // changed files and the diff of the selected file against the first parent.
  import { api, type GitCommitDetails, type GitCommitFile, type GitFileDiff } from "$lib/api";
  import { i18n, t } from "$lib/i18n/index.svelte";
  import { ws, type MenuItem } from "$lib/stores/workspace.svelte";
  import { git, gitError } from "$lib/stores/git.svelte";
  import { basename, relativePath, stateColor, stateLetter } from "$lib/git/status";
  import { formatDate, relativeTime } from "$lib/git/time";
  import { checkoutCommit, cherryPick, copyHash, restoreFile } from "$lib/git/commitMenu";
  import { methodColor, methodLabel } from "./methods";
  import DiffBody from "./DiffBody.svelte";

  let { repo, details: d }: { repo: string; details: GitCommitDetails } = $props();

  const view = $derived(git.view(repo));
  const busy = $derived(!!view.busy.history);

  const body = $derived(d.message.split("\n").slice(1).join("\n").trim());
  const committerDiffers = $derived(d.committer !== d.author || d.committer_email !== d.author_email);

  const files = $derived.by(() => {
    const q = view.fileFilter.trim().toLowerCase();
    return q ? d.files.filter((f) => relativePath(f.path, repo).toLowerCase().includes(q) || (reqOf(f)?.name.toLowerCase().includes(q) ?? false)) : d.files;
  });
  const maxChange = $derived(Math.max(1, ...d.files.map((f) => (f.additions ?? 0) + (f.deletions ?? 0))));
  const activeFile = $derived(d.files.find((f) => f.path === view.commitFile) ?? null);

  function reqOf(f: GitCommitFile) {
    const loc = ws.index.get(f.path);
    return loc ? { name: loc.file.request.name, method: loc.file.request.method } : null;
  }

  // ----- diff of the selected file -----
  let diff = $state<GitFileDiff | null>(null);
  let diffError = $state<string | null>(null);
  let diffLoading = $state(false);

  $effect(() => {
    const file = activeFile;
    const commit = d.id;
    diff = null;
    diffError = null;
    if (!file || file.binary) return;
    let cancelled = false;
    diffLoading = true;
    api
      .gitCommitFileDiff(repo, commit, file.path)
      .then((res) => {
        if (!cancelled) diff = res;
      })
      .catch((e) => {
        if (!cancelled) diffError = gitError(e);
      })
      .finally(() => {
        if (!cancelled) diffLoading = false;
      });
    return () => {
      cancelled = true;
    };
  });

  const parentTitle = $derived(d.parents[0] ? d.parents[0].slice(0, 7) : t("git.hist.noParent"));

  function fileMenu(e: MouseEvent, f: GitCommitFile) {
    e.preventDefault();
    view.commitFile = f.path;
    const items: MenuItem[] = [];
    if (ws.index.has(f.path)) items.push({ label: t("git.openRequest"), action: () => ws.select(f.path) });
    if (f.state !== "deleted") {
      items.push({
        label: t("git.hist.restoreFile"),
        action: () => void restoreFile(repo, f.path, reqOf(f)?.name ?? basename(f.path), d),
      });
    }
    items.push({ label: t("git.showHistory"), action: () => git.showHistory(repo, f.path) });
    ws.menu = { x: e.clientX, y: e.clientY, items };
  }

  let fileListEl = $state<HTMLDivElement>();
  function onfileskeydown(e: KeyboardEvent) {
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    if (!files.length) return;
    const i = files.findIndex((f) => f.path === view.commitFile);
    const next = files[Math.max(0, Math.min(files.length - 1, i + (e.key === "ArrowDown" ? 1 : -1)))];
    view.commitFile = next.path;
    fileListEl?.querySelector(`[data-path="${CSS.escape(next.path)}"]`)?.scrollIntoView({ block: "nearest" });
  }
</script>

<div class="details">
  <div class="meta">
    <div class="title-row">
      <h3 class="summary">{d.summary}</h3>
    </div>
    {#if d.refs.length}
      <div class="refs">
        {#each d.refs as r (r)}<span class="ref mono">{r}</span>{/each}
      </div>
    {/if}
    {#if body}<pre class="body">{body}</pre>{/if}
    <dl>
      <dt>{t("git.hist.author")}</dt>
      <dd>{d.author} &lt;{d.author_email}&gt; · <span title={formatDate(d.time, i18n.locale)}>{formatDate(d.time, i18n.locale)} ({relativeTime(d.time, i18n.locale)})</span></dd>
      {#if committerDiffers}
        <dt>{t("git.hist.committer")}</dt>
        <dd>{d.committer} &lt;{d.committer_email}&gt; · {formatDate(d.committer_time, i18n.locale)}</dd>
      {/if}
      <dt>{t("git.hist.hash")}</dt>
      <dd class="hash">
        <span class="mono sel">{d.id}</span>
        <button class="ghost small" title={t("git.hist.copyHash")} aria-label={t("git.hist.copyHash")} onclick={() => copyHash(d.id)}>
          <svg viewBox="0 0 16 16" width="13" height="13" aria-hidden="true"
            ><rect x="5" y="5" width="8.5" height="8.5" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.3" /><path
              d="M3 10.5V3.5A1 1 0 014 2.5h6.5"
              fill="none"
              stroke="currentColor"
              stroke-width="1.3"
            /></svg
          >
        </button>
      </dd>
      {#if d.parents.length}
        <dt>{d.parents.length > 1 ? t("git.hist.parents") : t("git.hist.parent")}</dt>
        <dd>
          {#each d.parents as p (p)}
            <button class="link mono" title={p} onclick={() => git.selectCommit(repo, p)}>{p.slice(0, 7)}</button>
          {/each}
        </dd>
      {/if}
    </dl>
    <div class="actions">
      <button class="small" disabled={busy} onclick={() => (view.action = { kind: "revert", commit: d })}>{t("git.hist.revert")}</button>
      <button class="small" disabled={busy} onclick={() => cherryPick(repo, d)}>{t("git.hist.cherryPick")}</button>
      <button class="small" disabled={busy} onclick={() => (view.action = { kind: "branch", commit: d })}>{t("git.hist.newBranch")}</button>
      <button class="small" disabled={busy} onclick={() => checkoutCommit(repo, d)}>{t("git.hist.checkout")}</button>
      <button class="small danger" disabled={busy} onclick={() => (view.action = { kind: "reset", commit: d })}>{t("git.hist.reset")}</button>
      {#if busy}<span class="spinner" aria-hidden="true"></span>{/if}
    </div>
  </div>

  <div class="files">
    <div class="files-head">
      <span class="label">{t("git.hist.changedFiles", { count: d.files.length })}</span>
      <input class="filter" bind:value={view.fileFilter} placeholder={t("git.hist.filterFiles")} aria-label={t("git.hist.filterFiles")} />
    </div>
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <div class="file-list" bind:this={fileListEl} role="listbox" tabindex="0" aria-label={t("git.hist.changedFilesLabel")} onkeydown={onfileskeydown}>
      {#each files as f (f.path)}
        {@const req = reqOf(f)}
        {@const total = (f.additions ?? 0) + (f.deletions ?? 0)}
        <!-- Keyboard selection is handled on the listbox. -->
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <div
          class="file"
          class:active={view.commitFile === f.path}
          role="option"
          aria-selected={view.commitFile === f.path}
          tabindex="-1"
          data-path={f.path}
          title={relativePath(f.path, repo)}
          onclick={() => (view.commitFile = f.path)}
          oncontextmenu={(e) => fileMenu(e, f)}
        >
          <span class="letter mono" style:color={stateColor(f.state)}>{stateLetter(f.state)}</span>
          {#if req}
            <span class="badge mono" style:color={methodColor(req.method)}>{methodLabel(req.method)}</span>
            <span class="fname">{req.name}</span>
            <span class="sub mono">{relativePath(f.path, repo)}</span>
          {:else}
            <span class="fname mono">
              {#if f.old_path}{relativePath(f.old_path, repo)} → {/if}{relativePath(f.path, repo)}
            </span>
          {/if}
          <span class="spacer"></span>
          {#if f.binary}
            <span class="bin">{t("git.hist.binary")}</span>
          {:else}
            <span class="counts mono"><span class="plus">+{f.additions ?? 0}</span> <span class="minus">−{f.deletions ?? 0}</span></span>
            <span class="minibar" aria-hidden="true" style:width="{Math.max(4, Math.round((total / maxChange) * 40))}px">
              <span class="add" style:flex={f.additions ?? 0}></span><span class="del" style:flex={f.deletions ?? 0}></span>
            </span>
          {/if}
        </div>
      {/each}
      {#if files.length === 0}<p class="muted pad">{t("git.hist.noFiles")}</p>{/if}
    </div>
  </div>

  <div class="diff">
    {#if !activeFile}
      <p class="muted pad">{t("git.selectFile")}</p>
    {:else if activeFile.binary}
      <p class="muted pad">{t("git.hist.binaryNotice")}</p>
    {:else if diffError}
      <p class="error pad" role="alert">{diffError}</p>
    {:else if diff}
      <DiffBody old={diff.old} new={diff.new} path={activeFile.path} oldTitle={parentTitle} newTitle={d.short_id} />
    {:else if diffLoading}
      <p class="muted pad">{t("git.loading")}</p>
    {/if}
  </div>
</div>

<style>
  .details {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    min-width: 0;
  }
  .meta {
    flex: none;
    max-height: 40%;
    overflow: auto;
    padding: 10px 12px;
    border-bottom: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: 13px;
  }
  .summary {
    margin: 0;
    font-size: 14px;
    user-select: text;
  }
  .body {
    margin: 0;
    font-family: inherit;
    white-space: pre-wrap;
    word-break: break-word;
    user-select: text;
  }
  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 2px 10px;
    margin: 0;
    font-size: 12px;
  }
  dt {
    color: var(--muted);
  }
  dd {
    margin: 0;
    min-width: 0;
    word-break: break-word;
    user-select: text;
  }
  .hash {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .sel {
    user-select: all;
    word-break: break-all;
  }
  .refs {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .ref {
    font-size: 11px;
    padding: 1px 6px;
    border-radius: 4px;
    border: 1px solid var(--accent);
    color: var(--accent);
  }
  .link {
    border: none;
    background: none;
    padding: 0 6px 0 0;
    color: var(--accent);
    font-size: 12px;
  }
  .link:hover {
    text-decoration: underline;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
  }
  .small {
    padding: 3px 8px;
    font-size: 12px;
  }
  .ghost.small {
    padding: 2px 4px;
    display: flex;
  }
  .danger {
    color: var(--err);
  }
  .files {
    flex: none;
    max-height: 30%;
    display: flex;
    flex-direction: column;
    border-bottom: 1px solid var(--border);
    min-height: 0;
  }
  .files-head {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 10px;
  }
  .label {
    font-size: 12px;
    color: var(--muted);
    white-space: nowrap;
  }
  .filter {
    flex: 1;
    min-width: 0;
    padding: 2px 8px;
    font-size: 12px;
  }
  .file-list {
    overflow: auto;
    min-height: 0;
    padding-bottom: 4px;
  }
  .file-list:focus-visible {
    outline: 1px solid var(--accent);
    outline-offset: -1px;
  }
  .file {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 24px;
    padding: 0 10px;
    margin: 0 4px;
    border-radius: 5px;
    cursor: pointer;
    font-size: 12px;
  }
  .file:hover {
    background: var(--hover);
  }
  .file.active {
    background: var(--selected);
  }
  .letter {
    flex: none;
    width: 12px;
    font-weight: 700;
    font-size: 11px;
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
  .sub {
    min-width: 0;
    flex: 0 1 auto;
    color: var(--muted);
    font-size: 11px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .spacer {
    flex: 1;
  }
  .counts {
    flex: none;
    font-size: 11px;
  }
  .plus {
    color: var(--ok);
  }
  .minus {
    color: var(--err);
  }
  .minibar {
    flex: none;
    display: flex;
    height: 6px;
    border-radius: 2px;
    overflow: hidden;
    background: var(--border);
  }
  .minibar .add {
    background: var(--ok);
  }
  .minibar .del {
    background: var(--err);
  }
  .bin {
    color: var(--muted);
    font-size: 11px;
  }
  .diff {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .muted {
    color: var(--muted);
  }
  .pad {
    padding: 0 12px;
  }
  .error {
    color: var(--err);
    white-space: pre-wrap;
  }
  .spinner {
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
</style>
