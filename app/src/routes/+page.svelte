<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { t } from "$lib/i18n/index.svelte";
  import { historyKey, ws } from "$lib/stores/workspace.svelte";
  import HistoryHeader from "$lib/components/HistoryHeader.svelte";
  import { dateTime } from "$lib/components/historyUtil";
  import RequestEditor from "$lib/components/RequestEditor.svelte";
  import ResponsePanel from "$lib/components/ResponsePanel.svelte";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import TabsSidebar from "$lib/components/TabsSidebar.svelte";
  import RunnerView from "$lib/components/RunnerView.svelte";
  import ContainerSettingsView from "$lib/components/ContainerSettingsView.svelte";
  import { containerSession } from "$lib/stores/containerSettings.svelte";
  import LoadTestView from "$lib/components/LoadTestView.svelte";
  import GitView from "$lib/components/GitView.svelte";
  import EnvironmentsView from "$lib/components/EnvironmentsView.svelte";
  import { saveEnvTab } from "$lib/components/envActions";
  import { searchFocus } from "$lib/search/focus.svelte";
  import MergeView from "$lib/components/merge/MergeView.svelte";
  import ImportView from "$lib/components/import/ImportView.svelte";
  import NetConsole from "$lib/components/NetConsole.svelte";
  import { netlog } from "$lib/stores/netlog.svelte";

  const LS_LAYOUT = "ilixpatch.layout";

  const TABS_DEFAULT = 240;
  const RAIL_WIDTH = 44;

  interface Layout {
    sidebar: number;
    split: number;
    tabs: number;
    tabsCollapsed: boolean;
  }

  function loadLayout(): Layout {
    const fallback: Layout = { sidebar: 280, split: 0.5, tabs: TABS_DEFAULT, tabsCollapsed: false };
    try {
      const raw = localStorage.getItem(LS_LAYOUT);
      if (raw) {
        const l = JSON.parse(raw);
        return {
          sidebar: Number(l.sidebar) || fallback.sidebar,
          split: Number(l.split) || fallback.split,
          tabs: Number(l.tabs) || fallback.tabs,
          tabsCollapsed: l.tabsCollapsed === true,
        };
      }
    } catch {}
    return fallback;
  }

  const initial = loadLayout();
  let sidebarWidth = $state(initial.sidebar);
  let tabsWidth = $state(initial.tabs);
  let tabsCollapsed = $state(initial.tabsCollapsed);
  /** Fraction of the main column used by the request editor. */
  let split = $state(initial.split);
  let mainEl = $state<HTMLElement>();

  function saveLayout() {
    try {
      localStorage.setItem(LS_LAYOUT, JSON.stringify({ sidebar: sidebarWidth, split, tabs: tabsWidth, tabsCollapsed }));
    } catch {}
  }

  function drag(e: PointerEvent, onmove: (ev: PointerEvent) => void) {
    e.preventDefault();
    const target = e.currentTarget as HTMLElement;
    target.setPointerCapture(e.pointerId);
    const move = (ev: PointerEvent) => onmove(ev);
    const up = () => {
      target.removeEventListener("pointermove", move);
      target.removeEventListener("pointerup", up);
      target.removeEventListener("pointercancel", up);
      saveLayout();
    };
    target.addEventListener("pointermove", move);
    target.addEventListener("pointerup", up);
    target.addEventListener("pointercancel", up);
  }

  function resizeSidebar(e: PointerEvent) {
    drag(e, (ev) => (sidebarWidth = Math.round(Math.min(Math.max(ev.clientX, 180), window.innerWidth * 0.6))));
  }

  function resizeTabs(e: PointerEvent) {
    drag(
      e,
      (ev) => (tabsWidth = Math.round(Math.min(Math.max(window.innerWidth - ev.clientX, 160), window.innerWidth * 0.5))),
    );
  }

  function toggleTabs() {
    tabsCollapsed = !tabsCollapsed;
    saveLayout();
  }

  const columns = $derived(
    tabsCollapsed
      ? `${sidebarWidth}px 4px minmax(0, 1fr) ${RAIL_WIDTH}px`
      : `${sidebarWidth}px 4px minmax(0, 1fr) 4px ${tabsWidth}px`,
  );

  function resizeSplit(e: PointerEvent) {
    drag(e, (ev) => {
      if (!mainEl) return;
      const r = mainEl.getBoundingClientRect();
      split = Math.min(Math.max((ev.clientY - r.top) / r.height, 0.15), 0.85);
    });
  }

  onMount(() => {
    ws.init();
    void netlog.init();
    // The backend records each send in the local history, then emits this event.
    const unlistenHistory = listen<number>("history://recorded", () => ws.refreshHistory());
    // Capture phase so shortcuts win over the code editor's own keymap (e.g. Mod-Enter).
    const onkey = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey)) return;
      // D46: ⌘⌥C / Ctrl+Alt+C toggles the network console.
      if (e.altKey && !e.shiftKey && e.code === "KeyC") {
        e.preventDefault();
        netlog.toggle();
        return;
      }
      const stop = () => {
        e.preventDefault();
        e.stopPropagation();
      };
      // Ctrl+Tab / Ctrl+Shift+Tab cycle tabs (Cmd+Tab belongs to the OS on macOS).
      if (e.key === "Tab" && e.ctrlKey && !e.metaKey && !e.altKey) {
        stop();
        ws.cycleTab(e.shiftKey ? -1 : 1);
        return;
      }
      // Cmd/Ctrl+\ toggles the tabs panel. Some layouts (e.g. Spanish) need Alt to type "\", so allow it.
      if ((e.key === "\\" || e.code === "Backslash") && !e.shiftKey) {
        stop();
        toggleTabs();
        return;
      }
      // Cmd/Ctrl+Shift+F outside a code editor: search the sidebar (tree or history), like Cmd/Ctrl+P.
      if (e.shiftKey && !e.altKey && (e.key === "f" || e.key === "F") && !(e.target as Element | null)?.closest?.(".cm-editor")) {
        stop();
        searchFocus.sidebar++;
        return;
      }
      if (e.altKey || e.shiftKey) return;
      if (e.key === "p" || e.key === "P") {
        // Quick find: focus the visible sidebar search (project tree or history).
        stop();
        searchFocus.sidebar++;
      } else if (e.key === "e" || e.key === "E") {
        // Search the open tabs (expands the tabs panel if it is collapsed).
        stop();
        if (tabsCollapsed) toggleTabs();
        searchFocus.tabs++;
      } else if (e.key === "w" || e.key === "W") {
        stop();
        ws.closeTab();
      } else if (e.key === "s" || e.key === "S") {
        e.preventDefault();
        e.stopPropagation();
        const tab = ws.activeTab;
        if (tab?.kind === "history") ws.historySaveRequest = tab.id;
        else if (tab?.kind === "environments") void saveEnvTab(tab.project);
        else if (tab?.kind === "container") void containerSession(tab.path).save();
        else ws.save();
      } else if (e.key === "Enter") {
        e.preventDefault();
        e.stopPropagation();
        const tab = ws.activeTab;
        if (tab?.kind === "history") ws.sendHistory(tab.id);
        else ws.send();
      } else if (e.key === ".") {
        // ⌘. / Ctrl+. aborts a hung request of the active tab.
        e.preventDefault();
        e.stopPropagation();
        ws.cancelSend(ws.activeSendKey());
      }
    };
    // Git state may change outside the app (terminal, IDE): refresh on focus (throttled in the store).
    const onfocus = () => void ws.refreshAllRepos();
    window.addEventListener("keydown", onkey, true);
    window.addEventListener("focus", onfocus);
    return () => {
      window.removeEventListener("keydown", onkey, true);
      window.removeEventListener("focus", onfocus);
      void unlistenHistory.then((off) => off());
    };
  });
