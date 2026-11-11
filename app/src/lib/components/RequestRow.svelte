<script lang="ts">
  import type { RequestFile } from "$lib/api";
  import { t } from "$lib/i18n/index.svelte";
  import { ws, type MenuItem } from "$lib/stores/workspace.svelte";
  import { confirmDelete } from "./deleteItem";
  import { methodColor, methodLabel } from "./methods";
  import Highlight from "./Highlight.svelte";
  import { treeSearch } from "$lib/search/tree.svelte";
  import { allTermsIn, normalize } from "$lib/search/match";
  import { git } from "$lib/stores/git.svelte";
  import { stateColor } from "$lib/git/status";

  interface Props {
    file: RequestFile;
    projectPath: string;
    depth: number;
  }

  let { file, projectPath, depth }: Props = $props();

  const draft = $derived(ws.drafts[file.path]);
  const name = $derived(draft?.name || file.request.name);
  const method = $derived(draft?.method ?? file.request.method);
  const selected = $derived(ws.selectedPath === file.path);
  const dirty = $derived(ws.isDirty(file.path));
  const gitState = $derived(git.fileState.get(file.path));

  function items(): MenuItem[] {
    return [
      { label: t("tree.runRequest"), action: () => ws.openTab({ kind: "runner", target: file.path }) },
      { label: t("tree.delete"), danger: true, action: () => confirmDelete(file.path, name, projectPath) },
    ];
  }

  let rowEl = $state<HTMLDivElement>();
  $effect(() => {
    if (ws.revealPath !== file.path || !rowEl) return;
    rowEl.scrollIntoView({ block: "nearest" });
    rowEl.focus({ preventScroll: true });
    ws.revealPath = null;
  });

  /** While searching, show the URL when one of the terms is found in it. */
  const urlHit = $derived.by(() => {
    const terms = treeSearch.terms;
    if (!terms.length || !file.request.url) return false;
    const url = normalize(file.request.url);
    return terms.some((t) => allTermsIn([t], [url]));
  });

  function openMenu(e: MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    ws.menu = { x: e.clientX, y: e.clientY, items: items() };
  }

  function openMenuFromButton(e: MouseEvent) {
    e.stopPropagation();
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    ws.menu = { x: r.left, y: r.bottom + 2, items: items() };
  }
</script>

<div
  bind:this={rowEl}
  class="row"
  class:selected
  role="treeitem"
  aria-selected={selected}
  tabindex="0"
  style:padding-left="{8 + depth * 14 + 18}px"
  onclick={() => ws.select(file.path)}
  onkeydown={(e) => {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      ws.select(file.path);
    }
  }}
  oncontextmenu={openMenu}
  title={file.path}
>
  <span class="badge" style:color={methodColor(method)}>{methodLabel(method)}</span>
  <span class="name" style:color={gitState ? stateColor(gitState) : undefined}><Highlight text={name} terms={treeSearch.terms} /></span>
  {#if urlHit}<span class="url mono"><Highlight text={file.request.url} terms={treeSearch.terms} /></span>{/if}
  {#if dirty}<span class="dirty-dot" title={t("tree.unsaved")}></span>{/if}
  <button class="more ghost" title={t("tree.actions")} aria-label={t("tree.actions")} onclick={openMenuFromButton}
    >⋯</button
  >
</div>

<style>
  .row {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding-right: 4px;
    cursor: pointer;
    border-radius: 5px;
    margin: 0 4px;
    user-select: none;
  }
  .row:hover {
    background: var(--hover);
  }
  .row.selected {
    background: var(--selected);
  }
  .row:focus-visible {
    outline: 1px solid var(--accent);
  }
  .badge {
    flex: none;
    width: 36px;
    font-family: var(--mono);
    font-size: 10px;
    font-weight: 700;
  }
  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .url {
    flex: 0 1 auto;
    min-width: 0;
    max-width: 55%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 11px;
    color: var(--muted);
  }
  .more {
    visibility: hidden;
    padding: 0 6px;
    line-height: 20px;
  }
  .row:hover .more,
  .row.selected .more {
    visibility: visible;
  }
</style>
