<script lang="ts">
  // Branch switcher for the Git tab (D16, D21): themed dropdown with a filter, local and remote branches,
  // and an inline "New branch…" input. Checkout errors are shown inside the dropdown.
  import { api, type GitBranch, type GitRepoInfo } from "$lib/api";
  import { t } from "$lib/i18n/index.svelte";
  import { git, gitError } from "$lib/stores/git.svelte";
  import { ws } from "$lib/stores/workspace.svelte";
  import GitIcon from "./GitIcon.svelte";

  let { repo, info }: { repo: string; info: GitRepoInfo | null } = $props();

  const uid = $props.id();
  const listId = `${uid}-list`;
  const optionId = (i: number) => `${uid}-opt-${i}`;

  let open = $state(false);
  let branches = $state<GitBranch[]>([]);
  let loading = $state(false);
  let loadError = $state<string | null>(null);
  let checkoutError = $state<string | null>(null);
  let filter = $state("");
  let active = $state(0);
  let creating = $state(false);
  let newName = $state("");
  let root = $state<HTMLDivElement>();
  let button = $state<HTMLButtonElement>();
  let filterInput = $state<HTMLInputElement>();
  let newInput = $state<HTMLInputElement>();
  let list = $state<HTMLUListElement>();

  const busy = $derived(!!git.view(repo).busy.checkout);
  const label = $derived(
    info ? (info.branch ?? (info.head ? `${t("git.detached")} (${info.head})` : t("git.detached"))) : "—",
  );

  const visible = $derived.by(() => {
    const q = filter.trim().toLowerCase();
    const match = (b: GitBranch) => !q || b.name.toLowerCase().includes(q);
    const all = branches.filter((b) => !b.name.endsWith("/HEAD") && match(b));
    return { local: all.filter((b) => !b.remote), remote: all.filter((b) => b.remote) };
  });
  /** Flat list of selectable branches in display order (keyboard navigation). */
  const flat = $derived([...visible.local, ...visible.remote]);

  async function load() {
    loading = true;
    try {
      branches = await api.gitBranches(repo);
      loadError = null;
    } catch (e) {
      loadError = gitError(e);
    } finally {
      loading = false;
    }
  }

  function show() {
    open = true;
    filter = "";
    creating = false;
    checkoutError = null;
    active = 0;
    void load();
  }

  function hide(refocus = true) {
    open = false;
    creating = false;
    if (refocus) button?.focus();
  }

  $effect(() => {
    if (open && !creating) filterInput?.focus();
  });
  $effect(() => {
    if (creating) newInput?.focus();
  });
  $effect(() => {
    if (!open || !list) return;
    list.querySelector(`#${CSS.escape(optionId(active))}`)?.scrollIntoView({ block: "nearest" });
  });

  async function checkout(name: string, create: boolean) {
    if (!create && branches.some((b) => b.name === name && b.current)) {
      hide();
      return;
    }
    checkoutError = null;
    const v = git.view(repo);
    if (v.busy.checkout) return;
    v.busy.checkout = true;
    try {
      await api.gitCheckout(repo, name, create);
      hide();
      git.toast("ok", t(create ? "git.branchCreated" : "git.switched", { name }));
      await git.afterChange(repo);
    } catch (e) {
      checkoutError = gitError(e);
    } finally {
      v.busy.checkout = false;
    }
  }

  /** Merge `name` into the current branch; conflicts show the merge banner in the Git view. */
  async function mergeIntoCurrent(name: string) {
    checkoutError = null;
    const v = git.view(repo);
    if (v.busy.merge) return;
    v.busy.merge = true;
    try {
      const st = await api.gitMerge(repo, name);
      git.merge[repo] = st;
      hide();
      if (st.conflicted.length) git.toast("err", t("git.merge.conflicts", { count: st.conflicted.length }));
      else git.toast("ok", t("git.merge.merged", { name }));
    } catch (e) {
      checkoutError = gitError(e);
    } finally {
      v.busy.merge = false;
    }
    await git.afterChange(repo);
  }

  function branchMenu(e: MouseEvent, b: GitBranch) {
    e.preventDefault();
    e.stopPropagation();
    if (b.current) return;
    ws.menu = {
      x: e.clientX,
      y: e.clientY,
      items: [
        { label: t("git.checkout"), action: () => void checkout(b.name, false) },
        { label: t("git.merge.intoCurrent"), action: () => void mergeIntoCurrent(b.name) },
      ],
    };
  }

  function onfilterkeydown(e: KeyboardEvent) {
    const n = flat.length;
    if (e.key === "ArrowDown" && n) active = (active + 1) % n;
    else if (e.key === "ArrowUp" && n) active = (active - 1 + n) % n;
    else if (e.key === "Enter") {
      if (flat[active]) void checkout(flat[active].name, false);
    } else if (e.key === "Escape") hide();
    else return;
    e.preventDefault();
    e.stopPropagation();
  }

  function onnewkeydown(e: KeyboardEvent) {
    if (e.key === "Enter") {
      const name = newName.trim();
      if (name) void checkout(name, true);
    } else if (e.key === "Escape") creating = false;
    else return;
    e.preventDefault();
    e.stopPropagation();
  }

  function onwindowpointerdown(e: PointerEvent) {
    // Clicks in the branch context menu (rendered outside) must not close the dropdown.
    if (ws.menu) return;
    if (open && root && !root.contains(e.target as Node)) hide(false);
  }
</script>

<svelte:window onpointerdown={onwindowpointerdown} />

