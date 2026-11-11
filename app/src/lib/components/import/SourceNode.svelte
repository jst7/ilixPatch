<script lang="ts">
  // One node of the "From Postman" tree: tri-state checkbox, badges, warnings, placement status; draggable.
  import type { ImportNode, ImportWarning } from "$lib/api";
  import { t } from "$lib/i18n/index.svelte";
  import { importer } from "$lib/stores/import.svelte";
  import { checkState, coveringPlacement } from "$lib/import/plan";
  import { placeMenu, startDrag, warningText } from "$lib/import/ui.svelte";
  import { methodColor, methodLabel } from "../methods";
  import SourceNode from "./SourceNode.svelte";

  let { node, depth, warnings }: { node: ImportNode; depth: number; warnings: Map<string, ImportWarning[]> } = $props();

  const check = $derived(checkState(importer.index, importer.excluded, node.id));
  const expanded = $derived(!!importer.expanded[node.id]);
  const own = $derived(importer.placements.find((p) => p.source_id === node.id) ?? null);
  const covered = $derived(own ? null : coveringPlacement(importer.index, importer.placements, node.id));
  const unplaced = $derived(check !== "none" && !own && !covered);
  const nodeWarnings = $derived(warnings.get(node.id) ?? []);
  const selected = $derived(importer.selected === node.id);

  function indeterminate(el: HTMLInputElement, value: boolean) {
    el.indeterminate = value;
    return { update: (v: boolean) => (el.indeterminate = v) };
  }

  function toggleExpand() {
    importer.expanded[node.id] = !expanded;
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.target !== e.currentTarget) return;
    if (e.key === " ") {
      e.preventDefault();
      importer.toggle(node.id, check !== "all");
    } else if (e.key === "Enter" || e.key === "p" || e.key === "P") {
      e.preventDefault();
      const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
      importer.selected = node.id;
      placeMenu(node.id, r.left + 24, r.bottom);
    } else if (e.key === "ArrowRight" && node.children.length && !expanded) {
      e.preventDefault();
      importer.expanded[node.id] = true;
    } else if (e.key === "ArrowLeft" && expanded) {
      e.preventDefault();
      importer.expanded[node.id] = false;
    }
  }

  const placedTitle = $derived.by(() => {
    if (own) return t("import.src.placedAt", { path: own.target_parent });
    if (covered) return t("import.src.covered", { name: importer.index.get(covered.source_id)?.node.name ?? "" });
    return t("import.src.unplacedHint");
  });
</script>

<div
  class="row"
  class:selected
  class:off={check === "none"}
  role="treeitem"
  aria-selected={selected}
  aria-expanded={node.children.length ? expanded : undefined}
  tabindex="0"
  data-src={node.id}
  style:padding-left="{6 + depth * 14}px"
  title={placedTitle}
  onclick={() => (importer.selected = node.id)}
  {onkeydown}
  onpointerdown={(e) => startDrag(e, node.id, node.name)}
  oncontextmenu={(e) => {
    e.preventDefault();
    importer.selected = node.id;
    placeMenu(node.id, e.clientX, e.clientY);
  }}
>
  {#if node.children.length}
    <button class="chev" class:open={expanded} tabindex="-1" aria-label={t("import.toggle")} onclick={(e) => (e.stopPropagation(), toggleExpand())}>
      <svg viewBox="0 0 16 16" width="11" height="11"><path d="M6 4l4 4-4 4" fill="none" stroke="currentColor" stroke-width="1.6" /></svg>
    </button>
  {:else}
    <span class="chev"></span>
  {/if}
  <input
    type="checkbox"
    tabindex="-1"
    aria-label={t("import.src.include", { name: node.name })}
    checked={check === "all"}
    use:indeterminate={check === "some"}
    onclick={(e) => e.stopPropagation()}
    onchange={(e) => importer.toggle(node.id, e.currentTarget.checked)}
  />
  {#if node.kind === "request"}
    <span class="badge mono" style:color={methodColor(node.method ?? "GET")}>{methodLabel(node.method ?? "GET")}</span>
  {:else}
    <span class="kind" aria-hidden="true">
      {#if node.kind === "collection"}
        <svg viewBox="0 0 16 16" width="13" height="13"><path d="M3 3h10v3H3zM3 7.5h10v3H3zM3 12h10" fill="none" stroke="currentColor" stroke-width="1.3" /></svg>
      {:else}
        <svg viewBox="0 0 16 16" width="13" height="13"><path d="M1.5 4.5a1 1 0 011-1h3.5l1.5 1.5h6a1 1 0 011 1v6.5a1 1 0 01-1 1h-11a1 1 0 01-1-1z" fill="none" stroke="currentColor" stroke-width="1.3" /></svg>
      {/if}
    </span>
  {/if}
  <span class="name">{node.name}</span>
  {#if node.has_scripts}<span class="icon mono" title={t("import.src.scripts")}>ƒ</span>{/if}
  {#if node.auth && node.auth !== "noauth" && node.auth !== "inherit"}
    <span class="icon" title={t("import.src.auth", { type: node.auth })}>
      <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true"><rect x="3.5" y="7" width="9" height="6.5" rx="1.2" fill="none" stroke="currentColor" stroke-width="1.3" /><path d="M5.5 7V5a2.5 2.5 0 015 0v2" fill="none" stroke="currentColor" stroke-width="1.3" /></svg>
    </span>
  {/if}
  {#if nodeWarnings.length}
    <span class="warn" title={nodeWarnings.map(warningText).join("\n")} aria-label={t("import.warnings")}>⚠</span>
  {/if}
  <span class="spacer"></span>
  {#if own}
    <span class="chip placed">{t("import.src.placed")}</span>
  {:else if unplaced}
    <span class="chip unplaced">{t("import.src.unplaced")}</span>
  {/if}
</div>

{#if expanded}
  {#each node.children as child (child.id)}
    <SourceNode node={child} depth={depth + 1} {warnings} />
  {/each}
{/if}

<style>
  .row {
    display: flex;
    align-items: center;
    gap: 5px;
    height: 26px;
    padding-right: 6px;
    margin: 0 4px;
    border-radius: 5px;
    cursor: grab;
    user-select: none;
    font-size: 13px;
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
  .row.off .name,
  .row.off .badge {
    opacity: 0.5;
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
  input {
    margin: 0;
    accent-color: var(--accent);
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
    color: var(--muted);
  }
  .name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .icon {
    flex: none;
    display: flex;
    color: var(--muted);
    font-size: 12px;
  }
  .warn {
    flex: none;
    color: var(--warn);
    font-size: 12px;
  }
  .spacer {
    flex: 1;
  }
  .chip {
    flex: none;
    font-size: 10px;
    padding: 0 5px;
    border-radius: 4px;
    border: 1px solid var(--border);
  }
  .chip.placed {
    color: var(--accent);
    border-color: var(--accent);
  }
  .chip.unplaced {
    color: var(--muted);
    border-style: dashed;
  }
</style>
