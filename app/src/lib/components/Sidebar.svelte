<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { t } from "$lib/i18n/index.svelte";
  import { ws } from "$lib/stores/workspace.svelte";
  import CloneDialog from "./CloneDialog.svelte";
  import { importer } from "$lib/stores/import.svelte";
  import ContextMenu from "./ContextMenu.svelte";
  import GitToasts from "./GitToasts.svelte";
  import HistoryView from "./HistoryView.svelte";
  import SettingsDialog from "./SettingsDialog.svelte";
  import AboutDialog from "./AboutDialog.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import TreeNode from "./TreeNode.svelte";
  import TreeSearchBar from "./TreeSearchBar.svelte";
  import { treeSearch } from "$lib/search/tree.svelte";
  import { treeKeydown } from "$lib/search/treeNav";

  let settingsOpen = $state(false);
  let aboutOpen = $state(false);
  let cloneOpen = $state(false);

  /** "+" menu: open an existing folder or clone a repository. */
  function addMenu(e: MouseEvent) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    ws.menu = {
      x: r.left,
      y: r.bottom + 2,
      items: [
        { label: t("sidebar.openFolder"), action: openFolder },
        { label: t("sidebar.cloneRepo"), action: () => (cloneOpen = true) },
        { label: t("sidebar.importPostman"), action: () => importer.open() },
      ],
    };
  }

  const LS_VIEW = "ilixpatch.sidebarView";
  type View = "collections" | "history";

  function loadView(): View {
    try {
      return localStorage.getItem(LS_VIEW) === "history" ? "history" : "collections";
    } catch {
      return "collections";
    }
  }

  let view = $state<View>(loadView());

  function setView(v: View) {
    view = v;
    try {
      localStorage.setItem(LS_VIEW, v);
    } catch {}
  }

  const views: { id: View; label: () => string }[] = [
    { id: "collections", label: () => t("sidebar.viewCollections") },
    { id: "history", label: () => t("sidebar.viewHistory") },
  ];

  async function openFolder() {
    try {
      const dir = await open({ directory: true, multiple: false, title: t("sidebar.openFolderTitle") });
      if (typeof dir === "string") await ws.addProject(dir);
    } catch (e) {
      ws.fail(e);
    }
  }
</script>

