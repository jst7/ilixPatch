<script lang="ts">
  // D60: the secrets use the default master password ("pass"): recommend changing it.
  import { t } from "$lib/i18n/index.svelte";
  import { vault } from "$lib/stores/vault.svelte";
  import { ui } from "$lib/stores/ui.svelte";
</script>

{#if vault.defaultPassword && !vault.noticeHidden}
  <div class="notice" role="status">
    <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M6 11V8a6 6 0 0112 0v3M5 11h14v10H5zM12 15v2" /></svg>
    <span>{t("vault.defaultNotice")}</span>
    <button class="primary" onclick={() => ui.openSettings("security")}>{t("vault.change")}</button>
    <button class="ghost" title={t("vault.later")} aria-label={t("vault.later")} onclick={() => (vault.noticeHidden = true)}>×</button>
  </div>
{/if}

<style>
  .notice {
    position: fixed;
    left: 16px;
    bottom: 16px;
    z-index: 60;
    display: flex;
    align-items: center;
    gap: 8px;
    max-width: min(520px, calc(100vw - 32px));
    padding: 8px 8px 8px 12px;
    border: 1px solid var(--border);
    border-left: 3px solid var(--warn, var(--accent));
    border-radius: 8px;
    background: var(--surface);
    box-shadow: var(--shadow);
    font-size: var(--fs-sm);
  }
  svg {
    flex: none;
    color: var(--warn, var(--accent));
  }
  span {
    flex: 1;
  }
</style>