<div class="branch-select" bind:this={root}>
  <button
    bind:this={button}
    type="button"
    class="trigger"
    aria-haspopup="listbox"
    aria-expanded={open}
    aria-controls={listId}
    title={t("git.switchBranch")}
    disabled={!info}
    onclick={() => (open ? hide() : show())}
    onkeydown={(e) => {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        show();
      }
    }}
  >
    <GitIcon size={13} />
    <span class="mono name">{label}</span>
    {#if busy}<span class="spinner" aria-hidden="true"></span>{/if}
    <svg class="caret" viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
      <path d="M4 6l4 4 4-4" fill="none" stroke="currentColor" stroke-width="1.6" />
    </svg>
  </button>

  {#if open}
    <div class="pop">
      <input
        bind:this={filterInput}
        bind:value={filter}
        class="filter"
        role="combobox"
        aria-expanded="true"
        aria-controls={listId}
        aria-activedescendant={flat[active] ? optionId(active) : undefined}
        aria-label={t("git.filterBranches")}
        placeholder={t("git.filterBranches")}
        oninput={() => (active = 0)}
        onkeydown={onfilterkeydown}
      />
      {#if checkoutError}<div class="error" role="alert">{checkoutError}</div>{/if}
      {#if loadError}<div class="error" role="alert">{loadError}</div>{/if}
      <ul bind:this={list} id={listId} class="list" role="listbox" aria-label={t("git.branches")}>
        {#if loading && branches.length === 0}
          <li class="hint" role="presentation">{t("git.loading")}</li>
        {/if}
        {#if visible.local.length}
          <li class="section" role="presentation">{t("git.localBranches")}</li>
        {/if}
        {#each flat as b, i (b.name + (b.remote ? "@r" : ""))}
          {#if i === visible.local.length && visible.remote.length}
            <li class="section" role="presentation">{t("git.remoteBranches")}</li>
          {/if}
          <!-- Keyboard interaction is handled on the filter input (aria-activedescendant pattern). -->
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <li
            id={optionId(i)}
            role="option"
            aria-selected={b.current}
            class:active={i === active}
            class:current={b.current}
            onclick={() => checkout(b.name, false)}
            onpointermove={() => (active = i)}
            oncontextmenu={(e) => branchMenu(e, b)}
          >
            <span class="mono bname">{b.name}</span>
            {#if b.upstream && !b.remote}<span class="mono upstream">{b.upstream}</span>{/if}
            {#if !b.current}
              <button
                class="merge-btn"
                title={t("git.merge.intoCurrent")}
                disabled={!!git.view(repo).busy.merge}
                onclick={(e) => {
                  e.stopPropagation();
                  void mergeIntoCurrent(b.name);
                }}>{t("git.merge.short")}</button
              >
            {/if}
            {#if b.current}
              <svg class="check" viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
                <path d="M3.5 8.5l3 3 6-7" fill="none" stroke="currentColor" stroke-width="1.8" />
              </svg>
            {/if}
          </li>
        {/each}
        {#if !loading && !loadError && flat.length === 0}
          <li class="hint" role="presentation">{t("git.noBranches")}</li>
        {/if}
      </ul>
      <div class="new">
        {#if creating}
          <input
            bind:this={newInput}
            bind:value={newName}
            class="mono"
            placeholder={t("git.newBranchName")}
            aria-label={t("git.newBranchName")}
            onkeydown={onnewkeydown}
          />
          <button class="primary" disabled={!newName.trim() || busy} onclick={() => checkout(newName.trim(), true)}
            >{t("git.create")}</button
          >
        {:else}
          <button
            class="ghost link"
            onclick={() => {
              newName = filter.trim();
              creating = true;
              checkoutError = null;
            }}>{t("git.newBranch")}</button
          >
        {/if}
      </div>
    </div>
  {/if}
</div>

<style>
  .branch-select {
    position: relative;
    min-width: 0;
  }
  .trigger {
    display: flex;
    align-items: center;
    gap: 6px;
    max-width: 280px;
    padding: 4px 8px;
  }
  .trigger[aria-expanded="true"],
  .trigger:focus-visible {
    outline: none;
    border-color: var(--accent);
  }
  .name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
  }
  .caret {
    flex: none;
    color: var(--muted);
  }
  .pop {
    position: absolute;
    z-index: 40;
    top: calc(100% + 4px);
    left: 0;
    width: 320px;
    max-width: calc(100vw - 32px);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: var(--shadow);
    padding: 6px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .filter {
    width: 100%;
  }
  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    max-height: 300px;
    overflow: auto;
  }
  li {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 6px;
    border-radius: 5px;
    cursor: pointer;
    font-size: 12px;
  }
  li.active {
    background: var(--hover);
  }
  li.current .bname {
    font-weight: 700;
  }
  li.section,
  li.hint {
    cursor: default;
    color: var(--muted);
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    padding-top: 8px;
  }
  li.hint {
    text-transform: none;
    letter-spacing: 0;
  }
  .bname {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .upstream {
    color: var(--muted);
    font-size: 11px;
    white-space: nowrap;
  }
  .merge-btn {
    visibility: hidden;
    flex: none;
    padding: 0 6px;
    font-size: 11px;
  }
  li.active .merge-btn {
    visibility: visible;
  }
  .check {
    flex: none;
    color: var(--accent);
  }
  .error {
    color: var(--err);
    font-size: 12px;
    white-space: pre-wrap;
    word-break: break-word;
    max-height: 120px;
    overflow: auto;
  }
  .new {
    display: flex;
    gap: 6px;
    border-top: 1px solid var(--border);
    padding-top: 6px;
  }
  .new input {
    flex: 1;
    min-width: 0;
  }
  .link {
    width: 100%;
    text-align: left;
    color: var(--accent);
  }
  .spinner {
    flex: none;
    width: 11px;
    height: 11px;
    border: 2px solid var(--border);
    border-top-color: var(--accent);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
