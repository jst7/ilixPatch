<script lang="ts">
  // "A new version is available" (D54). Mounted once, in the sidebar; runs the daily startup check.
  import { onMount } from "svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { t } from "$lib/i18n/index.svelte";
  import { updates } from "$lib/stores/update.svelte";

  onMount(() => {
    void updates.autoCheck();
  });

  async function download(url: string) {
    try {
      await openUrl(url);
    } catch {
      window.open(url, "_blank", "noopener");
    }
    updates.hidden = true;
  }
</script>

{#if updates.notice}
  {@const latest = updates.notice}
  <div class="update" role="status" aria-live="polite">
    <div class="text">
      <strong>{t("update.available", { version: latest.version })}</strong>
      <span class="muted">{t("update.current", { version: updates.result?.current ?? "?" })}</span>
    </div>
    <div class="buttons">
      <button class="primary" onclick={() => download(latest.url)}>{t("update.download")}</button>
      <button onclick={() => updates.skip(latest.version)}>{t("update.skip")}</button>
    </div>
    <button
      class="ghost x"
      aria-label={t("common.dismiss")}
      title={t("update.later")}
      onclick={() => (updates.hidden = true)}>×</button
    >
  </div>
{/if}

<style>
  .update {
    position: fixed;
    right: 16px;
    bottom: 16px;
    z-index: 60;
    display: flex;
    align-items: flex-start;
    gap: 12px;
    padding: 10px 8px 10px 14px;
    max-width: min(440px, calc(100vw - 32px));
    background: var(--surface);
    border: 1px solid var(--accent);
    border-radius: 8px;
    box-shadow: var(--shadow);
    font-size: 13px;
  }
  .text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .muted {
    color: var(--muted);
    font-size: 12px;
  }
  .buttons {
    display: flex;
    gap: 6px;
    align-items: center;
    margin-left: auto;
  }
  .x {
    padding: 0 6px;
  }
</style>
