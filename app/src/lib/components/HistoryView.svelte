<script lang="ts">
  import { onMount } from "svelte";
  import type { HistoryEntrySummary } from "$lib/api";
  import { t } from "$lib/i18n/index.svelte";
  import { history } from "$lib/stores/history.svelte";
  import { ws, type MenuItem } from "$lib/stores/workspace.svelte";
  import { clearHistory, copyCurl, copyUrl, dayKey, dayLabel, deleteHistory, timeAgo, timeOfDay, dateTime } from "./historyUtil";
  import { methodColor, methodLabel } from "./methods";
  import SearchBox from "./SearchBox.svelte";
  import Highlight from "./Highlight.svelte";
  import { parseQuery } from "$lib/search/match";
  import { searchFocus, shortcutLabel } from "$lib/search/focus.svelte";

  /** Ticks so "5 min ago" labels stay current. */
  let now = $state(Date.now());
  let search = $state(history.query);
  let searchTimer: ReturnType<typeof setTimeout> | undefined;
  /** Anchor for Shift-click range selection. */
  let anchor: number | null = null;

  onMount(() => {
    // The view just became visible: show fresh data.
    void history.refresh();
    void history.loadSettings();
    const timer = setInterval(() => (now = Date.now()), 30_000);
    return () => {
      clearInterval(timer);
      clearTimeout(searchTimer);
    };
  });

  let searchInput = $state<HTMLInputElement>();
  // Cmd/Ctrl+P focuses the visible sidebar search (ignore the value we were mounted with).
  let seenFocus = searchFocus.sidebar;
  $effect(() => {
    const n = searchFocus.sidebar;
    if (n === seenFocus) return;
    seenFocus = n;
    searchInput?.focus();
    searchInput?.select();
  });

  /** Terms of the applied query, for highlighting. */
  const terms = $derived(parseQuery(history.query).terms);
  const countLabel = $derived(
    history.query.trim()
      ? history.items.length === 1
        ? t("search.oneResult")
        : t("search.results", { n: `${history.items.length}${history.hasMore ? "+" : ""}` })
      : null,
  );

  function onsearchkey(e: KeyboardEvent) {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      (e.currentTarget as HTMLElement).closest(".history")?.querySelector<HTMLElement>(".entry")?.focus();
    } else if (e.key === "Enter") {
      e.preventDefault();
      clearTimeout(searchTimer);
      if (history.query !== search) {
        history.query = search;
        void history.refresh();
      } else if (history.items[0]) ws.openHistory(history.items[0].id);
    }
  }

  function onsearch() {
    clearTimeout(searchTimer);
    searchTimer = setTimeout(() => {
      history.query = search;
      void history.refresh();
    }, 250);
  }

  const groups = $derived.by(() => {
    const out: { key: number; label: string; items: HistoryEntrySummary[] }[] = [];
    for (const e of history.items) {
      const key = dayKey(e.timestamp);
      let g = out[out.length - 1];
      if (!g || g.key !== key) {
        g = { key, label: dayLabel(e.timestamp, now), items: [] };
        out.push(g);
      }
      g.items.push(e);
    }
    return out;
  });

  const selectedCount = $derived(history.selectedIds.length);
  const disabled = $derived(history.settings?.enabled === false);

  function onscroll(e: Event) {
    const el = e.currentTarget as HTMLElement;
    if (el.scrollTop + el.clientHeight >= el.scrollHeight - 120) void history.loadMore();
  }

  function selectRange(to: number) {
    const ids = history.items.map((e) => e.id);
    const a = anchor === null ? -1 : ids.indexOf(anchor);
    const b = ids.indexOf(to);
    if (a < 0 || b < 0) {
      history.toggleSelected(to, true);
    } else {
      for (let i = Math.min(a, b); i <= Math.max(a, b); i++) history.toggleSelected(ids[i], true);
    }
  }

  function onrowclick(e: MouseEvent, entry: HistoryEntrySummary) {
    if (e.shiftKey) {
      e.preventDefault();
      selectRange(entry.id);
      return;
    }
    if (e.metaKey || e.ctrlKey) {
      history.toggleSelected(entry.id);
      anchor = entry.id;
      return;
    }
    ws.openHistory(entry.id);
  }

  function onrowkeydown(e: KeyboardEvent, entry: HistoryEntrySummary) {
    if (e.key === "Enter") {
      e.preventDefault();
      ws.openHistory(entry.id);
    } else if (e.key === " ") {
      e.preventDefault();
      history.toggleSelected(entry.id);
      anchor = entry.id;
    } else if (e.key === "Delete" || e.key === "Backspace") {
      e.preventDefault();
      void deleteHistory(selectedCount && history.selected[entry.id] ? history.selectedIds : [entry.id]);
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const rows = Array.from(
        (e.currentTarget as HTMLElement).closest(".list")?.querySelectorAll<HTMLElement>(".entry") ?? [],
      );
      const i = rows.indexOf(e.currentTarget as HTMLElement);
      rows[i + (e.key === "ArrowDown" ? 1 : -1)]?.focus();
    }
  }

  function items(entry: HistoryEntrySummary): MenuItem[] {
    const list: MenuItem[] = [
      { label: t("history.openInTab"), action: () => ws.openHistory(entry.id) },
      { label: t("history.copyUrl"), action: () => copyUrl(entry) },
      { label: t("history.copyCurl"), action: () => copyCurl(entry) },
    ];
    if (selectedCount > 1 && history.selected[entry.id]) {
      list.push({
        label: t("history.deleteSelected", { count: selectedCount }),
        danger: true,
        action: () => deleteHistory(history.selectedIds),
      });
    } else {
      list.push({ label: t("history.delete"), danger: true, action: () => deleteHistory([entry.id]) });
    }
    return list;
  }

  function openMenu(e: MouseEvent, entry: HistoryEntrySummary) {
    e.preventDefault();
    e.stopPropagation();
    ws.menu = { x: e.clientX, y: e.clientY, items: items(entry) };
  }

  function statusTitle(entry: HistoryEntrySummary): string {
    return entry.error ?? (entry.status !== null ? String(entry.status) : "");
  }
