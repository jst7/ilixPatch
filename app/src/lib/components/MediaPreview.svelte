<script lang="ts">
  // Rendered preview of a response body (D44): image, PDF, audio, video or HTML.
  // The body is read from the Rust ResponseStore in chunks into a Blob URL.
  import { api, type BodyHandle } from "$lib/api";
  import { t } from "$lib/i18n/index.svelte";
  import { base64ToBytes, formatBytes } from "$lib/largebody/format";
  import type { PreviewType } from "$lib/preview/kind";

  interface Props {
    handle: BodyHandle;
    type: PreviewType;
  }

  let { handle, type }: Props = $props();

  /** Larger bodies are not loaded into the webview; "Open with system app" still works. */
  const MAX_PREVIEW = 150 * 1024 * 1024;
  const CHUNK = 4 * 1024 * 1024;

  let url = $state<string | null>(null);
  let loaded = $state(0);
  let error = $state<string | null>(null);
  let openError = $state<string | null>(null);
  let broken = $state(false);
  let fit = $state(true);
  let dims = $state<{ w: number; h: number } | null>(null);
  const tooBig = $derived(handle.size > MAX_PREVIEW);

  $effect(() => {
    const id = handle.id;
    const mime = type.mime;
    if (handle.size > MAX_PREVIEW) return;
    let cancelled = false;
    let objectUrl: string | null = null;
    url = null;
    loaded = 0;
    error = null;
    broken = false;
    dims = null;
    (async () => {
      const parts: BlobPart[] = [];
      let offset = 0;
      for (;;) {
        const c = await api.responseRead(id, offset, CHUNK);
        if (cancelled) return;
        const bytes = c.encoding === "base64" ? base64ToBytes(c.data) : new TextEncoder().encode(c.data);
        parts.push(bytes as BlobPart);
        offset += c.length;
        loaded = offset;
        if (c.eof || c.length === 0) break;
      }
      objectUrl = URL.createObjectURL(new Blob(parts, { type: mime }));
      url = objectUrl;
    })().catch((e) => {
      if (!cancelled) error = String(e);
    });
    return () => {
      cancelled = true;
      if (objectUrl) URL.revokeObjectURL(objectUrl);
    };
  });

  async function openExternal() {
    openError = null;
    try {
      await api.responseOpen(handle.id, type.ext);
    } catch (e) {
      openError = String(e);
    }
  }
</script>

<div class="preview">
  <div class="bar">
    <span class="what">{type.mime}</span>
    {#if dims}<span class="muted">{dims.w} × {dims.h}</span>{/if}
    <span class="muted">{formatBytes(handle.size)}</span>
    {#if type.kind === "image" && url && !broken}
      <button class="ghost small" onclick={() => (fit = !fit)}>{fit ? t("preview.actualSize") : t("preview.fit")}</button>
    {/if}
    <button class="ghost small push" onclick={openExternal}>{t("preview.openExternal")}</button>
  </div>
  {#if !handle.complete}
    <p class="note warn">{t("preview.incomplete")}</p>
  {/if}
  {#if openError}<p class="note err">{openError}</p>{/if}

  <div class="stage" class:checker={type.kind === "image"}>
    {#if tooBig}
      <p class="note">{t("preview.tooBig", { size: formatBytes(MAX_PREVIEW) })}</p>
    {:else if error}
      <p class="note err">{error}</p>
    {:else if !url}
      <p class="note">{t("preview.loading", { done: formatBytes(loaded), total: formatBytes(handle.size) })}</p>
    {:else if broken}
      <p class="note">{t("preview.cannotRender")}</p>
    {:else if type.kind === "image"}
      <img
        src={url}
        alt=""
        class:fit
        onload={(e) => {
          const img = e.currentTarget as HTMLImageElement;
          dims = { w: img.naturalWidth, h: img.naturalHeight };
        }}
        onerror={() => (broken = true)}
      />
    {:else if type.kind === "pdf"}
      <iframe src={url} title="PDF"></iframe>
    {:else if type.kind === "audio"}
      <audio src={url} controls onerror={() => (broken = true)}></audio>
    {:else if type.kind === "video"}
      <!-- svelte-ignore a11y_media_has_caption -->
      <video src={url} controls onerror={() => (broken = true)}></video>
    {:else}
      <!-- HTML: sandboxed without scripts, forms or same-origin access. -->
      <iframe src={url} title="HTML" sandbox="" class="html"></iframe>
    {/if}
  </div>
</div>

<style>
  .preview {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 4px 10px;
    border-bottom: 1px solid var(--border);
    font-size: 12px;
    min-height: 28px;
  }
  .what {
    font-family: var(--mono);
  }
  .muted {
    color: var(--muted);
  }
  .push {
    margin-left: auto;
  }
  .note {
    margin: 10px;
    font-size: 12px;
    color: var(--muted);
  }
  .note.warn {
    color: var(--warn);
  }
  .note.err {
    color: var(--err);
  }
  .stage {
    flex: 1;
    min-height: 0;
    overflow: auto;
    display: flex;
    align-items: flex-start;
    justify-content: center;
  }
  .stage.checker {
    align-items: center;
    background-color: var(--surface);
    background-image:
      linear-gradient(45deg, var(--surface-2) 25%, transparent 25%),
      linear-gradient(-45deg, var(--surface-2) 25%, transparent 25%),
      linear-gradient(45deg, transparent 75%, var(--surface-2) 75%),
      linear-gradient(-45deg, transparent 75%, var(--surface-2) 75%);
    background-size: 16px 16px;
    background-position:
      0 0,
      0 8px,
      8px -8px,
      -8px 0;
  }
  img {
    image-rendering: auto;
  }
  img.fit {
    max-width: 100%;
    max-height: 100%;
    object-fit: contain;
  }
  iframe {
    flex: 1;
    align-self: stretch;
    border: none;
    min-height: 0;
  }
  iframe.html {
    background: #fff;
  }
  audio {
    margin: 24px;
    width: min(520px, 90%);
  }
  video {
    max-width: 100%;
    max-height: 100%;
  }
</style>
