<script lang="ts">
  // Binary bodies (D41): size, content type, hex preview of the first 4 KB and Save to file.
  import { api, type BodyHandle } from "$lib/api";
  import { t } from "$lib/i18n/index.svelte";
  import { base64ToBytes, hexDump } from "$lib/largebody/format";
  import BodyInfoBar from "./BodyInfoBar.svelte";

  interface Props {
    handle: BodyHandle;
  }

  let { handle }: Props = $props();

  const PREVIEW = 4096;
  let lines = $state<string[] | null>(null);
  let error = $state<string | null>(null);

  $effect(() => {
    const id = handle.id;
    lines = null;
    error = null;
    api
      .responseRead(id, 0, PREVIEW)
      .then((chunk) => {
        const bytes = chunk.encoding === "base64" ? base64ToBytes(chunk.data) : new TextEncoder().encode(chunk.data);
        lines = hexDump(bytes, chunk.offset);
      })
      .catch((e) => (error = String(e)));
  });
</script>

<div class="binary">
  <BodyInfoBar {handle} />
  <p class="hint">{t("large.binaryHint", { n: Math.min(PREVIEW, handle.size) })}</p>
  {#if error}
    <p class="err">{error}</p>
  {:else if !lines}
    <p class="hint">{t("common.loading")}</p>
  {:else}
    <pre class="hex">{lines.join("\n")}</pre>
  {/if}
</div>

<style>
  .binary {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .hint {
    color: var(--muted);
    font-size: 12px;
    margin: 0 0 6px;
  }
  .err {
    color: var(--err);
    font-size: 12px;
  }
  .hex {
    flex: 1;
    min-height: 0;
    overflow: auto;
    margin: 0;
    padding: 8px 10px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--surface);
    font-family: var(--mono);
    font-size: 12px;
    line-height: 1.5;
    color: var(--text);
  }
</style>
