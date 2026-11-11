<script lang="ts">
  // History sub-tab of the Git view (D27): paged commit list (infinite scroll) and the selected commit's details.
  import type { GitCommit } from "$lib/api";
  import { i18n, t } from "$lib/i18n/index.svelte";
  import { ws } from "$lib/stores/workspace.svelte";
  import { git } from "$lib/stores/git.svelte";
  import { relativePath } from "$lib/git/status";
  import { formatDate, relativeTime } from "$lib/git/time";
  import { commitMenu } from "$lib/git/commitMenu";
  import CommitActionDialog from "./CommitActionDialog.svelte";
  import CommitDetails from "./CommitDetails.svelte";

  let { repo }: { repo: string } = $props();

  const view = $derived(git.view(repo));
  const uid = $props.id();
  const rowId = (i: number) => `${uid}-c-${i}`;

  /** Refs of a list commit: from the log entry if the backend provides them, else from loaded details. */
  function refsOf(c: GitCommit): string[] {
    const own = (c as GitCommit & { refs?: string[] }).refs;
    if (own?.length) return own;
    return view.details?.id === c.id ? view.details.refs : [];
  }

  const selectedIndex = $derived(view.log.findIndex((c) => c.id === view.selected));

  let listEl = $state<HTMLDivElement>();

  function onscroll() {
    if (!listEl || !view.logMore || view.logLoading) return;
    if (listEl.scrollTop + listEl.clientHeight >= listEl.scrollHeight - 200) void git.loadLog(repo, true);
  }

  function select(c: GitCommit) {
    void git.selectCommit(repo, c.id);
  }

  function onkeydown(e: KeyboardEvent) {
    const n = view.log.length;
    if (!n) return;
    let i = selectedIndex;
    if (e.key === "ArrowDown") i = Math.min(n - 1, i + 1);
    else if (e.key === "ArrowUp") i = Math.max(0, i - 1);
    else if (e.key === "Home") i = 0;
    else if (e.key === "End") i = n - 1;
    else if (e.key === "ContextMenu" || (e.shiftKey && e.key === "F10")) {
      e.preventDefault();
      const c = view.log[Math.max(0, i)];
      const el = listEl?.querySelector(`#${CSS.escape(rowId(Math.max(0, i)))}`)?.getBoundingClientRect();
      if (c) ws.menu = { x: (el?.left ?? 0) + 24, y: el?.bottom ?? 0, items: commitMenu(repo, c) };
      return;
    } else return;
    e.preventDefault();
    select(view.log[i]);
    listEl?.querySelector(`#${CSS.escape(rowId(i))}`)?.scrollIntoView({ block: "nearest" });
    if (i >= n - 5) void git.loadLog(repo, true);
  }

  function openMenu(e: MouseEvent, c: GitCommit) {
    e.preventDefault();
    select(c);
    ws.menu = { x: e.clientX, y: e.clientY, items: commitMenu(repo, c) };
  }

  let wrapEl = $state<HTMLDivElement>();
  function resize(e: PointerEvent) {
    e.preventDefault();
    const target = e.currentTarget as HTMLElement;
    target.setPointerCapture(e.pointerId);
    const move = (ev: PointerEvent) => {
      if (!wrapEl) return;
      const r = wrapEl.getBoundingClientRect();
      view.historyWidth = Math.round(Math.min(Math.max(ev.clientX - r.left, 240), Math.max(280, r.width - 320)));
    };
    const up = () => {
      target.removeEventListener("pointermove", move);
      target.removeEventListener("pointerup", up);
    };
    target.addEventListener("pointermove", move);
    target.addEventListener("pointerup", up);
  }
</script>

