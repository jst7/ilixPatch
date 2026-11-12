<script lang="ts">
  // Header of large/binary bodies (D41): size, storage badge, incomplete warning, Save to file….
  import type { Snippet } from "svelte";
  import { save } from "@tauri-apps/plugin-dialog";
  import { api, type BodyHandle } from "$lib/api";
  import { t } from "$lib/i18n/index.svelte";
  import { formatBytes } from "$lib/largebody/format";
  import { ui } from "$lib/stores/ui.svelte";

  interface Props {
    handle: BodyHandle;
    /** Save the pretty-printed view instead of the raw body. */
    pretty?: boolean;
    children?: Snippet;
  }

  let { handle, pretty = false, children }: Props = $props();

  let saving = $state(false);
  let saved = $state<string | null>(null);
  let error = $state<string | null>(null);

  function defaultName(): string {
    const ct = (handle.content_type ?? "").toLowerCase();
    if (handle.is_json) return "response.json";
    if (ct.includes("xml")) return "response.xml";
    if (ct.includes("html")) return "response.html";
    if (ct.includes("csv")) return "response.csv";
    if (handle.is_text) return "response.txt";
    const sub = ct.split("/")[1]?.split(";")[0]?.trim();
    return sub && /^[a-z0-9.+-]+$/.test(sub) ? `response.${sub.replace(/^.*\+/, "")}` : "response.bin";
  }

  async function saveToFile() {
    error = null;
    saved = null;
    try {
      const path = await save({ defaultPath: defaultName() });
      if (!path) return;
      saving = true;
      await api.responseSave(handle.id, path, pretty);
      saved = path;
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
    }
  }
</script>

<div class="bar">
  <span class="size">{formatBytes(handle.size)}</span>
  {#if handle.content_type}<span class="ct mono">{handle.content_type}</span>{/if}
  {#if handle.stored === "file"}<span class="badge" title={t("large.storedFileHint")}>{t("large.storedFile")}</span>{/if}
  {@render children?.()}
  <span class="spacer"></span>
  <button class="small" disabled={saving} onclick={saveToFile}>{saving ? t("large.saving") : t("large.save")}</button>
</div>
{#if !handle.complete}
  <div class="incomplete" role="status">
    <span>{t("large.incomplete", { size: formatBytes(handle.size) })}</span>
    <button class="link" onclick={() => ui.openSettings("requests")}>{t("large.openLimit")}</button>
  </div>
{/if}
{#if saved}<p class="ok">{t("large.saved", { path: saved })}</p>{/if}
{#if error}<p class="err">{error}</p>{/if}

<style>
  .bar {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
    padding: 4px 0 6px;
    font-size: 12px;
  }
  .size {
    font-weight: 600;
  }
  .ct {
    color: var(--muted);
  }
  .mono {
    font-family: var(--mono);
  }
  .badge {
    padding: 1px 7px;
    border-radius: 999px;
    border: 1px solid var(--border);
    background: var(--surface-2);
    color: var(--muted);
    font-size: 11px;
  }
  .spacer {
    flex: 1;
  }
  .small {
    padding: 3px 10px;
    font-size: 12px;
  }
  .incomplete {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
    color: var(--warn);
    border: 1px solid var(--warn);
    background: color-mix(in srgb, var(--warn) 10%, transparent);
    border-radius: 6px;
    padding: 5px 10px;
    font-size: 12px;
    margin-bottom: 6px;
  }
  .link {
    border: none;
    background: none;
    padding: 0;
    color: var(--accent);
    font-size: 12px;
  }
  .link:hover {
    text-decoration: underline;
  }
  .ok,
  .err {
    margin: 0 0 6px;
    font-size: 12px;
    word-break: break-all;
  }
  .ok {
    color: var(--ok);
  }
  .err {
    color: var(--err);
  }
</style>
