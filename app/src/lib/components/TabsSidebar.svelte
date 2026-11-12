<script lang="ts">
  import { t } from "$lib/i18n/index.svelte";
  import { ws, historyKey, tabKey, tabPath, type MenuItem, type OpenTab } from "$lib/stores/workspace.svelte";
  import GitIcon from "./GitIcon.svelte";
  import EnvIcon from "./EnvIcon.svelte";
  import { envSession } from "$lib/stores/environments.svelte";
  import { dateTime } from "./historyUtil";
  import { methodColor, methodLabel } from "./methods";
  import SearchBox from "./SearchBox.svelte";
  import Highlight from "./Highlight.svelte";
  import { allTermsIn, normalize, parseQuery } from "$lib/search/match";
  import { searchFocus, shortcutLabel } from "$lib/search/focus.svelte";

  interface Props {
    collapsed: boolean;
    ontoggle: () => void;
  }

  let { collapsed, ontoggle }: Props = $props();

  function basename(p: string): string {
    return p.split(/[\\/]/).filter(Boolean).pop() ?? p;
  }

  interface TabView {
    key: string;
    tab: OpenTab;
    /** Full title shown in the row / tooltip. */
    title: string;
    hint: string;
    method: string | null;
    dirty: boolean;
  }

  function view(tab: OpenTab): TabView {
    const key = tabKey(tab);
    const node = ws.nodes.get(tabPath(tab));
    if (tab.kind === "request") {
      const draft = ws.drafts[tab.path];
      const file = ws.index.get(tab.path)?.file;
      return {
        key,
        tab,
        title: draft?.name || file?.request.name || node?.name || basename(tab.path),
        hint: node?.hint ?? "",
        method: draft?.method ?? file?.request.method ?? "GET",
        dirty: ws.isDirty(tab.path),
      };
    }
    if (tab.kind === "environments") {
      const name = tab.project ? (ws.nodes.get(tab.project)?.name ?? basename(tab.project)) : "";
      return {
        key,
        tab,
        title: tab.project === null ? t("env.globalTitle") : t("env.projectTitle", { name }),
        hint: "",
        method: null,
        dirty: envSession(tab.project).dirty,
      };
    }
    if (tab.kind === "history") {
      const draft = ws.drafts[historyKey(tab.id)];
      const entry = ws.historyTabs[tab.id]?.entry ?? null;
      return {
        key,
        tab,
        title: draft?.name || entry?.name || entry?.url || t("history.tabLoading"),
        hint: entry ? t("history.tabHint", { date: dateTime(entry.timestamp) }) : "",
        method: draft?.method ?? entry?.method ?? null,
        dirty: ws.isHistoryDirty(tab.id),
      };
    }
    if (tab.kind === "merge") {
      // Request files show their request name; other files their file name.
      const name = node?.kind === "request" ? node.name : basename(tab.path);
      return { key, tab, title: t("tabs.merge", { name }), hint: node?.hint ?? basename(tab.repo), method: null, dirty: false };
    }
    if (tab.kind === "import") {
      return { key, tab, title: t("tabs.import"), hint: "", method: null, dirty: false };
    }
    if (tab.kind === "container") {
      const name = node?.name ?? basename(tab.path);
      return { key, tab, title: t("tabs.container", { name }), hint: node?.hint ?? "", method: null, dirty: ws.isTabDirty(tab) };
    }
    if (tab.kind === "git") {
      const name = basename(tab.repo);
      const info = ws.repoByRoot.get(tab.repo);
      const branch = info ? (info.branch ?? info.head) : null;
      return {
        key,
        tab,
        title: branch ? t("tabs.git", { name, branch }) : t("tabs.gitPlain", { name }),
        hint: node?.hint ?? "",
        method: null,
        dirty: false,
      };
    }
    const name = node?.name ?? basename(tab.target);
    return {
      key,
      tab,
      title: tab.kind === "runner" ? t("tabs.run", { name }) : t("tabs.load", { name }),
      hint: node?.hint ?? "",
      method: null,
      dirty: false,
    };
  }

  const views = $derived(ws.tabs.map(view));

  // ----- search -----
  let query = $state("");
  let searchInput = $state<HTMLInputElement>();
  const terms = $derived(parseQuery(query).terms);
  /** Keys of the tabs matching the query (null = not filtering). */
  const matches = $derived.by(() => {
    if (terms.length === 0) return null;
    const keys = new Set<string>();
    for (const v of views) {
      const url = ws.drafts[tabPath(v.tab)]?.url ?? "";
      const hay = normalize([v.title, v.hint, v.method ?? "", url, v.tab.kind].join("\n"));
      if (allTermsIn(terms, [hay])) keys.add(v.key);
    }
    return keys;
  });
  const shownViews = $derived(matches ? views.filter((v) => matches.has(v.key)) : views);

  // Cmd/Ctrl+E (or the rail's search button) focuses the box once it is rendered.
  let seenFocus = searchFocus.tabs;
  $effect(() => {
    const n = searchFocus.tabs;
    if (n === seenFocus || !searchInput) return;
    seenFocus = n;
    searchInput.focus();
    searchInput.select();
  });

  function onsearchkey(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      const first = shownViews[0];
      if (first) ws.activateTab(first.key);
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      listEl?.querySelector<HTMLElement>("[data-tab-index]")?.focus();
    }
  }

  function searchFromRail() {
    ontoggle();
    searchFocus.tabs++;
  }

  // ----- context menu -----
  function items(v: TabView): MenuItem[] {
    const list: MenuItem[] = [
      { label: t("tabs.close"), action: () => ws.closeTab(v.key) },
      { label: t("tabs.closeOthers"), action: () => ws.closeOtherTabs(v.key) },
      { label: t("tabs.closeAll"), action: () => ws.closeAllTabs() },
      { label: t("tabs.closeSaved"), action: () => ws.closeSavedTabs() },
    ];
    return [...list, ...revealItem(v)];
  }

  function revealItem(v: TabView): MenuItem[] {
    if (v.tab.kind !== "history") return [{ label: t("tabs.reveal"), action: () => ws.reveal(tabPath(v.tab)) }];
    const p = ws.historyTabs[v.tab.id]?.entry?.request_path;
    return p && ws.nodes.has(p) ? [{ label: t("tabs.reveal"), action: () => ws.reveal(p) }] : [];
  }

  function openMenu(e: MouseEvent, v: TabView) {
    e.preventDefault();
    e.stopPropagation();
    ws.menu = { x: e.clientX, y: e.clientY, items: items(v) };
  }

  // ----- drag to reorder (pointer based, so it doesn't depend on HTML5 DnD in the webview) -----
  let listEl = $state<HTMLElement>();
  let dragFrom = $state<number | null>(null);
  /** Insertion index (0..n) while dragging. */
  let dragOver = $state<number | null>(null);
  let suppressClick = false;

  function insertionIndex(y: number): number {
    const rows = listEl ? Array.from(listEl.querySelectorAll<HTMLElement>("[data-tab-index]")) : [];
    for (const row of rows) {
      const r = row.getBoundingClientRect();
      if (y < r.top + r.height / 2) return Number(row.dataset.tabIndex);
    }
    return rows.length;
  }

  function startDrag(e: PointerEvent, index: number) {
    if (e.button !== 0 || ws.tabs.length < 2 || matches) return;
    if ((e.target as Element).closest(".close")) return;
    const startY = e.clientY;
    let moved = false;
    const move = (ev: PointerEvent) => {
      if (!moved && Math.abs(ev.clientY - startY) < 5) return;
      moved = true;
      dragFrom = index;
      dragOver = insertionIndex(ev.clientY);
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", up);
      if (moved && dragOver !== null) {
        ws.moveTab(index, dragOver > index ? dragOver - 1 : dragOver);
        suppressClick = true;
        setTimeout(() => (suppressClick = false));
      }
      dragFrom = null;
      dragOver = null;
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", up);
  }

  function activate(v: TabView) {
    if (suppressClick) return;
    ws.activateTab(v.key);
  }

  function onrowkeydown(e: KeyboardEvent, v: TabView, index: number) {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      ws.activateTab(v.key);
    } else if (e.key === "Delete" || e.key === "Backspace") {
      e.preventDefault();
      ws.closeTab(v.key);
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      // DOM order (the list may be filtered by the search box).
      const rows = Array.from(listEl?.querySelectorAll<HTMLElement>("[data-tab-index]") ?? []);
      const at = rows.indexOf(e.currentTarget as HTMLElement);
      const next = rows[(at < 0 ? index : at) + (e.key === "ArrowDown" ? 1 : -1)];
      if (next) next.focus();
      else if (e.key === "ArrowUp") searchInput?.focus();
    }
  }
