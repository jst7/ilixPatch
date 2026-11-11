<script lang="ts">
  // Narrow column between two editor panes: coloured ribbons connecting corresponding chunks,
  // plus optional action buttons (apply / ignore) next to them.
  import type { Ribbon, StripAction } from "$lib/merge/panes";

  let {
    ribbons,
    actions = [],
    el = $bindable(),
  }: { ribbons: Ribbon[]; actions?: StripAction[]; el?: HTMLDivElement } = $props();

  let width = $state(40);

  function path(r: Ribbon): string {
    const w = width;
    const m = w / 2;
    return `M0,${r.lt} C${m},${r.lt} ${m},${r.rt} ${w},${r.rt} L${w},${r.rb} C${m},${r.rb} ${m},${r.lb} 0,${r.lb} Z`;
  }
</script>

<div class="strip" bind:this={el} bind:clientWidth={width}>
  <svg aria-hidden="true">
    {#each ribbons as r (r.key)}
      <path class="ribbon {r.tone}" d={path(r)} />
    {/each}
  </svg>
  {#each actions as a (a.key)}
    <button
      class="act {a.kind} {a.side}"
      style:top="{a.y}px"
      title={a.title}
      aria-label={a.title}
      onclick={a.run}
      onmousedown={(e) => e.preventDefault()}>{a.label}</button
    >
  {/each}
</div>

<style>
  .strip {
    position: relative;
    overflow: hidden;
    min-width: 0;
    background: var(--surface-2);
    border-left: 1px solid var(--border);
    border-right: 1px solid var(--border);
  }
  svg {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    overflow: visible;
  }
  .ribbon {
    stroke-width: 1;
  }
  .ribbon.changed {
    fill: var(--merge-changed-bg);
    stroke: var(--git-modified);
  }
  .ribbon.added {
    fill: var(--merge-added-bg);
    stroke: var(--git-added);
  }
  .ribbon.removed {
    fill: var(--merge-removed-bg);
    stroke: var(--git-deleted);
  }
  .ribbon.conflict {
    fill: var(--merge-conflict-bg);
    stroke: var(--git-conflicted);
  }
  .ribbon.resolved {
    fill: var(--merge-resolved-bg);
    stroke: var(--border);
  }
  .act {
    position: absolute;
    width: 16px;
    height: 16px;
    padding: 0;
    margin: 0;
    display: grid;
    place-items: center;
    font-size: 12px;
    line-height: 1;
    border: 1px solid var(--border);
    border-radius: 3px;
    background: var(--surface);
    color: var(--text);
    cursor: pointer;
    z-index: 1;
  }
  .act.left {
    left: 2px;
  }
  .act.right {
    right: 2px;
  }
  .act.left.ignore {
    left: 20px;
  }
  .act.right.ignore {
    right: 20px;
  }
  .act.apply:hover {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--accent-text);
  }
  .act.ignore {
    color: var(--muted);
  }
  .act.ignore:hover {
    color: var(--err);
    border-color: var(--err);
  }
</style>