<aside class="sidebar">
  <div class="segmented" role="tablist" aria-label={t("sidebar.view")}>
    {#each views as v (v.id)}
      <button
        role="tab"
        aria-selected={view === v.id}
        class:active={view === v.id}
        onclick={() => setView(v.id)}
        onkeydown={(e) => {
          if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
            e.preventDefault();
            const next = view === "collections" ? 1 : 0;
            setView(views[next].id);
            (e.currentTarget.parentElement?.children[next] as HTMLElement | undefined)?.focus();
          }
        }}>{v.label()}</button
      >
    {/each}
  </div>

  {#if view === "history"}
    <HistoryView />
  {:else}
  <header>
    <span class="title-wrap">
      <span class="title">{t("sidebar.title")}</span>
      <button
        class="env-pill"
        title={t("env.openGlobal")}
        aria-label={t("env.openGlobal")}
        onclick={() => ws.openEnvironments(null)}>{t("env.pill")}</button
      >
    </span>
    <div class="header-actions">
      <!-- Single entry point for adding projects: open folder / clone / import (D37). -->
      <button
        class="ghost small add"
        title={t("sidebar.addProject")}
        aria-label={t("sidebar.addProject")}
        aria-haspopup="menu"
        onclick={addMenu}>+</button
      >
    </div>
  </header>

  {#if ws.entries.length > 0}<TreeSearchBar />{/if}

  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="tree" role="tree" tabindex="-1" onkeydown={treeKeydown}>
    {#if !ws.loaded}
      <p class="hint">{t("common.loading")}</p>
    {:else if ws.entries.length === 0}
      <div class="empty">
        <p class="empty-title">{t("sidebar.emptyTitle")}</p>
        <p class="hint">{t("sidebar.emptyBody")}</p>
        <p class="hint">{t("sidebar.emptyPlusHint")}</p>
      </div>
    {:else}
      {#each ws.entries as entry (entry.path)}
        {#if !treeSearch.result || treeSearch.result.visible.has(entry.path)}
          <TreeNode kind="project" {entry} depth={0} />
        {/if}
      {/each}
      {#if treeSearch.result && treeSearch.result.visible.size === 0}
        <p class="hint">{t("search.noTreeMatches")}</p>
      {/if}
    {/if}
  </div>
  {/if}

  <footer>
    <button
      class="ghost icon"
      title={t("settings.open")}
      aria-label={t("settings.open")}
      onclick={() => (settingsOpen = true)}
    >
      <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
        <circle cx="12" cy="12" r="3" />
        <path d="M19.4 15a1.65 1.65 0 00.33 1.82l.06.06a2 2 0 11-2.83 2.83l-.06-.06a1.65 1.65 0 00-1.82-.33 1.65 1.65 0 00-1 1.51V21a2 2 0 11-4 0v-.09A1.65 1.65 0 009 19.4a1.65 1.65 0 00-1.82.33l-.06.06a2 2 0 11-2.83-2.83l.06-.06A1.65 1.65 0 004.68 15a1.65 1.65 0 00-1.51-1H3a2 2 0 110-4h.09A1.65 1.65 0 004.6 9a1.65 1.65 0 00-.33-1.82l-.06-.06a2 2 0 112.83-2.83l.06.06A1.65 1.65 0 009 4.68a1.65 1.65 0 001-1.51V3a2 2 0 114 0v.09a1.65 1.65 0 001 1.51 1.65 1.65 0 001.82-.33l.06-.06a2 2 0 112.83 2.83l-.06.06A1.65 1.65 0 0019.4 9a1.65 1.65 0 001.51 1H21a2 2 0 110 4h-.09a1.65 1.65 0 00-1.51 1z" />
      </svg>
    </button>
    <button
      class="ghost icon"
      title={t("about.open")}
      aria-label={t("about.open")}
      onclick={() => (aboutOpen = true)}
    >
      <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
        <circle cx="12" cy="12" r="9" />
        <path d="M12 11v5M12 8h.01" />
      </svg>
    </button>
  </footer>
</aside>

<ContextMenu />
<SettingsDialog
  open={settingsOpen || ui.settingsOpen}
  section={ui.settingsSection}
  onclose={() => {
    settingsOpen = false;
    ui.closeSettings();
  }}
/>
<AboutDialog open={aboutOpen} onclose={() => (aboutOpen = false)} />
<CloneDialog open={cloneOpen} onclose={() => (cloneOpen = false)} />
<GitToasts />

<style>
  .sidebar {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-width: 0;
    background: var(--surface);
  }
  .segmented {
    display: flex;
    gap: 2px;
    margin: 8px 8px 0;
    padding: 2px;
    border-radius: 7px;
    background: var(--surface-2);
    border: 1px solid var(--border);
  }
  .segmented button {
    flex: 1;
    border: none;
    background: none;
    padding: 3px 8px;
    font-size: 12px;
    border-radius: 5px;
    color: var(--muted);
  }
  .segmented button.active {
    background: var(--surface);
    color: var(--text);
    box-shadow: 0 0 0 1px var(--border);
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 10px 8px 8px 12px;
    border-bottom: 1px solid var(--border);
  }
  .title {
    font-weight: 600;
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--muted);
  }
  .small {
    padding: 3px 8px;
    font-size: 12px;
  }
  .title-wrap {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .env-pill {
    flex: none;
    padding: 0 6px;
    height: 18px;
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.05em;
    border-radius: 9px;
    border: 1px solid var(--border);
    background: var(--surface-2);
    color: var(--muted);
  }
  .env-pill:hover {
    color: var(--accent);
    border-color: var(--accent);
  }
  .header-actions {
    display: flex;
    align-items: center;
    gap: 2px;
  }
  .add {
    font-size: 16px;
    font-weight: 600;
    line-height: 1;
    padding: 2px 8px;
    color: var(--accent);
  }
  .tree:focus {
    outline: none;
  }
  .tree {
    flex: 1;
    overflow: auto;
    padding: 6px 0;
    font-size: 13px;
  }
  .hint {
    color: var(--muted);
    font-size: 12px;
    margin: 8px 12px;
  }
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    padding: 32px 16px;
    gap: 4px;
  }
  .empty-title {
    font-weight: 600;
    margin: 0;
  }
  .empty .hint {
    margin: 0 0 12px;
  }
  footer {
    display: flex;
    justify-content: flex-start;
    padding: 6px 8px;
    border-top: 1px solid var(--border);
  }
  .icon {
    display: flex;
    padding: 5px;
  }
</style>
