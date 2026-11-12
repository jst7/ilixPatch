<script lang="ts">
  // Masked input with an eye toggle; optional warning icon (literal secret in a git project).
  import { t } from "$lib/i18n/index.svelte";

  interface Props {
    value: string;
    oninput: (v: string) => void;
    placeholder?: string;
    /** Tooltip of a warning icon; no icon when null. */
    warning?: string | null;
    label?: string;
  }

  let { value, oninput, placeholder = "", warning = null, label }: Props = $props();
  let shown = $state(false);
</script>

<span class="secret">
  <input
    class="mono"
    type={shown ? "text" : "password"}
    {value}
    {placeholder}
    aria-label={label}
    autocomplete="off"
    spellcheck="false"
    oninput={(e) => oninput(e.currentTarget.value)}
  />
  {#if warning}
    <span class="warn" title={warning} aria-label={warning} role="img">
      <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true"
        ><path d="M8 1.8l6.6 11.7H1.4z" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round" /><path
          d="M8 6.2v3.4M8 11.6h.01"
          stroke="currentColor"
          stroke-width="1.5"
          stroke-linecap="round"
        /></svg
      >
    </span>
  {/if}
  <button
    type="button"
    class="eye"
    title={shown ? t("auth.hide") : t("auth.show")}
    aria-label={shown ? t("auth.hide") : t("auth.show")}
    aria-pressed={shown}
    onclick={() => (shown = !shown)}
  >
    <svg viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.3" aria-hidden="true">
      <path d="M1.5 8S4 3.5 8 3.5 14.5 8 14.5 8 12 12.5 8 12.5 1.5 8 1.5 8z" /><circle cx="8" cy="8" r="2" />
      {#if !shown}<path d="M2.5 13.5l11-11" />{/if}
    </svg>
  </button>
</span>

<style>
  .secret {
    display: flex;
    align-items: center;
    gap: 4px;
    min-width: 0;
  }
  input {
    flex: 1;
    min-width: 0;
    font-size: 13px;
    padding: 5px 8px;
  }
  .eye {
    flex: none;
    border: none;
    background: none;
    color: var(--muted);
    padding: 3px 5px;
  }
  .eye:hover {
    color: var(--text);
  }
  .warn {
    flex: none;
    color: var(--warn);
    display: inline-flex;
  }
</style>
