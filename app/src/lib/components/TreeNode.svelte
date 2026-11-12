<script lang="ts">
  import type { Collection, Folder, ProjectEntry } from "$lib/api";
  import { t } from "$lib/i18n/index.svelte";
  import { ws, type CreateKind, type MenuItem } from "$lib/stores/workspace.svelte";
  import { confirmDelete } from "./deleteItem";
  import GitIcon from "./GitIcon.svelte";
  import EnvIcon from "./EnvIcon.svelte";
  import { git } from "$lib/stores/git.svelte";
  import { importer } from "$lib/stores/import.svelte";
  import { stateColor } from "$lib/git/status";
  import InlineInput from "./InlineInput.svelte";
  import RequestRow from "./RequestRow.svelte";
  import TreeNode from "./TreeNode.svelte";
  import Highlight from "./Highlight.svelte";
  import { treeSearch } from "$lib/search/tree.svelte";

  type Props =
    | { kind: "project"; entry: ProjectEntry; depth: number }
    | { kind: "collection"; collection: Collection; projectPath: string; depth: number }
    | { kind: "folder"; folder: Folder; projectPath: string; depth: number };

  let props: Props = $props();

  function basename(p: string): string {
    return p.split(/[\\/]/).filter(Boolean).pop() ?? p;
  }

  const path = $derived(
    props.kind === "project" ? props.entry.path : props.kind === "collection" ? props.collection.path : props.folder.path,
  );
  const name = $derived(
    props.kind === "project"
      ? (props.entry.project?.name ?? basename(props.entry.path))
      : props.kind === "collection"
        ? props.collection.name
        : props.folder.name,
  );
  const projectPath = $derived(props.kind === "project" ? props.entry.path : props.projectPath);
  const folder: Folder | null = $derived(
    props.kind === "collection" ? props.collection.root : props.kind === "folder" ? props.folder : null,
  );
  const error = $derived(props.kind === "project" ? props.entry.error : null);
  const defaultOpen = $derived(props.kind === "project");
  /** Active tree search: only matching nodes (and their ancestors) are shown, expanded. */
  const filter = $derived(treeSearch.result);
  const shown = (p: string) => !filter || filter.visible.has(p);
  const expanded = $derived(filter ? treeSearch.isExpanded(path) : ws.isExpanded(path, defaultOpen));
  const pending = $derived(ws.pendingCreate?.parentPath === path ? ws.pendingCreate : null);

  const placeholders: Record<CreateKind, () => string> = {
    collection: () => t("tree.collectionName"),
    folder: () => t("tree.folderName"),
    request: () => t("tree.requestName"),
  };

  const repo = $derived(ws.repoByRoot.get(path) ?? null);
  const gitState = $derived(git.dirState.get(path));
  const branchLabel = $derived(repo ? (repo.branch ?? repo.head ?? t("git.detached")) : "");
  const gitTitle = $derived.by(() => {
    if (!repo) return "";
    const parts = [`${t("tree.gitOpen")} — ${repo.branch ?? `${t("git.detached")}${repo.head ? ` (${repo.head})` : ""}`}`];
    if (repo.dirty) parts.push(t("tree.gitDirty"));
    if (repo.ahead || repo.behind) parts.push(t("tree.gitAheadBehind", { ahead: repo.ahead, behind: repo.behind }));
    if (repo.conflicted) parts.push(t("tree.gitConflicted"));
    return parts.join("\n");
  });

  function openGit() {
    ws.openTab({ kind: "git", repo: path });
  }

  function gitItem(): MenuItem {
    return repo
      ? { label: t("tree.git"), action: openGit }
      : { label: t("tree.gitInit"), action: () => ws.initGit(path, projectPath) };
  }

  /** Items for the "+" button: what can be created directly under this node. */
  function addItems(): MenuItem[] {
    return [
      { label: t("tree.newRequest"), action: () => ws.startCreate("request", path, projectPath) },
      { label: t("tree.newFolder"), action: () => ws.startCreate("folder", path, projectPath) },
    ];
  }

  function items(): MenuItem[] {
    if (props.kind === "project") {
      const list: MenuItem[] = [];
      if (props.entry.project) {
        list.push(
          { label: t("tree.newCollection"), action: () => ws.startCreate("collection", path, projectPath) },
          { label: t("tree.importPostman"), action: () => importer.open(projectPath) },
          gitItem(),
        );
      }
      list.push(
        { label: t("tree.reload"), action: () => ws.reload(projectPath) },
        { label: t("tree.removeProject"), danger: true, action: () => ws.removeProject(projectPath) },
      );
      return list;
    }
    return [
      ...addItems(),
      { label: t("tree.authorization"), action: () => ws.openContainer(path) },
      { label: t("tree.run"), action: () => ws.openTab({ kind: "runner", target: path }) },
      { label: t("tree.loadTest"), action: () => ws.openTab({ kind: "load", target: path }) },
      gitItem(),
      { label: t("tree.delete"), danger: true, action: () => confirmDelete(path, name, projectPath) },
    ];
  }

  function onAdd(e: MouseEvent) {
    e.stopPropagation();
    if (props.kind === "project") {
      ws.startCreate("collection", path, projectPath);
      return;
    }
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    ws.menu = { x: r.left, y: r.bottom + 2, items: addItems() };
  }

  let rowEl = $state<HTMLDivElement>();
  $effect(() => {
    if (ws.revealPath !== path || !rowEl) return;
    rowEl.scrollIntoView({ block: "nearest" });
    rowEl.focus({ preventScroll: true });
    ws.revealPath = null;
  });

  function openMenu(e: MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    ws.menu = { x: e.clientX, y: e.clientY, items: items() };
  }

  function openMenuFromButton(e: MouseEvent) {
    e.stopPropagation();
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    ws.menu = { x: r.left, y: r.bottom + 2, items: items() };
  }

  function setOpen(value: boolean) {
    if (filter) treeSearch.collapsed[path] = !value;
    else ws.setExpanded(path, value);
  }

  function toggle() {
    if (filter) treeSearch.toggle(path);
    else ws.toggle(path, defaultOpen);
  }
