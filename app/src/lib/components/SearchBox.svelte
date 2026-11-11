<script lang="ts">
  // Reusable search field: icon, clear button, optional result count and shortcut hint.
  // Esc clears the text (or blurs when already empty) unless the parent's onkeydown prevented it.
  import type { Snippet } from "svelte";
  import { t } from "$lib/i18n/index.svelte";

  interface Props {
    value: string;
    placeholder: string;
    /** Text shown at the right while there is a query, e.g. "12 results". */
    count?: string | null;
    /** Shortcut hint shown while empty and not focused, e.g. "⌘P". */
    shortcut?: string | null;
    oninput?: (value: string) => void;
    onkeydown?: (e: KeyboardEvent) => void;
    /** The <input>, for focusing from outside. */
    input?: HTMLInputElement;
    /** Extra controls after the field (e.g. an options toggle). */
    extra?: Snippet;
  }

  let {
    value = $bindable(),
    placeholder,
    count = null,
    shortcut = null,
    oninput,
    onkeydown,
    input = $bindable(),
    extra,
  }: Props = $props();

  let focused = $state(false);

  function clear() {
    value = "";
    oninput?.("");
    input?.focus();
  }

  function keydown(e: KeyboardEvent) {
    onkeydown?.(e);
    if (e.defaultPrevented) return;
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      if (value) clear();
      else input?.blur();
    }
  }
</script>

<div class="search-box" class:focused>
  <svg class="glass" viewBox="0 0 16 16" width="13" height="13" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.5">
    <circle cx="7" cy="7" r="4.5" /><path d="M10.5 10.5L14 14" />
  </svg>
  <input
    bind:this={input}
    bind:value
    type="text"
    role="searchbox"
    {placeholder}
    aria-label={placeholder}
    spellcheck="false"
    autocomplete="off"
    oninput={() => oninput?.(value)}
    onkeydown={keydown}
    onfocus={() => (focused = true)}
    onblur={() => (focused = false)}
  />
  {#if value && count}
    <span class="count" aria-live="polite">{count}</span>
  {/if}
  {#if value}
    <button type="button" class="clear" title={t("search.clear")} aria-label={t("search.clear")} onclick={clear}>×</button>
  {:else if shortcut && !focused}
    <kbd class="hint" aria-hidden="true">{shortcut}</kbd>
  {/if}
  {@render extra?.()}
</div>

<style>
  .search-box {
    display: flex;
    align-items: center;
    gap: 4px;
    min-width: 0;
    height: 28px;
    padding: 0 4px 0 8px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--surface-2);
  }
  .search-box.focused {
    border-color: var(--accent);
    background: var(--surface);
  }
  .glass {
    flex: none;
    color: var(--muted);
  }
  input {
    flex: 1;
    min-width: 0;
    border: none;
    background: none;
    padding: 2px 2px;
    font-size: 12px;
  }
  input:focus {
    outline: none;
  }
  .count {
    flex: none;
    font-size: 11px;
    color: var(--muted);
    white-space: nowrap;
  }
  .clear {
    flex: none;
    border: none;
    background: none;
    color: var(--muted);
    padding: 0 5px;
    line-height: 18px;
    border-radius: 4px;
  }
  .clear:hover {
    color: var(--text);
    background: var(--hover);
  }
  .hint {
    flex: none;
    font-family: var(--mono);
    font-size: 10px;
    color: var(--muted);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 0 4px;
    line-height: 16px;
  }
</style>
