<script lang="ts">
  // Rich JSON view of a response (D63): tree with types and counts, search, levels, table view of
  // arrays of objects, and actions on the selected node (copy path/value, save it in a variable).
  import { stats, tableColumns, type Json } from "$lib/json/model";
  import { JsonViewState, valueAt } from "$lib/json/view.svelte";
  import { t } from "$lib/i18n/index.svelte";
  import { ws } from "$lib/stores/workspace.svelte";
  import JsonNode from "./JsonNode.svelte";
  import JsonTable from "./JsonTable.svelte";

  interface Props {
    value: Json;
    /** Draft key of the request (to add a response-variable rule); null when there is none. */
    draftKey: string | null;
  }
  let { value, draftKey }: Props = $props();

  const view = new JsonViewState();
  let tablePath = $state<string | null>(null);
  let copied = $state<string | null>(null);

  const info = $derived(stats(value));
  // Open the first levels of small/medium documents right away.
  let initialised = false;
  $effect(() => {
    if (initialised) return;
    initialised = true;
    view.expandTo(value, info.nodes > 3000 ? 1 : 2);
  });

  const selectedValue = $derived(view.selected !== null ? valueAt(value, view.selected) : undefined);
  const tableRows = $derived(tablePath !== null ? (valueAt(value, tablePath) as Json[] | undefined) : undefined);

  function copy(text: string, what: string) {
    void navigator.clipboard.writeText(text).then(() => {
      copied = what;
      setTimeout(() => (copied = null), 1200);
    });
  }

  /** D48: a response-variable rule for the selected node. */
  function saveAsVariable() {
    const draft = draftKey ? ws.drafts[draftKey] : null;
    if (!draft || view.selected === null) return;
    const last = view.selected.match(/([A-Za-z_$][\w$]*)(?!.*[A-Za-z_$][\w$]*)/)?.[1] ?? "value";
    draft.extract = [...draft.extract, { var: last, from: "json", path: view.selected, scope: "environment", enabled: true }];
    ws.notify(t("json.savedVariable", { name: last }));
  }

  function onSearchKey(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      if (view.matches.length && view.query.trim()) view.step(value, e.shiftKey ? -1 : 1);
      else view.runSearch(value);
    } else if (e.key === "Escape") {
      view.query = "";
      view.runSearch(value);
    }
  }
</script>

<div class="jview">
  <div class="bar">
    <input
      class="search"
      placeholder={t("json.search")}
      aria-label={t("json.search")}
      bind:value={view.query}
      oninput={() => view.runSearch(value)}
      onkeydown={onSearchKey}
    />
    {#if view.query.trim()}
      <span class="count">{view.matches.length ? `${view.current + 1}/${view.matches.length}` : t("json.noMatches")}</span>
      <button class="ghost small" disabled={!view.matches.length} onclick={() => view.step(value, -1)} aria-label="↑">↑</button>
      <button class="ghost small" disabled={!view.matches.length} onclick={() => view.step(value, 1)} aria-label="↓">↓</button>
    {/if}
    <span class="sep"></span>
    <span class="label">{t("json.expand")}</span>
    <button class="ghost small" onclick={() => view.expandTo(value, 1)}>1</button>
    <button class="ghost small" onclick={() => view.expandTo(value, 2)}>2</button>
    <button class="ghost small" onclick={() => view.expandTo(value, 3)}>3</button>
    <button class="ghost small" onclick={() => view.expandTo(value, Infinity)} disabled={info.nodes > 20000} title={info.nodes > 20000 ? t("json.tooBig") : ""}>{t("json.all")}</button>
    <button class="ghost small" onclick={() => view.collapseAll()}>{t("json.collapse")}</button>
    <span class="stats">{t("json.stats", { nodes: info.nodes, depth: info.depth })}</span>
  </div>

  {#if view.selected !== null}
    <div class="selbar">
      <code class="path" title={view.selected || "$"}>{view.selected || "$"}</code>
      <button class="ghost small" onclick={() => copy(view.selected ?? "", "path")}>{copied === "path" ? "✓" : t("json.copyPath")}</button>
      <button class="ghost small" onclick={() => copy(JSON.stringify(selectedValue, null, 2), "value")}>{copied === "value" ? "✓" : t("json.copyValue")}</button>
      {#if draftKey && view.selected}
        <button class="ghost small" title={t("json.saveVariableHint")} onclick={saveAsVariable}>{t("json.saveVariable")}</button>
      {/if}
      {#if selectedValue !== undefined && tableColumns(selectedValue) && tablePath !== view.selected}
        <button class="ghost small" onclick={() => (tablePath = view.selected)}>⊞ {t("json.asTable")}</button>
      {/if}
    </div>
  {/if}

  <div class="body">
    {#if tablePath !== null && Array.isArray(tableRows)}
      <div class="tablehead">
        <button class="ghost small" onclick={() => (tablePath = null)}>← {t("json.backToTree")}</button>
        <code>{tablePath || "$"}</code>
        <span class="muted">{t("json.rows", { count: tableRows.length })}</span>
      </div>
      <div class="table">
        <JsonTable rows={tableRows} path={tablePath} onselect={(p) => (view.reveal(value, p), (tablePath = null))} />
      </div>
    {:else}
      <div class="tree" role="tree">
        <JsonNode {value} name={null} path="" {view} ontable={(p) => ((view.selected = p), (tablePath = p))} />
      </div>
    {/if}
  </div>
</div>

<style>
  .jview {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }
  .bar,
  .selbar,
  .tablehead {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 4px 6px;
    border-bottom: 1px solid var(--border);
    font-size: var(--fs-sm);
    flex-wrap: wrap;
  }
  .selbar {
    background: color-mix(in srgb, var(--accent) 6%, transparent);
  }
  .search {
    width: 200px;
    padding: 2px 8px;
  }
  .count,
  .label,
  .stats,
  .muted {
    color: var(--muted);
    font-size: var(--fs-xs);
  }
  .stats {
    margin-left: auto;
  }
  .sep {
    width: 8px;
  }
  .path {
    max-width: 50%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-sm);
  }
  .body {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .tree {
    flex: 1;
    overflow: auto;
    padding: 4px 2px;
    font-family: var(--mono);
    font-size: var(--fs-sm);
  }
  .table {
    flex: 1;
    min-height: 0;
  }
</style>
