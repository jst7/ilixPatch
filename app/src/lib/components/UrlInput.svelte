<script lang="ts">
  // URL bar (D43): a textarea with transparent text over a coloured copy of the URL.
  // Hovering a {{variable}} opens VarPopover to see where it comes from and change it.
  import type { Collection } from "$lib/api";
  import { tokenizeUrl } from "$lib/url/tokens";
  import { varStatus } from "$lib/vars/edit";
  import VarPopover from "./VarPopover.svelte";

  interface Props {
    value: string;
    collection: Collection | null;
    placeholder?: string;
    ariaLabel?: string;
    onpaste?: (e: ClipboardEvent & { currentTarget: HTMLTextAreaElement }) => void;
    onkeydown?: (e: KeyboardEvent & { currentTarget: HTMLTextAreaElement }) => void;
    oninput?: (e: Event & { currentTarget: HTMLTextAreaElement }) => void;
  }

  let { value = $bindable(), collection, placeholder, ariaLabel, onpaste, onkeydown, oninput }: Props = $props();

  let textarea = $state<HTMLTextAreaElement>();
  let backdrop = $state<HTMLDivElement>();

  const tokens = $derived(tokenizeUrl(value ?? ""));

  /** Svelte action: grow the textarea with its content (re-measured when `value` changes). */
  function autogrow(node: HTMLTextAreaElement, _value: string) {
    const resize = () => {
      node.style.height = "auto";
      node.style.height = `${node.scrollHeight + 2}px`;
    };
    resize();
    const ro = new ResizeObserver(resize); // re-wrap when the bar width changes
    ro.observe(node);
    return {
      update: () => queueMicrotask(resize),
      destroy: () => ro.disconnect(),
    };
  }

  function syncScroll() {
    if (backdrop && textarea) backdrop.scrollTop = textarea.scrollTop;
  }

  // ----- hover a variable -----

  let hover = $state<{ name: string; start: number; rect: DOMRect } | null>(null);
  let openTimer: ReturnType<typeof setTimeout> | undefined;
  let closeTimer: ReturnType<typeof setTimeout> | undefined;
  /** The popover has focus (editing): only Esc, Cancel, Save or a click outside close it. */
  let pinned = $state(false);

  /** The variable span under the pointer (the textarea is on top, so hit-test the backdrop). */
  function varAt(x: number, y: number): { name: string; start: number; rect: DOMRect } | null {
    if (!backdrop) return null;
    for (const el of backdrop.querySelectorAll<HTMLElement>("[data-var]")) {
      for (const r of el.getClientRects()) {
        if (x >= r.left && x <= r.right && y >= r.top && y <= r.bottom) {
          return { name: el.dataset.var ?? "", start: Number(el.dataset.start), rect: r };
        }
      }
    }
    return null;
  }

  function onmove(e: MouseEvent) {
    if (pinned) return;
    const v = varAt(e.clientX, e.clientY);
    if (v && hover && v.start === hover.start) {
      clearTimeout(closeTimer);
      return;
    }
    clearTimeout(openTimer);
    if (v) {
      clearTimeout(closeTimer);
      openTimer = setTimeout(() => (hover = v), hover ? 0 : 350);
    } else if (hover) {
      scheduleClose();
    }
  }

  function scheduleClose() {
    clearTimeout(openTimer);
    clearTimeout(closeTimer);
    if (!pinned) closeTimer = setTimeout(() => (hover = null), 250);
  }

  function close() {
    pinned = false;
    hover = null;
  }
</script>

<div class="urlbox" role="presentation" onmousemove={onmove} onmouseleave={scheduleClose}>
  <div class="backdrop mono" bind:this={backdrop} aria-hidden="true">
    {#each tokens as tok (tok.start)}{#if tok.kind === "var"}<span
          class="tok var {varStatus(collection, tok.name ?? '')}"
          class:hovered={hover?.start === tok.start}
          data-var={tok.name}
          data-start={tok.start}>{tok.text}</span
        >{:else}<span class="tok {tok.kind}">{tok.text}</span>{/if}{/each}{" "}
  </div>
  <textarea
    bind:this={textarea}
    class="url mono"
    rows="1"
    bind:value
    use:autogrow={value}
    {placeholder}
    aria-label={ariaLabel}
    spellcheck="false"
    autocomplete="off"
    onscroll={syncScroll}
    {onpaste}
    onkeydown={(e) => {
      if (hover && e.key === "Escape") close();
      onkeydown?.(e);
    }}
    oninput={(e) => {
      oninput?.(e);
      queueMicrotask(syncScroll);
    }}
  ></textarea>
</div>

{#if hover}
  <VarPopover
    name={hover.name}
    anchor={hover.rect}
    {collection}
    onenter={() => clearTimeout(closeTimer)}
    onleave={scheduleClose}
    onpin={(p) => (pinned = p)}
    onclose={close}
  />
{/if}

<style>
  .urlbox {
    position: relative;
    flex: 1;
    min-width: 0;
    display: flex;
    background: var(--surface);
    border-radius: 6px;
  }
  .url,
  .backdrop {
    box-sizing: border-box;
    margin: 0;
    border: 1px solid transparent;
    padding: 6px 10px;
    line-height: 1.45;
    white-space: pre-wrap;
    word-break: break-all;
    overflow-wrap: anywhere;
    font-family: ui-monospace, monospace;
    font-size: 13px;
    letter-spacing: normal;
    scrollbar-gutter: stable;
  }
  .url {
    position: relative;
    flex: 1;
    width: 100%;
    min-width: 0;
    resize: none;
    overflow-y: auto;
    max-height: calc(6 * 1.45em + 18px);
    border-color: var(--border);
    background: transparent;
    color: transparent;
    caret-color: var(--text);
  }
  .url::placeholder {
    color: var(--muted);
  }
  .url::selection {
    color: transparent;
    background: var(--selection);
  }
  .url:focus {
    border-color: var(--accent);
  }
  .backdrop {
    position: absolute;
    inset: 0;
    overflow: hidden;
    pointer-events: none;
    color: var(--text);
  }
  .tok.scheme {
    color: var(--muted);
  }
  .tok.host {
    color: var(--syntax-function);
  }
  .tok.port {
    color: var(--syntax-number);
  }
  .tok.path {
    color: var(--text);
  }
  .tok.param {
    color: var(--syntax-keyword);
  }
  .tok.punct {
    color: var(--syntax-punct);
  }
  .tok.qkey {
    color: var(--syntax-key);
  }
  .tok.qval {
    color: var(--syntax-string);
  }
  .tok.fragment {
    color: var(--syntax-comment);
  }
  /* Variables: no padding/border so the text stays aligned with the textarea. */
  .tok.var {
    border-radius: 3px;
  }
  .tok.var.ok {
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 14%, transparent);
  }
  .tok.var.dynamic {
    color: var(--syntax-keyword);
    background: color-mix(in srgb, var(--syntax-keyword) 14%, transparent);
  }
  .tok.var.missing {
    color: var(--err);
    background: color-mix(in srgb, var(--err) 12%, transparent);
    text-decoration: underline wavy var(--err);
    text-underline-offset: 3px;
  }
  .tok.var.hovered {
    background: color-mix(in srgb, currentColor 26%, transparent);
  }
</style>
