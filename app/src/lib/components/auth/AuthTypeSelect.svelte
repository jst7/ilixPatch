<script lang="ts">
  // Themed, grouped dropdown of authorization types (D39).
  import type { AuthType } from "$lib/api";
  import { AUTH_GROUPS } from "$lib/auth";
  import { t, type MessageKey } from "$lib/i18n/index.svelte";

  interface Props {
    value: AuthType;
    /** Types to hide (e.g. "inherit" for collections). */
    exclude?: AuthType[];
    onchange: (type: AuthType) => void;
  }

  let { value, exclude = [], onchange }: Props = $props();

  let open = $state(false);
  let root: HTMLDivElement | undefined = $state();
  let list: HTMLDivElement | undefined = $state();

  const groups = $derived(
    AUTH_GROUPS.map((g) => ({ ...g, types: g.types.filter((x) => !exclude.includes(x)) })).filter((g) => g.types.length),
  );

  const label = (type: AuthType) => t(`auth.type.${type}` as MessageKey);

  function pick(type: AuthType) {
    open = false;
    if (type !== value) onchange(type);
  }

  function toggle() {
    open = !open;
    if (open) queueMicrotask(() => list?.querySelector<HTMLButtonElement>("[aria-selected=true]")?.focus());
  }

  function onListKey(e: KeyboardEvent) {
    const items = [...(list?.querySelectorAll<HTMLButtonElement>("[role=option]") ?? [])];
    const i = items.indexOf(document.activeElement as HTMLButtonElement);
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const n = e.key === "ArrowDown" ? Math.min(i + 1, items.length - 1) : Math.max(i - 1, 0);
      items[n]?.focus();
    } else if (e.key === "Escape") {
      e.preventDefault();
      open = false;
      root?.querySelector<HTMLButtonElement>(".trigger")?.focus();
    } else if (e.key === "Tab") {
      open = false;
    }
  }

  function onWindowPointer(e: PointerEvent) {
    if (open && root && !root.contains(e.target as Node)) open = false;
  }
</script>

<svelte:window onpointerdown={onWindowPointer} />

<div class="ats" bind:this={root}>
  <button class="trigger" aria-haspopup="listbox" aria-expanded={open} aria-label={t("auth.typeLabel")} onclick={toggle}>
    <span>{label(value)}</span>
    <svg viewBox="0 0 12 12" width="10" height="10" aria-hidden="true"><path d="M2.5 4.5L6 8l3.5-3.5" fill="none" stroke="currentColor" stroke-width="1.5" /></svg>
  </button>
  {#if open}
    <!-- svelte-ignore a11y_interactive_supports_focus -->
    <div class="list" role="listbox" aria-label={t("auth.typeLabel")} bind:this={list} onkeydown={onListKey}>
      {#each groups as g (g.id)}
        <div class="group" role="presentation">{t(`auth.group.${g.id}` as MessageKey)}</div>
        {#each g.types as type (type)}
          <button role="option" aria-selected={type === value} class:sel={type === value} onclick={() => pick(type)}>
            {label(type)}
          </button>
        {/each}
      {/each}
    </div>
  {/if}
</div>

<style>
  .ats {
    position: relative;
    display: inline-block;
  }
  .trigger {
    display: inline-flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    min-width: 220px;
    font-size: 13px;
    padding: 5px 10px;
  }
  .trigger svg {
    color: var(--muted);
  }
  .list {
    position: absolute;
    z-index: 50;
    top: calc(100% + 4px);
    left: 0;
    min-width: 100%;
    max-height: 360px;
    overflow: auto;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: var(--shadow);
    padding: 4px;
    display: flex;
    flex-direction: column;
  }
  .group {
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
    padding: 6px 8px 2px;
  }
  .group:not(:first-child) {
    border-top: 1px solid var(--border);
    margin-top: 4px;
  }
  [role="option"] {
    text-align: left;
    border: none;
    background: none;
    font-size: 13px;
    padding: 5px 10px;
    border-radius: 5px;
    white-space: nowrap;
  }
  [role="option"]:hover,
  [role="option"]:focus {
    background: var(--hover);
    outline: none;
  }
  [role="option"].sel {
    background: var(--selection);
    font-weight: 600;
  }
</style>