<div class="history" bind:this={wrapEl}>
  <div class="left" style:width="{view.historyWidth}px">
    {#if view.logPath}
      <div class="filter-chip">
        <span class="chip-text" title={view.logPath}>{t("git.historyOf", { name: relativePath(view.logPath, repo) })}</span>
        <button class="ghost small" onclick={() => git.showHistory(repo, null)}>{t("git.showAll")}</button>
      </div>
    {/if}
    {#if view.logError}<p class="error pad">{view.logError}</p>{/if}
    <div
      class="log"
      bind:this={listEl}
      role="listbox"
      tabindex="0"
      aria-label={t("git.history")}
      aria-activedescendant={selectedIndex >= 0 ? rowId(selectedIndex) : undefined}
      {onscroll}
      {onkeydown}
    >
      {#each view.log as c, i (c.id)}
        {@const refs = refsOf(c)}
        <!-- Keyboard selection is handled on the listbox (aria-activedescendant pattern). -->
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <div
          id={rowId(i)}
          class="row"
          class:active={view.selected === c.id}
          role="option"
          aria-selected={view.selected === c.id}
          tabindex="-1"
          title={c.id}
          onclick={() => select(c)}
          oncontextmenu={(e) => openMenu(e, c)}
        >
          <div class="line1">
            <span class="summary">{c.summary}</span>
            {#each refs as r (r)}<span class="ref mono">{r}</span>{/each}
          </div>
          <div class="line2">
            <span class="sha mono">{c.short_id}</span>
            <span class="author">{c.author}</span>
            <span class="spacer"></span>
            <span class="when" title={formatDate(c.time, i18n.locale)}>{relativeTime(c.time, i18n.locale)}</span>
          </div>
        </div>
      {/each}
      {#if view.logLoading}
        <p class="muted pad">{t("git.loading")}</p>
      {:else if !view.logError && view.log.length === 0}
        <p class="muted pad">{t("git.noCommits")}</p>
      {:else if view.logMore}
        <div class="more"><button class="small" onclick={() => git.loadLog(repo, true)}>{t("git.loadMore")}</button></div>
      {/if}
    </div>
  </div>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="gutter" onpointerdown={resize}></div>
  <div class="right">
    {#if !view.selected}
      <p class="muted pad center">{t("git.hist.selectCommit")}</p>
    {:else if view.detailsError}
      <p class="error pad">{view.detailsError}</p>
    {:else if view.details}
      <CommitDetails {repo} details={view.details} />
    {:else if view.detailsLoading}
      <p class="muted pad">{t("git.loading")}</p>
    {/if}
  </div>
</div>

{#if view.action}
  {#key view.action}
    <CommitActionDialog {repo} action={view.action} />
  {/key}
{/if}

<style>
  .history {
    flex: 1;
    display: flex;
    min-height: 0;
  }
  .left {
    flex: none;
    display: flex;
    flex-direction: column;
    min-width: 240px;
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
  .filter-chip {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 12px;
    border-bottom: 1px solid var(--border);
    font-size: 12px;
  }
  .chip-text {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .log {
    flex: 1;
    overflow: auto;
    min-height: 0;
  }
  .log:focus-visible {
    outline: 1px solid var(--accent);
    outline-offset: -1px;
  }
  .row {
    padding: 5px 12px;
    border-bottom: 1px solid var(--border);
    cursor: pointer;
    font-size: 13px;
  }
  .row:hover {
    background: var(--hover);
  }
  .row.active {
    background: var(--selected);
  }
  .line1,
  .line2 {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .line2 {
    font-size: 11px;
    color: var(--muted);
    margin-top: 2px;
  }
  .summary {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ref {
    flex: none;
    font-size: 10px;
    padding: 0 5px;
    border-radius: 4px;
    border: 1px solid var(--accent);
    color: var(--accent);
  }
  .sha {
    color: var(--accent);
  }
  .author {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .spacer {
    flex: 1;
  }
  .when {
    flex: none;
    white-space: nowrap;
  }
  .more {
    display: flex;
    justify-content: center;
    padding: 10px;
  }
  .small {
    padding: 2px 8px;
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
</style>
