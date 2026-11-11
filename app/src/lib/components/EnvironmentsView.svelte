<script lang="ts">
  // Environments tab (D28): list of a project's (or the global) environments + variables editor.
  import { tick } from "svelte";
  import { t } from "$lib/i18n/index.svelte";
  import { EnvSession, envSession, type EnvItem, type EnvRow } from "$lib/stores/environments.svelte";
  import { ws, type MenuItem } from "$lib/stores/workspace.svelte";
  import EnvIcon from "./EnvIcon.svelte";
  import { copyEnv, deleteEnv, saveEnvTab } from "./envActions";

  let { project }: { project: string | null } = $props();

  const session = $derived(envSession(project));
  const projectEntry = $derived(project ? (ws.entries.find((e) => e.path === project) ?? null) : null);
  const projectName = $derived(projectEntry?.project?.name ?? project ?? "");
  const title = $derived(project === null ? t("env.globalTitle") : t("env.projectTitle", { name: projectName }));

  $effect(() => {
    const s = session;
    if (!s.loaded && !s.loading) void s.load();
  });

  let search = $state("");
  let bulk = $state(false);
  let bulkText = $state("");
  let flash = $state<string | null>(null);
  let flashTimer: ReturnType<typeof setTimeout> | undefined;
  let nameInput = $state<HTMLInputElement>();

  const filtered = $derived(
    session.items.filter((i) => !search.trim() || i.name.toLowerCase().includes(search.trim().toLowerCase())),
  );
  const isGlobals = $derived(session.scope === "global" && session.selected === EnvSession.GLOBALS);
  const item = $derived(session.item(session.selected));
  /** Rows being edited: the selected environment's, or the globals. */
  const rows = $derived(isGlobals ? session.globals : (item?.rows ?? null));
  const nameError = $derived(item ? session.nameError(item) : null);

  // Context menu "Rename" focuses the name field.
  $effect(() => {
    if (session.focusName === 0) return;
    void tick().then(() => {
      nameInput?.focus();
      nameInput?.select();
    });
  });

  // Leaving bulk mode when the selection changes.
  $effect(() => {
    void session.selected;
    bulk = false;
  });

  function showFlash(text: string) {
    flash = text;
    clearTimeout(flashTimer);
    flashTimer = setTimeout(() => (flash = null), 4000);
  }

  function newEnv() {
    session.add(t("env.newName"));
    session.focusName++;
  }

  // ----- rows -----
  function ensureTrailing(list: EnvRow[]) {
    const last = list[list.length - 1];
    if (!last || last.key || last.value) list.push({ key: "", value: "", secret: false, enabled: true });
  }

  function removeRow(list: EnvRow[], i: number) {
    list.splice(i, 1);
    ensureTrailing(list);
  }

  // ----- bulk edit (one "key=value" per line; "//" prefix = disabled) -----
  function toBulk(list: EnvRow[]): string {
    return list
      .filter((r) => r.key.trim())
      .map((r) => `${r.enabled ? "" : "// "}${r.key}=${r.value}`)
      .join("\n");
  }

  function fromBulk(text: string, previous: EnvRow[]): EnvRow[] {
    const secret = new Map(previous.map((r) => [r.key.trim(), r.secret]));
    const out: EnvRow[] = [];
    for (const raw of text.split("\n")) {
      let line = raw.trim();
      if (!line) continue;
      let enabled = true;
      if (line.startsWith("//")) {
        enabled = false;
        line = line.slice(2).trim();
      }
      const eq = line.indexOf("=");
      const key = (eq < 0 ? line : line.slice(0, eq)).trim();
      if (!key) continue;
      out.push({ key, value: eq < 0 ? "" : line.slice(eq + 1), secret: secret.get(key) ?? false, enabled });
    }
    out.push({ key: "", value: "", secret: false, enabled: true });
    return out;
  }

  function toggleBulk() {
    if (!rows) return;
    if (!bulk) {
      bulkText = toBulk(rows);
      bulk = true;
      return;
    }
    const next = fromBulk(bulkText, rows);
    if (isGlobals) session.globals = next;
    else if (item) item.rows = next;
    bulk = false;
  }

  // ----- context menu -----
  function menuItems(it: EnvItem): MenuItem[] {
    const list: MenuItem[] = [
      {
        label: t("env.rename"),
        action: () => {
          session.selected = it.uid;
          session.focusName++;
        },
      },
      { label: t("env.duplicate"), action: () => session.duplicate(it.uid, t("env.copySuffix")) },
    ];
    if (it.original !== null) {
      if (project !== null) {
        list.push({
          label: t("env.copyToGlobal"),
          action: async () => {
            const name = await copyEnv(project, it, null);
            if (name) showFlash(t("env.copiedGlobal", { name }));
          },
        });
      } else {
        list.push({ label: t("env.copyToProject"), action: () => pickProject(it) });
      }
    }
    list.push({ label: t("env.delete"), danger: true, action: () => deleteEnv(project, it) });
    return list;
  }

  let menuPos = { x: 0, y: 0 };

  function openMenu(e: MouseEvent, it: EnvItem) {
    e.preventDefault();
    e.stopPropagation();
    menuPos = { x: e.clientX, y: e.clientY };
    ws.menu = { x: e.clientX, y: e.clientY, items: menuItems(it) };
  }

  function openMenuFromButton(e: MouseEvent, it: EnvItem) {
    e.stopPropagation();
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    menuPos = { x: r.left, y: r.bottom + 2 };
    ws.menu = { x: menuPos.x, y: menuPos.y, items: menuItems(it) };
  }

  /** Second menu: the registered projects to copy a global environment into. */
  function pickProject(it: EnvItem) {
    const projects = ws.entries.filter((e) => e.project);
    const items: MenuItem[] = projects.length
      ? projects.map((e) => ({
          label: e.project!.name,
          action: async () => {
            const name = await copyEnv(null, it, e.path);
            if (name) showFlash(t("env.copiedProject", { name, project: e.project!.name }));
          },
        }))
      : [{ label: t("env.noProjects"), action: () => {} }];
    // Open after the current menu has closed.
    setTimeout(() => (ws.menu = { x: menuPos.x, y: menuPos.y, items }));
  }