</script>

<div class="history">
  <div class="toolbar">
    <div class="search">
      <SearchBox
        bind:value={search}
        bind:input={searchInput}
        placeholder={t("history.search")}
        count={countLabel}
        shortcut={shortcutLabel("P")}
        oninput={onsearch}
        onkeydown={onsearchkey}
      />
    </div>
    <button
      class="ghost icon"
      title={t("history.refresh")}
      aria-label={t("history.refresh")}
      onclick={() => history.refresh()}
    >
      <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.5">
        <path d="M13 8a5 5 0 11-1.5-3.6M13 2.5v2.5h-2.5" />
      </svg>
    </button>
  </div>

  {#if selectedCount > 0}
    <div class="selbar">
      <button class="small danger-btn" onclick={() => deleteHistory(history.selectedIds)}>
        {t("history.deleteSelected", { count: selectedCount })}
      </button>
      <button class="ghost small" onclick={() => history.clearSelection()}>{t("history.clearSelection")}</button>
    </div>
  {/if}

  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    class="list"
    role="listbox"
    aria-multiselectable="true"
    aria-label={t("history.title")}
    tabindex="-1"
    {onscroll}
  >
    {#if disabled}
      <p class="empty">{t("history.disabled")}</p>
    {/if}
    {#if history.error}
      <p class="error">{t("history.loadError")}: {history.error}</p>
    {/if}
    {#if !disabled && !history.error && !history.loading && history.items.length === 0}
      <p class="empty">{history.query.trim() ? t("history.noMatches") : t("history.empty")}</p>
    {/if}

    {#each groups as g (g.key)}
      <div class="day" role="presentation">{g.label}</div>
      {#each g.items as entry (entry.id)}
        {@const checked = !!history.selected[entry.id]}
        <div
          class="entry"
          class:checked
          class:selecting={selectedCount > 0}
          role="option"
          aria-selected={checked}
          tabindex="0"
          title={`${entry.method} ${entry.url}\n${dateTime(entry.timestamp)}`}
          onclick={(e) => onrowclick(e, entry)}
          onkeydown={(e) => onrowkeydown(e, entry)}
          oncontextmenu={(e) => openMenu(e, entry)}
        >
          <input
            class="check"
            type="checkbox"
            tabindex="-1"
            {checked}
            aria-label={t("history.select")}
            onclick={(e) => {
              e.stopPropagation();
              if (e.shiftKey) selectRange(entry.id);
              else {
                history.toggleSelected(entry.id, e.currentTarget.checked);
                anchor = entry.id;
              }
            }}
          />
          <span class="badge mono" style:color={methodColor(entry.method)}>{methodLabel(entry.method)}</span>
          <span class="main">
            <span class="url"><Highlight text={entry.url} terms={terms} /></span>
            <span class="meta">
              {#if entry.error}
                <span class="status err" title={statusTitle(entry)}>
                  <svg viewBox="0 0 16 16" width="11" height="11" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.6">
                    <circle cx="8" cy="8" r="6" /><path d="M8 4.5v4.2M8 11v.5" />
                  </svg>
                  {t("history.failed")}
                </span>
              {:else if entry.status !== null}
                <span class="status" class:ok={entry.status < 400} class:err={entry.status >= 400}>{entry.status}</span>
              {/if}
              <span>{t("response.time", { ms: Math.round(entry.elapsed_ms) })}</span>
              <span class="when" title={timeOfDay(entry.timestamp)}>{timeAgo(entry.timestamp, now)}</span>
            </span>
          </span>
        </div>
      {/each}
    {/each}

    {#if history.loading}
      <p class="empty">{t("common.loading")}</p>
    {/if}
  </div>

  <footer>
    <button class="ghost small danger-text" disabled={history.items.length === 0} onclick={() => clearHistory()}>
      {t("history.clearAll")}
    </button>
  </footer>
</div>

<style>
  .history {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
  }
  .toolbar {
    display: flex;
    gap: 4px;
    padding: 8px 8px 6px;
  }
  .search {
    flex: 1;
    min-width: 0;
  }
  .icon {
    display: flex;
    padding: 5px;
  }
  .selbar {
    display: flex;
    gap: 6px;
    align-items: center;
    padding: 0 8px 6px;
  }
  .small {
    padding: 3px 8px;
    font-size: 12px;
  }
  .danger-btn {
    color: var(--err);
    border-color: var(--err);
  }
  .danger-text {
    color: var(--err);
  }
  .list {
    flex: 1;
    overflow: auto;
    padding: 0 0 6px;
    font-size: 13px;
  }
  .list:focus {
    outline: none;
  }
  .day {
    position: sticky;
    top: 0;
    z-index: 1;
    background: var(--surface);
    color: var(--muted);
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    padding: 8px 12px 4px;
  }
  .entry {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0 4px;
    padding: 4px 6px 4px 6px;
    border-radius: 5px;
    cursor: pointer;
    user-select: none;
  }
  .entry:hover {
    background: var(--hover);
  }
  .entry.checked {
    background: var(--selected);
  }
  .entry:focus-visible {
    outline: 1px solid var(--accent);
  }
  .check {
    flex: none;
    margin: 0;
    padding: 0;
    width: 13px;
    height: 13px;
    accent-color: var(--accent);
    display: none;
  }
  .entry:hover .check,
  .entry.selecting .check,
  .entry:focus-visible .check {
    display: block;
  }
  .badge {
    flex: none;
    width: 34px;
    font-size: 10px;
    font-weight: 700;
  }
  .main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    line-height: 1.3;
  }
  .url {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .meta {
    display: flex;
    gap: 8px;
    font-size: 11px;
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
  }
  .status {
    font-weight: 600;
    display: inline-flex;
    align-items: center;
    gap: 3px;
  }
  .status.ok {
    color: var(--ok);
  }
  .status.err {
    color: var(--err);
  }
  .when {
    margin-left: auto;
  }
  .empty,
  .error {
    font-size: 12px;
    margin: 8px 12px;
    color: var(--muted);
  }
  .error {
    color: var(--err);
    word-break: break-word;
  }
  footer {
    display: flex;
    justify-content: flex-end;
    padding: 4px 8px;
    border-top: 1px solid var(--border);
  }
</style>