</script>

<div class="app" style:grid-template-columns={columns}>
  <div class="side"><Sidebar /></div>

  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="gutter v" onpointerdown={resizeSidebar} ondblclick={() => ((sidebarWidth = 280), saveLayout())}></div>

  <main>
    <div class="workarea" bind:this={mainEl}>
    {#if ws.activeTab?.kind === "runner"}
      {#key ws.activeTab.target}<RunnerView target={ws.activeTab.target} />{/key}
    {:else if ws.activeTab?.kind === "load"}
      {#key ws.activeTab.target}<LoadTestView target={ws.activeTab.target} />{/key}
    {:else if ws.activeTab?.kind === "git"}
      {#key ws.activeTab.repo}<GitView repo={ws.activeTab.repo} />{/key}
    {:else if ws.activeTab?.kind === "container"}
      {#key ws.activeTab.path}<ContainerSettingsView path={ws.activeTab.path} />{/key}
    {:else if ws.activeTab?.kind === "import"}
      <ImportView />
    {:else if ws.activeTab?.kind === "merge"}
      {#key ws.activeTab.path}<MergeView repo={ws.activeTab.repo} path={ws.activeTab.path} />{/key}
    {:else if ws.activeTab?.kind === "environments"}
      {#key ws.activeTab.project}<EnvironmentsView project={ws.activeTab.project} />{/key}
    {:else if ws.activeTab?.kind === "history"}
      {@const id = ws.activeTab.id}
      {@const hst = ws.historyTabs[id]}
      {#if hst?.entry && ws.drafts[historyKey(id)]}
        {#key id}
          <div class="pane" style:flex-basis="{split * 100}%">
            <RequestEditor
              draftKey={historyKey(id)}
              collection={ws.historyCollection(id)}
              onsend={() => ws.sendHistory(id)}
            >
              {#snippet header()}<HistoryHeader {id} />{/snippet}
            </RequestEditor>
          </div>
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div class="gutter h" onpointerdown={resizeSplit} ondblclick={() => ((split = 0.5), saveLayout())}></div>
          <div class="pane grow history-response">
            {#if !hst.live}
              <div class="history-banner">
                <span>{t("history.fromHistory", { date: dateTime(hst.entry.timestamp) })}</span>
                {#if hst.entry.response?.body_truncated}
                  <span class="truncated">{t("history.bodyTruncated")}</span>
                {/if}
              </div>
            {/if}
            <div class="history-panel"><ResponsePanel path={historyKey(id)} /></div>
          </div>
        {/key}
      {:else}
        <div class="placeholder">
          {#if hst?.error}
            <p class="ph-title">{t("history.openError")}</p>
            <p class="err-text">{hst.error}</p>
          {:else}
            <p>{t("common.loading")}</p>
          {/if}
        </div>
      {/if}
    {:else if ws.selected}
      <div class="pane" style:flex-basis="{split * 100}%">
        <RequestEditor loc={ws.selected} />
      </div>
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="gutter h" onpointerdown={resizeSplit} ondblclick={() => ((split = 0.5), saveLayout())}></div>
      <div class="pane grow">
        <ResponsePanel path={ws.selected.file.path} />
      </div>
    {:else}
      <div class="placeholder">
        <p class="ph-title">{t("main.emptyTitle")}</p>
        <p>{t("main.emptyBody")}</p>
        <p class="keys">{t("main.shortcuts")}</p>
        <p class="keys">{t("main.shortcutsTabs")}</p>
        <p class="keys">{t("main.shortcutsSearch")}</p>
      </div>
    {/if}
    </div>
    {#if netlog.open}<NetConsole />{/if}
  </main>

  {#if !tabsCollapsed}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="gutter v" onpointerdown={resizeTabs} ondblclick={() => ((tabsWidth = TABS_DEFAULT), saveLayout())}></div>
  {/if}
  <div class="side right" class:rail={tabsCollapsed}><TabsSidebar collapsed={tabsCollapsed} ontoggle={toggleTabs} /></div>
</div>

{#if ws.notice}
  <div class="notice" role="alert">
    <span>{ws.notice}</span>
    <button class="ghost" aria-label={t("common.dismiss")} title={t("common.dismiss")} onclick={() => (ws.notice = null)}
      >×</button
    >
  </div>
{/if}

<style>
  .app {
    display: grid;
    height: 100vh;
    overflow: hidden;
  }
  .side.rail {
    border-left: 1px solid var(--border);
  }
  .side {
    min-width: 0;
    min-height: 0;
    overflow: hidden;
  }
  .gutter {
    background: var(--border);
    touch-action: none;
  }
  .gutter.v {
    cursor: col-resize;
    width: 1px;
    margin: 0 1.5px;
    position: relative;
  }
  .gutter.v::after,
  .gutter.h::after {
    content: "";
    position: absolute;
  }
  .gutter.v::after {
    inset: 0 -4px;
  }
  .gutter.h {
    cursor: row-resize;
    height: 1px;
    flex: none;
    position: relative;
  }
  .gutter.h::after {
    inset: -4px 0;
  }
  .gutter:hover {
    background: var(--accent);
  }
  main {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    background: var(--bg);
  }
  .workarea {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }
  .pane {
    min-height: 0;
    flex-grow: 0;
    flex-shrink: 0;
    overflow: hidden;
  }
  .pane.grow {
    flex: 1 1 0;
  }
  .history-response {
    display: flex;
    flex-direction: column;
  }
  .history-panel {
    flex: 1;
    min-height: 0;
  }
  .history-banner {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 12px;
    padding: 4px 12px;
    font-size: 12px;
    color: var(--muted);
    background: var(--surface-2);
    border-bottom: 1px solid var(--border);
  }
  .truncated {
    color: var(--warn);
  }
  .err-text {
    color: var(--err);
    word-break: break-word;
  }
  .placeholder {
    margin: auto;
    text-align: center;
    color: var(--muted);
    max-width: 360px;
    padding: 16px;
  }
  .ph-title {
    color: var(--text);
    font-weight: 600;
    font-size: 16px;
  }
  .keys {
    font-family: var(--mono);
    font-size: 12px;
  }
  .notice {
    position: fixed;
    bottom: 16px;
    right: 16px;
    max-width: min(480px, calc(100vw - 32px));
    display: flex;
    gap: 8px;
    align-items: flex-start;
    background: var(--surface);
    color: var(--err);
    border: 1px solid var(--err);
    border-radius: 8px;
    padding: 8px 8px 8px 12px;
    box-shadow: var(--shadow);
    z-index: 60;
    word-break: break-word;
    font-size: 13px;
  }
  .notice button {
    padding: 0 6px;
  }
</style>
