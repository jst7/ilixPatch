<script lang="ts">
  // "Variables" tab of a request (D48): save values of the response in variables after each send,
  // e.g. the access token of a login, then use it as {{token}} in other requests.
  import type { Extract } from "$lib/api";
  import { t } from "$lib/i18n/index.svelte";
  import { ws, type Draft } from "$lib/stores/workspace.svelte";
  import { previewRule } from "$lib/extract/eval";

  interface Props {
    draft: Draft;
    /** Key of the draft's response in ws.responses (for the preview). */
    path: string;
  }

  let { draft, path }: Props = $props();

  const result = $derived(ws.responses[path]?.result ?? null);
  /** The preview works on the body the UI has (whole when small, the first KB of large bodies). */
  const res = $derived(
    result
      ? {
          status: result.status,
          headers: result.headers,
          body: result.body_handle?.preview_truncated ? null : result.body,
        }
      : null,
  );

  const FROM: { v: Extract["from"]; label: () => string }[] = [
    { v: "json", label: () => t("extract.from.json") },
    { v: "header", label: () => t("extract.from.header") },
    { v: "status", label: () => t("extract.from.status") },
    { v: "regex", label: () => t("extract.from.regex") },
    { v: "body", label: () => t("extract.from.body") },
  ];
  const SCOPES: { v: Extract["scope"]; label: () => string }[] = [
    { v: "environment", label: () => t("extract.scope.environment") },
    { v: "collection", label: () => t("extract.scope.collection") },
    { v: "global", label: () => t("extract.scope.global") },
  ];

  function placeholder(from: Extract["from"]): string {
    if (from === "json") return "data.access_token";
    if (from === "header") return "X-Request-Id";
    if (from === "regex") return "csrf=(\\w+)";
    return "";
  }

  function add() {
    draft.extract = [...draft.extract, { var: "", from: "json", path: "", scope: "environment", enabled: true }];
  }

  function remove(i: number) {
    draft.extract = draft.extract.filter((_, j) => j !== i);
  }

  const short = (v: string) => (v.length > 80 ? `${v.slice(0, 80)}…` : v);
</script>

<div class="extract">
  <p class="hint">{t("extract.hint")}</p>

  {#if draft.extract.length}
    <table>
      <thead>
        <tr>
          <th class="on"></th>
          <th>{t("extract.var")}</th>
          <th>{t("extract.from")}</th>
          <th>{t("extract.path")}</th>
          <th>{t("extract.scope")}</th>
          <th>{t("extract.preview")}</th>
          <th></th>
        </tr>
      </thead>
      <tbody>
        {#each draft.extract as rule, i (i)}
          {@const pv = res && rule.var.trim() ? previewRule(rule, res) : null}
          <tr class:off={!rule.enabled}>
            <td class="on"><input type="checkbox" bind:checked={rule.enabled} aria-label={t("extract.enabled")} /></td>
            <td><input class="mono" bind:value={rule.var} placeholder="token" spellcheck="false" aria-label={t("extract.var")} /></td>
            <td>
              <select bind:value={rule.from} aria-label={t("extract.from")}>
                {#each FROM as f (f.v)}<option value={f.v}>{f.label()}</option>{/each}
              </select>
            </td>
            <td>
              <input
                class="mono"
                bind:value={rule.path}
                placeholder={placeholder(rule.from)}
                disabled={rule.from === "status" || rule.from === "body"}
                spellcheck="false"
                aria-label={t("extract.path")}
              />
            </td>
            <td>
              <select bind:value={rule.scope} aria-label={t("extract.scope")}>
                {#each SCOPES as s (s.v)}<option value={s.v}>{s.label()}</option>{/each}
              </select>
            </td>
            <td class="pv mono">
              {#if !res}
                <span class="muted">{t("extract.sendFirst")}</span>
              {:else if pv?.ok}
                <span class="ok" title={pv.value}>{short(pv.value)}</span>
              {:else if pv}
                <span class="err">{pv.error === "no body" ? t("extract.noBody") : pv.error}</span>
              {/if}
            </td>
            <td><button class="ghost small" aria-label={t("extract.remove")} title={t("extract.remove")} onclick={() => remove(i)}>×</button></td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
  <button class="small add" onclick={add}>+ {t("extract.add")}</button>
</div>

<style>
  .extract {
    padding: 8px 10px;
    overflow: auto;
    min-height: 0;
  }
  .hint {
    margin: 0 0 8px;
    color: var(--muted);
    font-size: 12px;
    line-height: 1.45;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 12px;
  }
  th {
    text-align: left;
    font-weight: 500;
    color: var(--muted);
    padding: 2px 4px;
  }
  td {
    padding: 2px 4px;
  }
  th.on,
  td.on {
    width: 22px;
  }
  td input:not([type="checkbox"]),
  td select {
    width: 100%;
    box-sizing: border-box;
    padding: 3px 6px;
    font-size: 12px;
  }
  .mono {
    font-family: var(--mono);
  }
  tr.off td:not(.on) {
    opacity: 0.5;
  }
  .pv {
    max-width: 240px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ok {
    color: var(--ok);
  }
  .err {
    color: var(--warn);
  }
  .muted {
    color: var(--muted);
  }
  .add {
    margin-top: 8px;
  }
  .small {
    padding: 3px 10px;
    font-size: 12px;
  }
</style>