</script>

<section class="envs">
  <header>
    <span class="icon"><EnvIcon size={16} /></span>
    <h2>{title}</h2>
    {#if session.dirty}<span class="dirty-dot" title={t("request.unsaved")}></span>{/if}
    <span class="where">
      {#if project === null}
        {t("env.storedLocally")}
      {:else}
        <span class="mono">{t("env.storedIn", { path: `${project}/.ilixpatch/environments/` })}</span>
      {/if}
    </span>
    {#if flash}<span class="flash" role="status">{flash}</span>{/if}
    <button
      class="primary small"
      disabled={!session.dirty || session.saving}
      title={t("request.saveHint")}
      onclick={() => saveEnvTab(project)}
    >
      {session.saving ? t("request.saving") : t("request.save")}
    </button>
  </header>

  <div class="body">
    <aside class="list">
      <div class="list-tools">
        <input
          type="search"
          bind:value={search}
          placeholder={t("env.search")}
          aria-label={t("env.search")}
          spellcheck="false"
        />
        <button class="ghost small" title={t("env.new")} aria-label={t("env.new")} onclick={newEnv}>+</button>
      </div>

      <div class="items" role="listbox" aria-label={title}>
        {#if session.scope === "global"}
          <div
            class="item globals"
            class:active={isGlobals}
            role="option"
            aria-selected={isGlobals}
            tabindex="0"
            onclick={() => (session.selected = EnvSession.GLOBALS)}
            onkeydown={(e) => (e.key === "Enter" || e.key === " ") && (e.preventDefault(), (session.selected = EnvSession.GLOBALS))}
          >
            <span class="item-name">{t("env.globals")}</span>
            {#if session.globalsDirty}<span class="dirty-dot"></span>{/if}
          </div>
          <div class="group" role="presentation">{t("env.environments")}</div>
        {/if}

        {#if session.loading && !session.loaded}
          <p class="hint">{t("common.loading")}</p>
        {:else if session.error}
          <p class="hint error">{session.error}</p>
        {:else if filtered.length === 0}
          <p class="hint">{search.trim() ? t("env.noMatches") : t("env.none")}</p>
        {/if}

        {#each filtered as it (it.uid)}
          {@const active = session.selected === it.uid}
          <div
            class="item"
            class:active
            role="option"
            aria-selected={active}
            tabindex="0"
            onclick={() => (session.selected = it.uid)}
            onkeydown={(e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                session.selected = it.uid;
              } else if (e.key === "F2") {
                e.preventDefault();
                session.selected = it.uid;
                session.focusName++;
              }
            }}
            oncontextmenu={(e) => openMenu(e, it)}
          >
            <span class="item-name" class:unsaved={it.original === null}>{it.name || t("env.unnamed")}</span>
            {#if session.isItemDirty(it)}<span class="dirty-dot"></span>{/if}
            <button
              class="more ghost"
              title={t("tree.actions")}
              aria-label={t("tree.actions")}
              onclick={(e) => openMenuFromButton(e, it)}>⋯</button
            >
          </div>
        {/each}
      </div>
    </aside>

    <div class="editor">
      {#if rows}
        <div class="editor-head">
          {#if isGlobals}
            <h3>{t("env.globals")}</h3>
            <span class="hint inline">{t("env.globalsHint")}</span>
          {:else if item}
            <input
              bind:this={nameInput}
              class="name"
              class:invalid={!!nameError}
              bind:value={item.name}
              placeholder={t("env.namePlaceholder")}
              aria-label={t("env.namePlaceholder")}
              spellcheck="false"
            />
            {#if nameError}
              <span class="hint error inline">{nameError === "required" ? t("env.nameRequired") : t("env.nameTaken")}</span>
            {/if}
          {/if}
          <button class="ghost small bulk" onclick={toggleBulk}>{bulk ? t("env.tableEdit") : t("env.bulkEdit")}</button>
        </div>

        {#if bulk}
          <textarea class="bulk-text mono" bind:value={bulkText} spellcheck="false" aria-label={t("env.bulkEdit")}></textarea>
          <p class="hint">{t("env.bulkHint")}</p>
        {:else}
          <div class="table" role="table" aria-label={t("env.variables")}>
            <div class="tr th" role="row">
              <span role="columnheader" class="c-on"></span>
              <span role="columnheader">{t("request.key")}</span>
              <span role="columnheader">{t("request.value")}</span>
              <span role="columnheader" class="c-actions"></span>
            </div>
            {#each rows as row, i (i)}
              {@const trailing = i === rows.length - 1 && !row.key && !row.value}
              <div class="tr" class:disabled={!row.enabled} role="row">
                <span role="cell" class="c-on">
                  {#if !trailing}
                    <input type="checkbox" bind:checked={row.enabled} aria-label={t("env.enabled")} title={t("env.enabled")} />
                  {/if}
                </span>
                <span role="cell">
                  <input
                    class="mono"
                    bind:value={row.key}
                    oninput={() => ensureTrailing(rows)}
                    placeholder={trailing ? t("env.addVariable") : ""}
                    aria-label={t("request.key")}
                    spellcheck="false"
                    autocomplete="off"
                  />
                </span>
                <span role="cell" class="value-cell">
                  <input
                    class="mono"
                    type={row.secret && !row.reveal ? "password" : "text"}
                    bind:value={row.value}
                    oninput={() => ensureTrailing(rows)}
                    aria-label={t("request.value")}
                    spellcheck="false"
                    autocomplete="off"
                  />
                  {#if row.secret}
                    <button
                      class="ghost icon-btn"
                      title={row.reveal ? t("env.hide") : t("env.reveal")}
                      aria-label={row.reveal ? t("env.hide") : t("env.reveal")}
                      aria-pressed={!!row.reveal}
                      onclick={() => (row.reveal = !row.reveal)}
                    >
                      <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.4">
                        <path d="M1.5 8s2.4-4.5 6.5-4.5S14.5 8 14.5 8s-2.4 4.5-6.5 4.5S1.5 8 1.5 8z" /><circle cx="8" cy="8" r="2" />
                        {#if row.reveal}<path d="M2.5 13.5l11-11" />{/if}
                      </svg>
                    </button>
                  {/if}
                </span>
                <span role="cell" class="c-actions">
                  {#if !trailing}
                    <button
                      class="ghost icon-btn"
                      class:on={row.secret}
                      title={project === null ? t("env.secretHintGlobal") : t("env.secretHint")}
                      aria-label={t("env.secret")}
                      aria-pressed={row.secret}
                      onclick={() => (row.secret = !row.secret)}
                    >
                      <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.4">
                        <rect x="3" y="7" width="10" height="7" rx="1.5" />
                        <path d={row.secret ? "M5.5 7V5a2.5 2.5 0 015 0v2" : "M5.5 7V5a2.5 2.5 0 014.9-.7"} />
                      </svg>
                    </button>
                    <button
                      class="ghost icon-btn"
                      title={t("request.removeRow")}
                      aria-label={t("request.removeRow")}
                      onclick={() => removeRow(rows, i)}>×</button
                    >
                  {/if}
                </span>
              </div>
            {/each}
          </div>
        {/if}
      {:else if session.loaded}
        <div class="placeholder">
          <p>{t("env.pick")}</p>
          <button class="primary small" onclick={newEnv}>{t("env.new")}</button>
        </div>
      {/if}
    </div>
  </div>
</section>

<style>
  .envs {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }
  header {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px 10px;
    padding: 12px 12px 10px;
    border-bottom: 1px solid var(--border);
  }
  header .icon {
    display: flex;
    color: var(--accent);
  }
  h2 {
    margin: 0;
    font-size: 15px;
  }
  .where {
    color: var(--muted);
    font-size: 12px;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1 1 120px;
  }
  .flash {
    color: var(--ok);
    font-size: 12px;
  }
  .small {
    padding: 4px 10px;
    font-size: 12px;
  }
  .body {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(180px, 240px) minmax(0, 1fr);
  }
  .list {
    display: flex;
    flex-direction: column;
    min-height: 0;
    border-right: 1px solid var(--border);
    background: var(--surface);
  }
  .list-tools {
    display: flex;
    gap: 4px;
    padding: 8px;
  }
  .list-tools input {
    flex: 1;
    min-width: 0;
    padding: 4px 8px;
    font-size: 12px;
  }
  .items {
    flex: 1;
    overflow: auto;
    padding-bottom: 8px;
    font-size: 13px;
  }
  .group {
    color: var(--muted);
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    padding: 10px 12px 4px;
  }
  .item {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    margin: 0 4px;
    padding: 0 4px 0 8px;
    border-radius: 5px;
    cursor: pointer;
    user-select: none;
  }
  .item:hover {
    background: var(--hover);
  }
  .item.active {
    background: var(--selected);
  }
  .item:focus-visible {
    outline: 1px solid var(--accent);
  }
  .item.globals {
    margin-top: 2px;
    font-weight: 600;
  }
  .item-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .item-name.unsaved {
    font-style: italic;
  }
  .more {
    visibility: hidden;
    padding: 0 6px;
    line-height: 20px;
  }
  .item:hover .more,
  .item.active .more {
    visibility: visible;
  }
  .editor {
    min-width: 0;
    min-height: 0;
    overflow: auto;
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .editor-head {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  h3 {
    margin: 0;
    font-size: 14px;
  }
  .name {
    flex: 0 1 280px;
    min-width: 120px;
    font-weight: 600;
  }
  .name.invalid {
    border-color: var(--err);
  }
  .bulk {
    margin-left: auto;
  }
  .table {
    display: flex;
    flex-direction: column;
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow: hidden;
  }
  .tr {
    display: grid;
    grid-template-columns: 32px minmax(0, 1fr) minmax(0, 1.4fr) 64px;
    align-items: center;
    border-top: 1px solid var(--border);
  }
  .tr:first-child {
    border-top: none;
  }
  .tr.th {
    font-size: 12px;
    color: var(--muted);
    background: var(--surface-2);
  }
  .tr.th span {
    padding: 5px 8px;
  }
  .tr.disabled input:not([type="checkbox"]) {
    opacity: 0.55;
  }
  .tr input:not([type="checkbox"]) {
    width: 100%;
    border: none;
    border-radius: 0;
    background: none;
    padding: 6px 8px;
    font-size: 13px;
  }
  .tr input:not([type="checkbox"]):focus {
    box-shadow: inset 0 0 0 1px var(--accent);
  }
  .c-on {
    display: flex;
    justify-content: center;
  }
  .c-on input {
    accent-color: var(--accent);
  }
  .value-cell {
    display: flex;
    align-items: center;
    border-left: 1px solid var(--border);
  }
  .tr > span:nth-child(2) {
    border-left: 1px solid var(--border);
  }
  .c-actions {
    display: flex;
    justify-content: flex-end;
    gap: 2px;
    padding-right: 4px;
  }
  .icon-btn {
    display: flex;
    align-items: center;
    padding: 3px 5px;
    flex: none;
  }
  .icon-btn.on {
    color: var(--accent);
  }
  .bulk-text {
    min-height: 240px;
    flex: 1;
    resize: vertical;
    font-size: 13px;
  }
  .hint {
    color: var(--muted);
    font-size: 12px;
    margin: 6px 12px;
  }
  .hint.inline {
    margin: 0;
  }
  .error {
    color: var(--err);
  }
  .placeholder {
    margin: auto;
    text-align: center;
    color: var(--muted);
  }
</style>
