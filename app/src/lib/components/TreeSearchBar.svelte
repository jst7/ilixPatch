<script lang="ts">
  // Search box of the Collections view: filters the project tree (see $lib/search/tree.svelte).
  import { t } from "$lib/i18n/index.svelte";
  import { searchFocus, shortcutLabel } from "$lib/search/focus.svelte";
  import { treeSearch } from "$lib/search/tree.svelte";
  import { ws } from "$lib/stores/workspace.svelte";
  import SearchBox from "./SearchBox.svelte";

  let input = $state<HTMLInputElement>();
  let value = $state(treeSearch.input);
  let root = $state<HTMLDivElement>();

  // Focus on Cmd/Ctrl+P (ignore the counter value we were mounted with).
  let seen = searchFocus.sidebar;
  $effect(() => {
    const n = searchFocus.sidebar;
    if (n === seen) return;
    seen = n;
    input?.focus();
    input?.select();
  });

  const count = $derived(
    treeSearch.result
      ? treeSearch.result.requests.length === 1
        ? t("search.oneResult")
        : t("search.results", { n: treeSearch.result.requests.length })
      : null,
  );

  function firstRow(): HTMLElement | null {
    return root?.closest(".sidebar")?.querySelector<HTMLElement>(".tree [role=treeitem]") ?? null;
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      treeSearch.apply();
      // Wait for the filtered tree to render.
      requestAnimationFrame(() => firstRow()?.focus());
    } else if (e.key === "Enter") {
      e.preventDefault();
      treeSearch.apply();
      const first = treeSearch.result?.requests[0];
      if (first) ws.select(first);
    } else if (e.key === "Escape" && !value) {
      // Nothing to clear: also leave the field (default behaviour of SearchBox).
      treeSearch.clear();
    }
  }
</script>

<div class="tree-search" bind:this={root}>
  <SearchBox
    bind:value
    bind:input
    placeholder={t("search.tree")}
    {count}
    shortcut={shortcutLabel("P")}
    oninput={(v) => treeSearch.setInput(v)}
    {onkeydown}
  >
    {#snippet extra()}
      <button
        type="button"
        class="opt"
        class:on={treeSearch.content}
        aria-pressed={treeSearch.content}
        title={t("search.inContentHint")}
        aria-label={t("search.inContent")}
        onclick={() => (treeSearch.content = !treeSearch.content)}
      >
        <svg viewBox="0 0 16 16" width="13" height="13" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.4">
          <path d="M4 1.5h5.5L13 5v9.5H4z" /><path d="M9.5 1.5V5H13M6 8h5M6 10.5h5M6 13h3" />
        </svg>
      </button>
    {/snippet}
  </SearchBox>
</div>

<style>
  .tree-search {
    padding: 8px 8px 2px;
  }
  .opt {
    flex: none;
    display: flex;
    align-items: center;
    border: 1px solid transparent;
    background: none;
    color: var(--muted);
    padding: 2px 4px;
    border-radius: 4px;
  }
  .opt:hover {
    color: var(--text);
    background: var(--hover);
  }
  .opt.on {
    color: var(--accent);
    border-color: var(--accent);
  }
</style>