</script>

<div
  bind:this={rowEl}
  class="row {props.kind}"
  role="treeitem"
  aria-selected="false"
  aria-expanded={expanded}
  tabindex="0"
  style:padding-left="{8 + props.depth * 14}px"
  onclick={toggle}
  onkeydown={(e) => {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      toggle();
    } else if (e.key === "ArrowRight" && !expanded) setOpen(true);
    else if (e.key === "ArrowLeft" && expanded) setOpen(false);
  }}
  oncontextmenu={openMenu}
  title={path}
>
  <span class="chev" class:open={expanded}>
    <svg viewBox="0 0 16 16" width="12" height="12"><path d="M6 4l4 4-4 4" fill="none" stroke="currentColor" stroke-width="1.6" /></svg>
  </span>
  <span class="icon">
    {#if props.kind === "project"}
      <svg viewBox="0 0 16 16" width="14" height="14"><rect x="2" y="2.5" width="12" height="11" rx="2" fill="none" stroke="currentColor" stroke-width="1.3" /><path d="M2 6h12" stroke="currentColor" stroke-width="1.3" /></svg>
    {:else if props.kind === "collection"}
      <svg viewBox="0 0 16 16" width="14" height="14"><path d="M3 3h10v3H3zM3 7.5h10v3H3zM3 12h10" fill="none" stroke="currentColor" stroke-width="1.3" /></svg>
    {:else}
      <svg viewBox="0 0 16 16" width="14" height="14"><path d="M1.5 4.5a1 1 0 011-1h3.5l1.5 1.5h6a1 1 0 011 1v6.5a1 1 0 01-1 1h-11a1 1 0 01-1-1z" fill="none" stroke="currentColor" stroke-width="1.3" /></svg>
    {/if}
  </span>
  <span class="name" style:color={gitState ? stateColor(gitState) : undefined}><Highlight text={name} terms={treeSearch.terms} /></span>
  {#if props.kind === "project" && props.entry.project}
    <button
      class="env-btn"
      title={t("env.openProject")}
      aria-label={t("env.openProject")}
      onclick={(e) => {
        e.stopPropagation();
        ws.openEnvironments(path);
      }}
      onkeydown={(e) => e.stopPropagation()}
    >
      <EnvIcon size={12} />
    </button>
  {/if}
  {#if repo}
    <button
      class="git"
      class:conflicted={repo.conflicted}
      title={gitTitle}
      aria-label={gitTitle}
      onclick={(e) => {
        e.stopPropagation();
        openGit();
      }}
      onkeydown={(e) => e.stopPropagation()}
    >
      <GitIcon size={11} />
      <span class="branch mono">{branchLabel}</span>
      {#if repo.dirty}<span class="git-dot"></span>{/if}
      {#if repo.ahead || repo.behind}
        <span class="sync mono">{repo.ahead ? `↑${repo.ahead}` : ""}{repo.ahead && repo.behind ? " " : ""}{repo.behind
            ? `↓${repo.behind}`
            : ""}</span>
      {/if}
    </button>
  {/if}
  <span class="spacer"></span>
  {#if props.kind !== "project" || props.entry.project}
    <button
      class="more ghost"
      title={props.kind === "project" ? t("tree.newCollection") : t("tree.add")}
      aria-label={props.kind === "project" ? t("tree.newCollection") : t("tree.add")}
      onclick={onAdd}>+</button
    >
  {/if}
  <button class="more ghost" title={t("tree.actions")} aria-label={t("tree.actions")} onclick={openMenuFromButton}
    >⋯</button
  >
</div>

{#if error}
  <div class="error" style:padding-left="{8 + props.depth * 14 + 18}px">{t("tree.loadError")}: {error}</div>
{/if}

{#if expanded}
  <div role="group">
    {#if pending}
      <InlineInput
        depth={props.depth + 1}
        placeholder={placeholders[pending.kind]()}
        oncommit={(n) => ws.commitCreate(n)}
        oncancel={() => (ws.pendingCreate = null)}
      />
    {/if}
    {#if props.kind === "project" && props.entry.project}
      {#each props.entry.project.collections as collection (collection.path)}
        {#if shown(collection.path)}
          <TreeNode kind="collection" {collection} {projectPath} depth={props.depth + 1} />
        {/if}
      {/each}
      {#if props.entry.project.collections.length === 0 && !pending && !filter}
        <div class="empty" style:padding-left="{8 + (props.depth + 1) * 14 + 18}px">
          <button class="link" onclick={() => ws.startCreate("collection", path, projectPath)}>
            {t("tree.newCollection")}
          </button>
        </div>
      {/if}
    {:else if folder}
      {#each folder.folders as sub (sub.path)}
        {#if shown(sub.path)}
          <TreeNode kind="folder" folder={sub} {projectPath} depth={props.depth + 1} />
        {/if}
      {/each}
      {#each folder.requests as file (file.path)}
        {#if shown(file.path)}
          <RequestRow {file} {projectPath} depth={props.depth + 1} />
        {/if}
      {/each}
      {#if folder.folders.length === 0 && folder.requests.length === 0 && !pending && !filter}
        <div class="empty" style:padding-left="{8 + (props.depth + 1) * 14 + 18}px">{t("tree.empty")}</div>
      {/if}
    {/if}
  </div>
{/if}

<style>
  .row {
    display: flex;
    align-items: center;
    gap: 4px;
    height: 28px;
    padding-right: 4px;
    margin: 0 4px;
    border-radius: 5px;
    cursor: pointer;
    user-select: none;
  }
  .row:hover {
    background: var(--hover);
  }
  .row:focus-visible {
    outline: 1px solid var(--accent);
  }
  .row.project {
    font-weight: 600;
  }
  .chev {
    flex: none;
    display: flex;
    width: 14px;
    color: var(--muted);
    transition: transform 0.12s;
  }
  .chev.open {
    transform: rotate(90deg);
  }
  .icon {
    flex: none;
    display: flex;
    color: var(--muted);
    margin-right: 2px;
  }
  .spacer {
    flex: 1;
  }
  .name {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .more {
    visibility: hidden;
    padding: 0 6px;
    line-height: 20px;
  }
  .row:hover .more,
  .row:focus-within .more {
    visibility: visible;
  }
  .env-btn {
    flex: none;
    display: flex;
    align-items: center;
    padding: 2px 4px;
    border: none;
    border-radius: 4px;
    background: none;
    color: var(--muted);
    opacity: 0.75;
  }
  .env-btn:hover {
    opacity: 1;
    color: var(--text);
    background: var(--hover);
  }
  .git {
    flex: 0 1 auto;
    min-width: 0;
    max-width: 50%;
    display: flex;
    align-items: center;
    gap: 3px;
    padding: 0 5px;
    height: 20px;
    border: none;
    border-radius: 4px;
    background: none;
    color: var(--muted);
    font-size: 11px;
    font-weight: 400;
  }
  .git:hover {
    background: var(--hover);
    color: var(--text);
  }
  .git.conflicted {
    color: var(--err);
  }
  .git :global(svg) {
    flex: none;
  }
  .branch {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .git-dot {
    flex: none;
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--warn);
  }
  .sync {
    flex: none;
    white-space: nowrap;
  }
  .error {
    color: var(--err);
    font-size: 12px;
    padding: 2px 8px 4px 0;
    word-break: break-word;
  }
  .empty {
    color: var(--muted);
    font-size: 12px;
    padding: 4px 8px 4px 0;
  }
  .link {
    border: none;
    background: none;
    padding: 0;
    color: var(--accent);
    font-size: 12px;
  }
  .link:hover {
    text-decoration: underline;
  }
</style>
