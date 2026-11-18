<script lang="ts">
  // One node of the rich JSON tree (D63). Children are rendered only when expanded; big arrays page by 100.
  import { childPath, kindOf, summary, tableColumns, type Json } from "$lib/json/model";
  import type { JsonViewState } from "$lib/json/view.svelte";
  import { t } from "$lib/i18n/index.svelte";
  import JsonNode from "./JsonNode.svelte";

  interface Props {
    value: Json;
    /** Key or index shown before the value (null for the root). */
    name: string | number | null;
    path: string;
    view: JsonViewState;
    ontable?: (path: string) => void;
  }
  let { value, name, path, view, ontable }: Props = $props();

  const kind = $derived(kindOf(value));
  const container = $derived(kind === "array" || kind === "object");
  const open = $derived(container && view.expanded.has(path));
  const entries = $derived.by((): [string | number, Json][] => {
    if (!open) return [];
    if (Array.isArray(value)) return value.slice(0, view.limit(path)).map((v, i) => [i, v]);
    return Object.entries(value as Record<string, Json>);
  });
  const total = $derived(Array.isArray(value) ? value.length : container ? Object.keys(value as object).length : 0);
  const tabular = $derived(kind === "array" && tableColumns(value) !== null);
  const isMatch = $derived(view.matchSet.has(path));
  const isSelected = $derived(view.selected === path);
  let row: HTMLDivElement | undefined = $state();

  $effect(() => {
    if (isSelected) row?.scrollIntoView({ block: "nearest" });
  });

  /** Collapsed preview: the keys of an object, the first items of an array (the count is in the badge). */
  function preview(v: Json): string {
    if (Array.isArray(v)) {
      const items = v.slice(0, 3).map((x) => summary(x, 24));
      return items.length ? `${items.join(", ")}${v.length > 3 ? ", …" : ""}` : "";
    }
    return summary(v);
  }

  function select() {
    view.selected = path;
  }
</script>

<div class="node">
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    bind:this={row}
    class="row"
    class:selected={isSelected}
    class:match={isMatch}
    role="treeitem"
    tabindex="-1"
    aria-expanded={container ? open : undefined}
    aria-selected={isSelected}
    onclick={select}
    ondblclick={() => container && view.toggle(path)}
  >
    {#if container}
      <button class="twisty" aria-label={open ? "−" : "+"} onclick={(e) => (e.stopPropagation(), view.toggle(path))}>{open ? "▾" : "▸"}</button>
    {:else}
      <span class="twisty"></span>
    {/if}
    {#if name !== null}
      <span class={typeof name === "number" ? "index" : "key"}>{typeof name === "number" ? name : name}</span><span class="colon">:</span>
    {/if}
    {#if container}
      <span class="badge">{kind === "array" ? `[${total}]` : `{${total}}`}</span>
      {#if !open}<span class="preview">{preview(value)}</span>{/if}
      {#if tabular && ontable}
        <button class="table-btn" title={t("json.asTable")} onclick={(e) => (e.stopPropagation(), ontable(path))}>⊞</button>
      {/if}
    {:else}
      <span class="v {kind}">{kind === "string" ? JSON.stringify(value) : String(value)}</span>
    {/if}
  </div>
  {#if open}
    <div class="children">
      {#each entries as [k, v] (k)}
        <JsonNode value={v} name={k} path={childPath(path, k)} {view} {ontable} />
      {/each}
      {#if Array.isArray(value) && value.length > view.limit(path)}
        <button class="more" onclick={() => view.showMore(path)}>
          {t("json.showMore", { shown: view.limit(path), total: value.length })}
        </button>
      {/if}
    </div>
  {/if}
</div>

<style>
  .row {
    display: flex;
    align-items: baseline;
    gap: 4px;
    padding: 0 4px;
    border-radius: 3px;
    white-space: nowrap;
    cursor: default;
    line-height: 1.55;
  }
  .row:hover {
    background: var(--hover);
  }
  .row.match {
    background: color-mix(in srgb, var(--warn) 22%, transparent);
  }
  .row.selected {
    background: var(--selected);
  }
  .children {
    margin-left: 14px;
    border-left: 1px solid color-mix(in srgb, var(--border) 70%, transparent);
    padding-left: 2px;
  }
  .twisty {
    display: inline-flex;
    justify-content: center;
    width: 12px;
    padding: 0;
    border: none;
    background: none;
    color: var(--muted);
    font-size: var(--fs-xs);
    flex: none;
  }
  .key {
    color: var(--accent);
  }
  .index {
    color: var(--muted);
  }
  .colon {
    color: var(--muted);
    margin-right: 2px;
  }
  .badge {
    color: var(--muted);
    font-size: var(--fs-xs);
  }
  .preview {
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .v {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .v.string {
    color: var(--ok);
  }
  .v.number {
    color: var(--warn);
  }
  .v.boolean,
  .v.null {
    color: var(--err);
  }
  .table-btn {
    padding: 0 4px;
    border: none;
    background: none;
    color: var(--muted);
    line-height: 1;
  }
  .table-btn:hover {
    color: var(--text);
  }
  .more {
    margin: 2px 0 2px 16px;
    padding: 1px 8px;
    font-size: var(--fs-xs);
  }
</style>
