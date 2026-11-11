<script lang="ts">
  import { ws } from "$lib/stores/workspace.svelte";

  let el = $state<HTMLDivElement>();
  let pos = $state({ x: 0, y: 0 });

  // Keep the menu inside the viewport.
  $effect(() => {
    const menu = ws.menu;
    if (!menu || !el) return;
    const r = el.getBoundingClientRect();
    pos = {
      x: Math.max(4, Math.min(menu.x, window.innerWidth - r.width - 4)),
      y: Math.max(4, Math.min(menu.y, window.innerHeight - r.height - 4)),
    };
  });

  function close() {
    ws.menu = null;
  }

  function onwindowkeydown(e: KeyboardEvent) {
    if (ws.menu && e.key === "Escape") close();
  }
</script>

<svelte:window onkeydown={onwindowkeydown} onblur={close} onresize={close} />

{#if ws.menu}
  <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
  <div class="backdrop" onmousedown={close} oncontextmenu={(e) => { e.preventDefault(); close(); }}></div>
  <div class="menu" role="menu" bind:this={el} style:left="{pos.x}px" style:top="{pos.y}px">
    {#each ws.menu.items as item (item.label)}
      <button
        role="menuitem"
        class:danger={item.danger}
        onclick={() => {
          close();
          item.action();
        }}>{item.label}</button
      >
    {/each}
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
  }
  .menu {
    position: fixed;
    z-index: 51;
    min-width: 160px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 4px;
    box-shadow: var(--shadow);
    display: flex;
    flex-direction: column;
  }
  button {
    border: none;
    background: none;
    text-align: left;
    padding: 6px 10px;
    border-radius: 5px;
  }
  button:hover {
    background: var(--hover);
  }
  button.danger {
    color: var(--err);
  }
</style>
