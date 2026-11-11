<script lang="ts">
  // Success / error toasts of git operations (fetch, commit, clone…). Mounted once, in the sidebar.
  import { t } from "$lib/i18n/index.svelte";
  import { git } from "$lib/stores/git.svelte";
</script>

{#if git.toasts.length}
  <div class="toasts" aria-live="polite">
    {#each git.toasts as toast (toast.id)}
      <div class="toast {toast.kind}" role={toast.kind === "err" ? "alert" : "status"}>
        <span>{toast.text}</span>
        <button class="ghost" aria-label={t("common.dismiss")} title={t("common.dismiss")} onclick={() => git.dismiss(toast.id)}
          >×</button
        >
      </div>
    {/each}
  </div>
{/if}

<style>
  .toasts {
    position: fixed;
    left: 16px;
    bottom: 16px;
    z-index: 60;
    display: flex;
    flex-direction: column;
    gap: 6px;
    max-width: min(440px, calc(100vw - 32px));
  }
  .toast {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 8px 8px 8px 12px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: var(--shadow);
    font-size: 13px;
    white-space: pre-wrap;
    word-break: break-word;
  }
  .toast.ok {
    border-color: var(--ok);
  }
  .toast.err {
    border-color: var(--err);
    color: var(--err);
  }
  .toast span {
    flex: 1;
    max-height: 160px;
    overflow: auto;
  }
  .toast button {
    padding: 0 6px;
  }
</style>
