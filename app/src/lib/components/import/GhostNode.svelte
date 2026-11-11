<script lang="ts">
  // A planned (not yet created) node in the destination tree: dashed and tinted, with a "new" badge.
  // The root of a placement can be renamed and removed; any ghost can be dragged elsewhere (moving it out).
  import type { ImportNode, ImportPlacement } from "$lib/api";
  import { t } from "$lib/i18n/index.svelte";
  import { importer } from "$lib/stores/import.svelte";
  import { placedChildren, type PlaceAs } from "$lib/import/plan";
  import { placeMenu, startDrag } from "$lib/import/ui.svelte";
  import { methodColor, methodLabel } from "../methods";
  import GhostNode from "./GhostNode.svelte";

  interface Props {
    node: ImportNode;
    as: PlaceAs;
    depth: number;
    /** Set for the root of a placement. */
    placement?: ImportPlacement | null;
    placed: Set<string>;
  }

  let { node, as, depth, placement = null, placed }: Props = $props();

  const key = $derived(`ghost:${node.id}`);
  const children = $derived(placedChildren(node, importer.excluded, placed));
  const expanded = $derived(!!importer.expanded[key]);
  const name = $derived(placement?.name ?? node.name);

  let editing = $state(false);
  let draft = $state("");

  function startRename() {
    if (!placement) return;
    draft = name;
    editing = true;
  }

  function commit() {
    if (!editing) return;
    editing = false;
    if (placement) importer.rename(node.id, draft === node.name ? "" : draft);
  }

  function focusSelect(el: HTMLInputElement) {
    el.focus();
    el.select();
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.target !== e.currentTarget) return;
    if (e.key === "F2" && placement) {
      e.preventDefault();
      startRename();
    } else if ((e.key === "Delete" || e.key === "Backspace") && placement) {
      e.preventDefault();
      importer.unplace(node.id);
    } else if (e.key === "Enter" || e.key === "p" || e.key === "P") {
      e.preventDefault();
      const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
      placeMenu(node.id, r.left + 24, r.bottom);
    } else if (e.key === "ArrowRight" && children.length && !expanded) importer.expanded[key] = true;
    else if (e.key === "ArrowLeft" && expanded) importer.expanded[key] = false;
  }
</script>

<div
  class="ghost"
  class:root={!!placement}
  class:hl={importer.selected === node.id}
  role="treeitem"
  aria-selected={importer.selected === node.id}
  aria-expanded={children.length ? expanded : undefined}
  tabindex="0"
  style:margin-left="{4 + depth * 14}px"
  title={placement ? t("import.ghost.rootHint") : t("import.ghost.childHint")}
  onclick={() => (importer.selected = node.id)}
  ondblclick={startRename}
  {onkeydown}
  onpointerdown={(e) => !editing && startDrag(e, node.id, name)}
>
  {#if children.length}
    <button class="chev" class:open={expanded} tabindex="-1" aria-label={t("import.toggle")} onclick={(e) => (e.stopPropagation(), (importer.expanded[key] = !expanded))}>
      <svg viewBox="0 0 16 16" width="11" height="11"><path d="M6 4l4 4-4 4" fill="none" stroke="currentColor" stroke-width="1.6" /></svg>
    </button>
  {:else}
    <span class="chev"></span>
  {/if}
  {#if as === "request"}
    <span class="badge mono" style:color={methodColor(node.method ?? "GET")}>{methodLabel(node.method ?? "GET")}</span>
  {:else}
    <span class="kind" aria-hidden="true">
      {#if as === "collection"}
        <svg viewBox="0 0 16 16" width="13" height="13"><path d="M3 3h10v3H3zM3 7.5h10v3H3zM3 12h10" fill="none" stroke="currentColor" stroke-width="1.3" /></svg>
      {:else}
        <svg viewBox="0 0 16 16" width="13" height="13"><path d="M1.5 4.5a1 1 0 011-1h3.5l1.5 1.5h6a1 1 0 011 1v6.5a1 1 0 01-1 1h-11a1 1 0 01-1-1z" fill="none" stroke="currentColor" stroke-width="1.3" /></svg>
      {/if}
    </span>
  {/if}
  {#if editing}
    <input
      class="rename"
      bind:value={draft}
      use:focusSelect
      aria-label={t("import.ghost.rename")}
      onblur={commit}
      onkeydown={(e) => {
        e.stopPropagation();
        if (e.key === "Enter") commit();
        else if (e.key === "Escape") editing = false;
      }}
    />
  {:else}
    <span class="name">{name}</span>
  {/if}
  {#if placement}
    <span class="new">{t("import.ghost.new")}</span>
    <span class="spacer"></span>
    <button class="icon-btn" tabindex="-1" title={t("import.ghost.rename")} aria-label={t("import.ghost.rename")} onclick={(e) => (e.stopPropagation(), startRename())}>✎</button>
    <button class="icon-btn" tabindex="-1" title={t("import.ghost.remove")} aria-label={t("import.ghost.remove")} onclick={(e) => (e.stopPropagation(), importer.unplace(node.id))}>×</button>
  {/if}
</div>

{#if expanded}
  {#each children as child (child.id)}
    <GhostNode node={child} as={child.kind === "request" ? "request" : "folder"} depth={depth + 1} {placed} />
  {/each}
{/if}

<style>
  .ghost {
    display: flex;
    align-items: center;
    gap: 5px;
    height: 24px;
    padding: 0 6px 0 2px;
    margin-right: 4px;
    margin-top: 1px;
    border: 1px dashed color-mix(in srgb, var(--accent) 60%, transparent);
    border-radius: 5px;
    background: color-mix(in srgb, var(--accent) 8%, transparent);
    cursor: grab;
    user-select: none;
    font-size: 13px;
  }
  .ghost:not(.root) {
    border-color: color-mix(in srgb, var(--accent) 30%, transparent);
    background: color-mix(in srgb, var(--accent) 4%, transparent);
  }
  .ghost.hl {
    border-style: solid;
    border-color: var(--accent);
  }
  .ghost:focus-visible {
    outline: 1px solid var(--accent);
  }
  .chev {
    flex: none;
    width: 14px;
    display: flex;
    padding: 0;
    border: none;
    background: none;
    color: var(--muted);
    transition: transform 0.12s;
  }
  .chev.open {
    transform: rotate(90deg);
  }
  .badge {
    flex: none;
    width: 30px;
    font-size: 10px;
    font-weight: 700;
  }
  .kind {
    flex: none;
    display: flex;
    color: var(--accent);
  }
  .name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .rename {
    flex: 1;
    min-width: 0;
    padding: 0 6px;
    height: 20px;
    font-size: 12px;
  }
  .new {
    flex: none;
    font-size: 10px;
    padding: 0 5px;
    border-radius: 4px;
    background: var(--accent);
    color: var(--accent-text);
    font-weight: 600;
  }
  .spacer {
    flex: 1;
  }
  .icon-btn {
    flex: none;
    border: none;
    background: none;
    padding: 0 4px;
    color: var(--muted);
    line-height: 18px;
  }
  .icon-btn:hover {
    color: var(--text);
    background: var(--hover);
  }
</style>
