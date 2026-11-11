<script lang="ts">
  // A real node of the destination project (project root, collection or folder): a drop target, with the
  // planned ghost nodes placed into it listed first.
  import type { Collection, Folder } from "$lib/api";
  import { t } from "$lib/i18n/index.svelte";
  import { importer } from "$lib/stores/import.svelte";
  import type { DestKind } from "$lib/import/plan";
  import { canDropOn, dnd } from "$lib/import/ui.svelte";
  import { methodColor, methodLabel } from "../methods";
  import DestNode from "./DestNode.svelte";
  import GhostNode from "./GhostNode.svelte";

  interface Props {
    kind: DestKind;
    path: string;
    name: string;
    depth: number;
    /** Collections (project root) or the folder's contents. */
    collections?: Collection[];
    folder?: Folder | null;
    placed: Set<string>;
  }

  let { kind, path, name, depth, collections = [], folder = null, placed }: Props = $props();

  const key = $derived(`dest:${path}`);
  const expanded = $derived(importer.expanded[key] ?? kind === "project");
  const ghosts = $derived(importer.placements.filter((p) => p.target_parent === path && placed.has(p.source_id)));
  const dragging = $derived(!!dnd.drag);
  const valid = $derived(dragging && canDropOn(kind));
  const hover = $derived(dnd.drag?.target?.path === path);
</script>

<div
  class="row {kind}"
  class:drop-ok={valid}
  class:drop-no={dragging && !valid}
  class:hover={hover && valid}
  data-drop={path}
  data-kind={kind}
  role="treeitem"
  aria-selected="false"
  aria-expanded={expanded}
  tabindex="-1"
  style:padding-left="{6 + depth * 14}px"
  title={dragging && !valid ? t("import.dest.invalid") : path}
  onclick={() => (importer.expanded[key] = !expanded)}
  onkeydown={(e) => {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      importer.expanded[key] = !expanded;
    }
  }}
>
  <span class="chev" class:open={expanded}>
    <svg viewBox="0 0 16 16" width="11" height="11"><path d="M6 4l4 4-4 4" fill="none" stroke="currentColor" stroke-width="1.6" /></svg>
  </span>
  <span class="kind" aria-hidden="true">
    {#if kind === "project"}
      <svg viewBox="0 0 16 16" width="13" height="13"><rect x="2" y="2.5" width="12" height="11" rx="2" fill="none" stroke="currentColor" stroke-width="1.3" /><path d="M2 6h12" stroke="currentColor" stroke-width="1.3" /></svg>
    {:else if kind === "collection"}
      <svg viewBox="0 0 16 16" width="13" height="13"><path d="M3 3h10v3H3zM3 7.5h10v3H3zM3 12h10" fill="none" stroke="currentColor" stroke-width="1.3" /></svg>
    {:else}
      <svg viewBox="0 0 16 16" width="13" height="13"><path d="M1.5 4.5a1 1 0 011-1h3.5l1.5 1.5h6a1 1 0 011 1v6.5a1 1 0 01-1 1h-11a1 1 0 01-1-1z" fill="none" stroke="currentColor" stroke-width="1.3" /></svg>
    {/if}
  </span>
  <span class="name">{name}</span>
  {#if ghosts.length && !expanded}<span class="count">+{ghosts.length}</span>{/if}
</div>

{#if expanded}
  {#each ghosts as g (g.source_id)}
    {@const src = importer.index.get(g.source_id)?.node}
    {#if src}<GhostNode node={src} as={g.as} placement={g} depth={depth + 1} {placed} />{/if}
  {/each}
  {#if kind === "project"}
    {#each collections as c (c.path)}
      <DestNode kind="collection" path={c.path} name={c.name} folder={c.root} depth={depth + 1} {placed} />
    {/each}
  {:else if folder}
    {#each folder.folders as sub (sub.path)}
      <DestNode kind="folder" path={sub.path} name={sub.name} folder={sub} depth={depth + 1} {placed} />
    {/each}
    {#each folder.requests as r (r.path)}
      <div class="req" style:padding-left="{6 + (depth + 1) * 14 + 18}px">
        <span class="badge mono" style:color={methodColor(r.request.method)}>{methodLabel(r.request.method)}</span>
        <span class="name">{r.request.name}</span>
      </div>
    {/each}
  {/if}
{/if}

<style>
  .row {
    display: flex;
    align-items: center;
    gap: 5px;
    height: 26px;
    padding-right: 6px;
    margin: 0 4px;
    border-radius: 5px;
    border: 1px solid transparent;
    cursor: pointer;
    user-select: none;
    font-size: 13px;
  }
  .row:hover {
    background: var(--hover);
  }
  .row.project {
    font-weight: 600;
  }
  .row.drop-ok {
    border-color: color-mix(in srgb, var(--accent) 40%, transparent);
  }
  .row.drop-no {
    opacity: 0.45;
    cursor: not-allowed;
  }
  .row.hover {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 15%, transparent);
  }
  .chev {
    flex: none;
    width: 14px;
    display: flex;
    color: var(--muted);
    transition: transform 0.12s;
  }
  .chev.open {
    transform: rotate(90deg);
  }
  .kind {
    flex: none;
    display: flex;
    color: var(--muted);
  }
  .name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .count {
    font-size: 11px;
    color: var(--accent);
  }
  .req {
    display: flex;
    align-items: center;
    gap: 5px;
    height: 22px;
    font-size: 12px;
    color: var(--muted);
    opacity: 0.75;
  }
  .badge {
    flex: none;
    width: 30px;
    font-size: 10px;
    font-weight: 700;
  }
</style>
