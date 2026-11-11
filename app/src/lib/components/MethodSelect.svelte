<script lang="ts">
  import { HTTP_METHODS, type HttpMethod } from "$lib/api";
  import { methodColor } from "./methods";

  interface Props {
    value: HttpMethod;
    label: string;
  }

  let { value = $bindable(), label }: Props = $props();

  const uid = $props.id();
  const listId = `${uid}-list`;
  const optionId = (i: number) => `${uid}-opt-${i}`;

  let open = $state(false);
  /** Index of the highlighted option while the list is open. */
  let active = $state(0);
  let root = $state<HTMLDivElement>();
  let button = $state<HTMLButtonElement>();
  let list = $state<HTMLUListElement>();

  function show() {
    active = Math.max(0, HTTP_METHODS.indexOf(value));
    open = true;
  }

  function hide(refocus = true) {
    open = false;
    if (refocus) button?.focus();
  }

  function choose(i: number) {
    value = HTTP_METHODS[i];
    hide();
  }

  /** Index of the next method starting with `letter`, after `from` (wrapping), or -1. */
  function matchLetter(letter: string, from: number): number {
    const l = letter.toUpperCase();
    const n = HTTP_METHODS.length;
    for (let k = 1; k <= n; k++) {
      const i = (from + k) % n;
      if (HTTP_METHODS[i].startsWith(l)) return i;
    }
    return -1;
  }

  const isLetter = (e: KeyboardEvent) => e.key.length === 1 && /\S/.test(e.key) && !e.metaKey && !e.ctrlKey && !e.altKey;

  // Focus the list when it opens and keep the highlighted option visible.
  $effect(() => {
    if (!open || !list) return;
    list.focus();
    list.querySelector(`#${CSS.escape(optionId(active))}`)?.scrollIntoView({ block: "nearest" });
  });

  function onbuttonkeydown(e: KeyboardEvent) {
    if (e.key === "ArrowDown" || e.key === "ArrowUp" || e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      show();
    } else if (isLetter(e)) {
      // Like a native select: typing a letter picks the next matching method directly.
      const i = matchLetter(e.key, HTTP_METHODS.indexOf(value));
      if (i >= 0) {
        e.preventDefault();
        value = HTTP_METHODS[i];
      }
    }
  }

  function onlistkeydown(e: KeyboardEvent) {
    const n = HTTP_METHODS.length;
    if (e.key === "ArrowDown") active = (active + 1) % n;
    else if (e.key === "ArrowUp") active = (active - 1 + n) % n;
    else if (e.key === "Home") active = 0;
    else if (e.key === "End") active = n - 1;
    else if (e.key === "Enter" || e.key === " ") choose(active);
    else if (e.key === "Escape") hide();
    else if (e.key === "Tab") {
      hide(false);
      return;
    } else if (isLetter(e)) {
      const i = matchLetter(e.key, active);
      if (i >= 0) active = i;
    } else return;
    e.preventDefault();
    e.stopPropagation();
  }

  function onwindowpointerdown(e: PointerEvent) {
    if (open && root && !root.contains(e.target as Node)) hide(false);
  }
</script>

<svelte:window onpointerdown={onwindowpointerdown} onblur={() => open && hide(false)} />

<div class="method-select" bind:this={root}>
  <button
    bind:this={button}
    type="button"
    class="trigger mono"
    style:color={methodColor(value)}
    aria-haspopup="listbox"
    aria-expanded={open}
    aria-controls={listId}
    aria-label="{label}: {value}"
    title={label}
    onclick={() => (open ? hide() : show())}
    onkeydown={onbuttonkeydown}
  >
    <span>{value}</span>
    <svg class="caret" viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
      <path d="M4 6l4 4 4-4" fill="none" stroke="currentColor" stroke-width="1.6" />
    </svg>
  </button>

  {#if open}
    <ul
      bind:this={list}
      id={listId}
      class="list"
      role="listbox"
      tabindex="-1"
      aria-label={label}
      aria-activedescendant={optionId(active)}
      onkeydown={onlistkeydown}
    >
      {#each HTTP_METHODS as m, i (m)}
        <!-- Keyboard interaction is handled on the listbox (aria-activedescendant pattern). -->
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <li
          id={optionId(i)}
          role="option"
          aria-selected={m === value}
          class:active={i === active}
          class:current={m === value}
          style:--m={methodColor(m)}
          onclick={() => choose(i)}
          onpointermove={() => (active = i)}
        >
          <span class="chip mono">{m}</span>
          {#if m === value}
            <svg class="check" viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
              <path d="M3.5 8.5l3 3 6-7" fill="none" stroke="currentColor" stroke-width="1.8" />
            </svg>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .method-select {
    position: relative;
    flex: none;
    width: 112px;
  }
  .trigger {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 6px;
    font-weight: 700;
  }
  .trigger:focus-visible,
  .trigger[aria-expanded="true"] {
    outline: none;
    border-color: var(--accent);
  }
  .caret {
    flex: none;
    color: var(--muted);
  }
  .list {
    position: absolute;
    z-index: 40;
    top: calc(100% + 4px);
    left: 0;
    min-width: 100%;
    margin: 0;
    padding: 4px;
    list-style: none;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: var(--shadow);
    max-height: 320px;
    overflow: auto;
  }
  .list:focus {
    outline: none;
  }
  li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 4px 6px;
    border-radius: 5px;
    cursor: pointer;
    color: var(--m);
  }
  li.active {
    background: var(--hover);
  }
  .chip {
    font-size: 12px;
    font-weight: 700;
    padding: 2px 8px;
    border-radius: 4px;
    background: color-mix(in srgb, var(--m) 14%, transparent);
  }
  .check {
    flex: none;
  }
</style>