</script>

<aside class="tabs-side" class:collapsed aria-label={t("tabs.title")}>
  <header>
    {#if !collapsed}
      <span class="title">{t("tabs.title")}</span>
      {#if ws.tabs.length}<span class="count">{ws.tabs.length}</span>{/if}
    {/if}
    <button
      class="ghost toggle"
      title={collapsed ? t("tabs.expand") : t("tabs.collapse")}
      aria-label={collapsed ? t("tabs.expand") : t("tabs.collapse")}
      aria-expanded={!collapsed}
      onclick={ontoggle}
    >
      <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
        {#if collapsed}
          <path d="M10 4L6 8l4 4" fill="none" stroke="currentColor" stroke-width="1.6" />
        {:else}
          <path d="M6 4l4 4-4 4" fill="none" stroke="currentColor" stroke-width="1.6" />
        {/if}
      </svg>
    </button>
  </header>

  {#if collapsed}
    <button class="ghost rail-search" title={t("search.tabs")} aria-label={t("search.tabs")} onclick={searchFromRail}>
      <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.5">
        <circle cx="7" cy="7" r="4.5" /><path d="M10.5 10.5L14 14" />
      </svg>
    </button>
  {:else if ws.tabs.length > 0}
    <div class="tabs-search">
      <SearchBox
        bind:value={query}
        bind:input={searchInput}
        placeholder={t("search.tabs")}
        count={matches ? (matches.size === 1 ? t("search.oneResult") : t("search.results", { n: matches.size })) : null}
        shortcut={shortcutLabel("E")}
        onkeydown={onsearchkey}
      />
    </div>
  {/if}

  <div class="list" role="tablist" aria-orientation="vertical" aria-label={t("tabs.title")} bind:this={listEl}>
    {#if views.length === 0 && !collapsed}
      <p class="empty">{t("tabs.empty")}</p>
    {:else if matches && matches.size === 0 && !collapsed}
      <p class="empty">{t("search.noTabMatches")}</p>
    {/if}
    {#each views as v, i (v.key)}
      {#if !matches || matches.has(v.key)}
      {@const active = ws.activeKey === v.key}
      <div
        class="tab"
        class:active
        class:dragging={dragFrom === i}
        class:drop-before={dragOver === i && dragFrom !== null}
        class:drop-after={dragOver === views.length && i === views.length - 1 && dragFrom !== null}
        role="tab"
        aria-selected={active}
        tabindex="0"
        data-tab-index={i}
        title={v.hint ? `${v.title} — ${v.hint}` : v.title}
        onclick={() => activate(v)}
        onpointerdown={(e) => startDrag(e, i)}
        onmousedown={(e) => e.button === 1 && e.preventDefault()}
        onauxclick={(e) => {
          if (e.button === 1) {
            e.preventDefault();
            ws.closeTab(v.key);
          }
        }}
        oncontextmenu={(e) => openMenu(e, v)}
        onkeydown={(e) => onrowkeydown(e, v, i)}
      >
        {#if v.method}
          <span class="badge mono" style:color={methodColor(v.method)}>{methodLabel(v.method)}</span>
        {:else}
          <span class="badge kind" aria-hidden="true">
            {#if v.tab.kind === "environments"}
              <EnvIcon size={13} />
            {:else if v.tab.kind === "history"}
              <svg viewBox="0 0 16 16" width="13" height="13" fill="none" stroke="currentColor" stroke-width="1.4"
                ><circle cx="8" cy="8" r="6" /><path d="M8 4.5V8l2.5 1.5" /></svg
              >
            {:else if v.tab.kind === "git"}
              <GitIcon size={13} />
            {:else if v.tab.kind === "merge"}
              <svg viewBox="0 0 16 16" width="13" height="13" fill="none" stroke="currentColor" stroke-width="1.4"
                ><circle cx="4" cy="3" r="1.6" /><circle cx="4" cy="13" r="1.6" /><circle cx="12" cy="8" r="1.6" /><path
                  d="M4 4.6v6.8M4.6 4.4C6 7 8 8 10.4 8"
                /></svg
              >
            {:else if v.tab.kind === "container"}
              <svg viewBox="0 0 16 16" width="13" height="13" fill="none" stroke="currentColor" stroke-width="1.4"
                ><rect x="3.5" y="7" width="9" height="7" rx="1.2" /><path d="M5.5 7V5a2.5 2.5 0 015 0v2" /></svg
              >
            {:else if v.tab.kind === "import"}
              <svg viewBox="0 0 16 16" width="13" height="13" fill="none" stroke="currentColor" stroke-width="1.4"
                ><path d="M8 2v8M5 7l3 3 3-3M3 11v2a1 1 0 001 1h8a1 1 0 001-1v-2" /></svg
              >
            {:else if v.tab.kind === "runner"}
              <svg viewBox="0 0 16 16" width="13" height="13"><path d="M4.5 3v10l8-5z" fill="currentColor" /></svg>
            {:else}
              <svg viewBox="0 0 16 16" width="13" height="13"><path d="M9 1.5L3.5 9H8l-1 5.5L12.5 7H8z" fill="currentColor" /></svg>
            {/if}
          </span>
        {/if}
        {#if collapsed}
          {#if v.dirty}<span class="dirty-dot rail-dot"></span>{/if}
        {:else}
          <span class="text">
            <span class="name"><Highlight text={v.title} {terms} /></span>
            {#if v.hint}<span class="hint"><Highlight text={v.hint} {terms} /></span>{/if}
          </span>
          {#if v.dirty}<span class="dirty-dot" title={t("tree.unsaved")}></span>{/if}
          <button
            class="close ghost"
            tabindex="-1"
            title={t("tabs.close")}
            aria-label={t("tabs.close")}
            onclick={(e) => {
              e.stopPropagation();
              ws.closeTab(v.key);
            }}>×</button
          >
        {/if}
      </div>
      {/if}
    {/each}
  </div>
</aside>

<style>
  .tabs-side {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-width: 0;
    background: var(--surface);
  }
  header {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 10px 6px 8px 12px;
    border-bottom: 1px solid var(--border);
    min-height: 41px;
  }
  .collapsed header {
    padding: 10px 0 8px;
    justify-content: center;
  }
  .title {
    font-weight: 600;
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .count {
    font-size: 11px;
    color: var(--muted);
  }
  .toggle {
    margin-left: auto;
    display: flex;
    padding: 4px;
  }
  .collapsed .toggle {
    margin: 0;
  }
  .tabs-search {
    padding: 8px 6px 2px;
  }
  .rail-search {
    display: flex;
    justify-content: center;
    margin: 6px auto 0;
    padding: 5px;
  }
  .list {
    flex: 1;
    overflow: auto;
    padding: 6px 0;
    font-size: 13px;
  }
  .empty {
    color: var(--muted);
    font-size: 12px;
    margin: 8px 12px;
  }
  .tab {
    position: relative;
    display: flex;
    align-items: center;
    gap: 6px;
    min-height: 36px;
    margin: 1px 4px;
    padding: 3px 4px 3px 8px;
    border-radius: 5px;
    cursor: pointer;
    user-select: none;
  }
  .collapsed .tab {
    justify-content: center;
    padding: 3px 0;
    min-height: 30px;
  }
  .tab:hover {
    background: var(--hover);
  }
  .tab.active {
    background: var(--selected);
  }
  .tab:focus-visible {
    outline: 1px solid var(--accent);
  }
  .tab.dragging {
    opacity: 0.5;
  }
  .tab.drop-before::before,
  .tab.drop-after::after {
    content: "";
    position: absolute;
    left: 4px;
    right: 4px;
    height: 2px;
    border-radius: 1px;
    background: var(--accent);
  }
  .tab.drop-before::before {
    top: -2px;
  }
  .tab.drop-after::after {
    bottom: -2px;
  }
  .badge {
    flex: none;
    width: 34px;
    font-size: 10px;
    font-weight: 700;
  }
  .collapsed .badge {
    width: auto;
    text-align: center;
  }
  .badge.kind {
    display: flex;
    justify-content: center;
    color: var(--accent);
  }
  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    line-height: 1.25;
  }
  .name,
  .hint {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .hint {
    font-size: 11px;
    color: var(--muted);
  }
  .rail-dot {
    position: absolute;
    top: 4px;
    right: 6px;
    width: 6px;
    height: 6px;
  }
  .close {
    flex: none;
    visibility: hidden;
    padding: 0 6px;
    line-height: 20px;
  }
  .tab:hover .close,
  .tab:focus-within .close {
    visibility: visible;
  }
</style>
