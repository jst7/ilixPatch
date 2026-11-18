<script lang="ts">
  // An array of objects as a sortable table (D63). Click a cell to select that node in the tree.
  import { childPath, kindOf, summary, tableColumns, type Json } from "$lib/json/model";
  import { t } from "$lib/i18n/index.svelte";

  interface Props {
    rows: Json[];
    path: string;
    onselect: (path: string) => void;
  }
  let { rows, path, onselect }: Props = $props();

  const MAX_ROWS = 2000;
  const columns = $derived(tableColumns(rows) ?? []);
  let sortBy = $state<string | null>(null);
  let desc = $state(false);

  function cell(row: Json, col: string): Json | undefined {
    return row && typeof row === "object" && !Array.isArray(row) ? row[col] : undefined;
  }

  function cmp(a: Json | undefined, b: Json | undefined): number {
    if (a === undefined) return 1;
    if (b === undefined) return -1;
    if (typeof a === "number" && typeof b === "number") return a - b;
    return String(a ?? "").localeCompare(String(b ?? ""), undefined, { numeric: true });
  }

  const order = $derived.by(() => {
    const idx = rows.map((_, i) => i).slice(0, MAX_ROWS);
    if (sortBy === null) return idx;
    const col = sortBy;
    idx.sort((i, j) => cmp(cell(rows[i], col), cell(rows[j], col)) * (desc ? -1 : 1));
    return idx;
  });

  function sort(col: string) {
    if (sortBy === col) desc = !desc;
    else {
      sortBy = col;
      desc = false;
    }
  }

  function show(v: Json | undefined): string {
    if (v === undefined) return "";
    const k = kindOf(v);
    return k === "string" ? (v as string) : k === "array" || k === "object" ? summary(v, 40) : String(v);
  }
</script>

<div class="wrap">
  <table>
    <thead>
      <tr>
        <th class="idx">#</th>
        {#each columns as c (c)}
          <th>
            <button onclick={() => sort(c)}>{c}{#if sortBy === c}{desc ? " ▾" : " ▴"}{/if}</button>
          </th>
        {/each}
      </tr>
    </thead>
    <tbody>
      {#each order as i (i)}
        <tr>
          <td class="idx">{i}</td>
          {#each columns as c (c)}
            {@const v = cell(rows[i], c)}
            <td class={v === undefined ? "missing" : kindOf(v)} onclick={() => onselect(childPath(childPath(path, i), c))} title={show(v)}>{show(v)}</td>
          {/each}
        </tr>
      {/each}
    </tbody>
  </table>
  {#if rows.length > MAX_ROWS}<p class="hint">{t("json.tableCut", { shown: MAX_ROWS, total: rows.length })}</p>{/if}
</div>

<style>
  .wrap {
    overflow: auto;
    height: 100%;
  }
  table {
    border-collapse: collapse;
    font-size: var(--fs-sm);
    font-family: var(--mono);
  }
  th {
    position: sticky;
    top: 0;
    background: var(--surface);
    border-bottom: 1px solid var(--border);
    text-align: left;
  }
  th button {
    border: none;
    background: none;
    padding: 3px 8px;
    font-weight: 600;
    white-space: nowrap;
  }
  td {
    padding: 2px 8px;
    border-bottom: 1px solid color-mix(in srgb, var(--border) 50%, transparent);
    max-width: 280px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    cursor: pointer;
  }
  tr:hover td {
    background: var(--hover);
  }
  .idx {
    color: var(--muted);
    text-align: right;
  }
  td.number {
    color: var(--warn);
    text-align: right;
  }
  td.string {
    color: var(--ok);
  }
  td.boolean,
  td.null {
    color: var(--err);
  }
  td.array,
  td.object,
  td.missing {
    color: var(--muted);
  }
  .hint {
    color: var(--muted);
    font-size: var(--fs-xs);
  }
</style>
