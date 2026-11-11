<script lang="ts">
  import { t } from "$lib/i18n/index.svelte";
  import type { KV } from "$lib/stores/workspace.svelte";
  import type { BlameInfo } from "$lib/api";
  import { blameLabel, blameTooltip, isUncommitted } from "$lib/editor/blame";

  interface Props {
    /** Always ends with an empty row (kept that way here). */
    rows: KV[];
    keyPlaceholder?: string;
    valuePlaceholder?: string;
    /** Git annotate: blame per row key (shows a compact "author · age" column when set). */
    annotations?: Record<string, BlameInfo> | null;
    onannotationclick?: (info: BlameInfo) => void;
  }

  let { rows = $bindable(), keyPlaceholder, valuePlaceholder, annotations = null, onannotationclick }: Props = $props();

  /** Case-insensitive fallback (header names). */
  const lowerAnnotations = $derived(
    annotations ? new Map(Object.entries(annotations).map(([k, v]) => [k.toLowerCase(), v] as const)) : null,
  );
  function blameFor(key: string): BlameInfo | null {
    const k = key.trim();
    if (!annotations || !k) return null;
    return annotations[k] ?? lowerAnnotations?.get(k.toLowerCase()) ?? null;
  }

  function ensureTrailing() {
    const last = rows[rows.length - 1];
    if (!last || last.key !== "" || last.value !== "") rows.push({ key: "", value: "" });
  }

  function remove(i: number) {
    rows.splice(i, 1);
    ensureTrailing();
  }
</script>

<table>
  <thead>
    <tr>
      <th>{t("request.key")}</th>
      <th>{t("request.value")}</th>
      <th class="actions"></th>
      {#if annotations}<th class="blame-col"></th>{/if}
    </tr>
  </thead>
  <tbody>
    {#each rows as row, i (i)}
      {@const trailing = i === rows.length - 1}
      <tr class:trailing>
        <td>
          <input
            class="mono"
            bind:value={row.key}
            oninput={ensureTrailing}
            placeholder={trailing ? (keyPlaceholder ?? t("request.key")) : ""}
            spellcheck="false"
            autocomplete="off"
          />
        </td>
        <td>
          <input
            class="mono"
            bind:value={row.value}
            oninput={ensureTrailing}
            placeholder={trailing ? (valuePlaceholder ?? t("request.value")) : ""}
            spellcheck="false"
            autocomplete="off"
          />
        </td>
        <td class="actions">
          {#if !trailing}
            <button
              class="ghost remove"
              title={t("request.removeRow")}
              aria-label={t("request.removeRow")}
              onclick={() => remove(i)}>×</button
            >
          {/if}
        </td>
        {#if annotations}
          {@const info = blameFor(row.key)}
          <td class="blame-col">
            {#if row.key.trim()}
              {#if info && !isUncommitted(info)}
                <button class="blame" title={blameTooltip(info)} onclick={() => onannotationclick?.(info)}
                  >{blameLabel(info)}</button
                >
              {:else}
                <span class="blame uncommitted" title={blameTooltip(null)}>—</span>
              {/if}
            {/if}
          </td>
        {/if}
      </tr>
    {/each}
  </tbody>
</table>

<style>
  table {
    width: 100%;
    border-collapse: collapse;
    table-layout: fixed;
  }
  th {
    text-align: left;
    font-weight: 500;
    font-size: 12px;
    color: var(--muted);
    padding: 6px 4px;
  }
  td {
    padding: 2px 4px;
  }
  .actions {
    width: 32px;
  }
  input {
    width: 100%;
    font-size: 13px;
    padding: 5px 8px;
    border-color: transparent;
    background: var(--surface-2);
  }
  input:focus {
    border-color: var(--accent);
  }
  .blame-col {
    width: 120px;
  }
  .blame {
    display: block;
    width: 100%;
    text-align: left;
    border: none;
    background: none;
    padding: 0 4px;
    font-size: 11px;
    color: var(--blame-text);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  button.blame:hover {
    color: var(--text);
    text-decoration: underline;
  }
  .blame.uncommitted {
    color: var(--muted);
  }
  .remove {
    padding: 0 6px;
    font-size: 16px;
    line-height: 22px;
  }
</style>
