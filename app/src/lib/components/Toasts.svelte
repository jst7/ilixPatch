<script lang="ts">
  // D65: stacked notifications, bottom right.
  import { t } from "$lib/i18n/index.svelte";
  import { toasts, type ToastKind } from "$lib/stores/toasts.svelte";

  const ICON: Record<ToastKind, string> = {
    success: "M20 6L9 17l-5-5",
    info: "M12 16v-4M12 8h.01M12 3a9 9 0 100 18 9 9 0 000-18z",
    warn: "M12 9v4M12 17h.01M10.3 3.9L2 18a2 2 0 001.7 3h16.6a2 2 0 001.7-3L13.7 3.9a2 2 0 00-3.4 0z",
    error: "M12 8v4M12 16h.01M12 3a9 9 0 100 18 9 9 0 000-18z",
  };
</script>

<div class="toasts" aria-live="polite">
  {#each toasts.list as toast (toast.id)}
    <div class="toast {toast.kind}" role={toast.kind === "error" ? "alert" : "status"}>
      <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d={ICON[toast.kind]} /></svg>
      <span class="text">{toast.text}</span>
      {#if toast.action}
        {@const a = toast.action}
        <button class="small" onclick={() => (a.run(), toasts.dismiss(toast.id))}>{a.label}</button>
      {/if}
      <button class="ghost x" aria-label={t("common.dismiss")} title={t("common.dismiss")} onclick={() => toasts.dismiss(toast.id)}>×</button>
    </div>
  {/each}
</div>

<style>
  .toasts {
    position: fixed;
    right: 16px;
    bottom: 40px;
    z-index: 70;
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 6px;
    pointer-events: none;
  }
  .toast {
    pointer-events: auto;
    display: flex;
    align-items: flex-start;
    gap: 8px;
    max-width: min(440px, calc(100vw - 32px));
    padding: 8px 6px 8px 10px;
    border: 1px solid var(--border);
    border-left: 3px solid var(--accent);
    border-radius: 8px;
    background: var(--surface);
    box-shadow: var(--shadow);
    font-size: var(--fs-sm);
    animation: in 0.15s ease-out;
    word-break: break-word;
  }
  @keyframes in {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
  }
  .toast svg {
    flex: none;
    margin-top: 1px;
  }
  .success {
    border-left-color: var(--ok);
  }
  .success svg {
    color: var(--ok);
  }
  .info svg {
    color: var(--accent);
  }
  .warn {
    border-left-color: var(--warn);
  }
  .warn svg {
    color: var(--warn);
  }
  .error {
    border-left-color: var(--err);
  }
  .error svg {
    color: var(--err);
  }
  .text {
    flex: 1;
    padding-top: 1px;
  }
  .x {
    padding: 0 6px;
    line-height: 1.2;
  }
</style>
