<script lang="ts">
  // Arrange step of the Postman import (D31): what to import (left), where it goes (right), options + summary.
  import { open } from "@tauri-apps/plugin-dialog";
  import { api, type ImportWarning } from "$lib/api";
  import { t } from "$lib/i18n/index.svelte";
  import { ws } from "$lib/stores/workspace.svelte";
  import { DEFAULT_SECRET_PATTERN, importer } from "$lib/stores/import.svelte";
  import { isExcluded } from "$lib/import/plan";
  import { dnd, placeMenu, revealSource, warningText, warningTitle, warningsByNode } from "$lib/import/ui.svelte";
  import DestNode from "./DestNode.svelte";
  import SourceNode from "./SourceNode.svelte";

  const preview = $derived(importer.preview!);
  const warnings = $derived(warningsByNode(preview.warnings));
  const projects = $derived(ws.entries.filter((e) => e.project));
  const dest = $derived(ws.entries.find((e) => e.path === importer.project)?.project ?? null);
  /** Sources that are placed and still checked (ghosts are drawn for these). */
  const placed = $derived(
    new Set(importer.placements.filter((p) => !isExcluded(importer.index, importer.excluded, p.source_id)).map((p) => p.source_id)),
  );

  const groups = $derived.by(() => {
    const map = new Map<string, ImportWarning[]>();
    for (const w of preview.warnings) {
      const list = map.get(w.code) ?? [];
      list.push(w);
      map.set(w.code, list);
    }
    return [...map.entries()];
  });

  const nameOf = (id: string | null) => (id ? (importer.index.get(id)?.node.name ?? preview.environments.find((e) => e.id === id)?.name ?? "") : "");

  async function onProjectChange(select: HTMLSelectElement) {
    const value = select.value;
    if (value !== "__new") {
      importer.setProject(value || null);
      return;
    }
    try {
      const dir = await open({ directory: true, multiple: false, title: t("import.dest.newProjectTitle") });
      if (typeof dir !== "string") {
        select.value = importer.project ?? "";
        return;
      }
      const project = await api.addProject(dir);
      ws.entries = await api.listProjects();
      ws.setExpanded(project.path, true);
      void ws.refreshRepos(project.path);
      importer.setProject(project.path);
    } catch (e) {
      select.value = importer.project ?? "";
      ws.fail(e);
    }
  }

  function placeSelected(e: MouseEvent) {
    if (!importer.selected) return;
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    placeMenu(importer.selected, r.left, r.bottom + 2);
  }

  const summaryText = $derived(
    t("import.summary", {
      collections: importer.summary.collections,
      folders: importer.summary.folders,
      requests: importer.summary.requests,
      environments: importer.envCount,
      project: dest?.name ?? "—",
    }),
  );
</script>

<div class="arrange" class:dragging={!!dnd.drag}>
  <div class="columns">
    <!-- Left: from Postman -->
    <section class="col source" aria-labelledby="import-src-title">
      <header>
        <h3 id="import-src-title">{t("import.src.title")}</h3>
        <button class="small" disabled={!importer.selected} onclick={placeSelected} title={t("import.src.placeHint")}>{t("import.src.place")}</button>
      </header>
      <p class="hint">{t("import.src.help")}</p>
      <div class="tree" role="tree" aria-label={t("import.src.title")}>
        {#each preview.collections as c (c.id)}
          <SourceNode node={c} depth={0} {warnings} />
        {/each}
        {#if preview.collections.length === 0}<p class="muted pad">{t("import.src.noCollections")}</p>{/if}
      </div>

      {#if preview.environments.length}
        <h4>{t("import.env.title")}</h4>
        <ul class="envs">
          {#each preview.environments as env (env.id)}
            {@const choice = importer.envs[env.id]}
            {#if choice}
              <li>
                <label class="check">
                  <input type="checkbox" bind:checked={choice.checked} />
                  <span class="name">{env.name}</span>
                  <span class="muted small-text">{t("import.env.vars", { count: env.variables.length })}</span>
                </label>
                <select bind:value={choice.scope} disabled={!choice.checked} aria-label={t("import.env.scope")}>
                  <option value="project">{t("import.env.project")}</option>
                  <option value="global">{t("import.env.global")}</option>
                </select>
              </li>
            {/if}
          {/each}
        </ul>
      {/if}
      {#if preview.globals?.length}
        <label class="row-field">
          <span>{t("import.globals", { count: preview.globals.length })}</span>
          <select bind:value={importer.globals}>
            <option value="skip">{t("import.globals.skip")}</option>
            <option value="merge">{t("import.globals.merge")}</option>
          </select>
        </label>
      {/if}
    </section>

    <!-- Right: destination -->
    <section class="col dest" aria-labelledby="import-dest-title">
      <header>
        <h3 id="import-dest-title">{t("import.dest.title")}</h3>
        <button class="small" disabled={!importer.project} onclick={() => importer.resetPlacements()}>{t("import.dest.reset")}</button>
      </header>
      <select
        class="project"
        value={importer.project ?? ""}
        aria-label={t("import.dest.project")}
        onchange={(e) => void onProjectChange(e.currentTarget)}
      >
        <option value="" disabled>{t("import.dest.choose")}</option>
        {#each projects as p (p.path)}
          <option value={p.path}>{p.project?.name}</option>
        {/each}
        <option value="__new">{t("import.dest.newProject")}</option>
      </select>
      <p class="hint">{t("import.dest.help")}</p>
      <div class="tree" role="tree" aria-label={t("import.dest.title")}>
        {#if dest}
          <DestNode kind="project" path={dest.path} name={dest.name} collections={dest.collections} depth={0} {placed} />
        {:else}
          <p class="muted pad">{t("import.dest.none")}</p>
        {/if}
      </div>
    </section>
  </div>

  <!-- Options, summary and warnings -->
  <section class="bottom" aria-label={t("import.options")}>
    <div class="options">
      <label class="row-field">
        <span>{t("import.opt.conflict")}</span>
        <select bind:value={importer.onConflict}>
          <option value="rename">{t("import.opt.conflict.rename")}</option>
          <option value="overwrite">{t("import.opt.conflict.overwrite")}</option>
          <option value="skip">{t("import.opt.conflict.skip")}</option>
        </select>
      </label>
      <label class="check"><input type="checkbox" bind:checked={importer.convertAuth} /> {t("import.opt.convertAuth")}</label>
      <label class="check"><input type="checkbox" bind:checked={importer.keepScripts} /> {t("import.opt.keepScripts")}</label>
      <label class="check">
        <input type="checkbox" bind:checked={importer.inlineParentScripts} disabled={!importer.keepScripts} />
        {t("import.opt.inlineParent")}
      </label>
      <div class="secret">
        <label class="check"><input type="checkbox" bind:checked={importer.secretEnabled} /> {t("import.opt.secret")}</label>
        <input
          class="mono"
          bind:value={importer.secretPattern}
          disabled={!importer.secretEnabled}
          aria-label={t("import.opt.secretPattern")}
          aria-invalid={!importer.patternValid}
          spellcheck="false"
        />
        {#if importer.secretPattern !== DEFAULT_SECRET_PATTERN}
          <button class="ghost small" onclick={() => (importer.secretPattern = DEFAULT_SECRET_PATTERN)}>{t("import.opt.secretDefault")}</button>
        {/if}
      </div>
      {#if !importer.patternValid}<p class="invalid">{t("import.opt.secretInvalid")}</p>{/if}
    </div>

    <div class="summary">
      <p class="summary-text" aria-live="polite">{summaryText}</p>
      {#if importer.summary.unplaced}
        <p class="warn-text">{t("import.unplaced", { count: importer.summary.unplaced })}</p>
      {/if}
      {#if groups.length}
        <details class="warnings">
          <summary>{t("import.warningsCount", { count: preview.warnings.length })}</summary>
          {#each groups as [code, list] (code)}
            <div class="wgroup">
              <strong>{warningTitle(code) ?? code} ({list.length})</strong>
              <ul>
                {#each list as w, i (i)}
                  <li>
                    {#if w.source_id && importer.index.has(w.source_id)}
                      <button class="link" onclick={() => revealSource(w.source_id!)}>{nameOf(w.source_id)}</button>
                    {:else if w.source_id}
                      <span>{nameOf(w.source_id)}</span>
                    {/if}
                    <span class="muted">{warningTitle(code) ? w.message : warningText(w)}</span>
                  </li>
                {/each}
              </ul>
            </div>
          {/each}
        </details>
      {/if}
      {#if importer.error}<p class="error" role="alert">{importer.error}</p>{/if}
      <div class="actions">
        <button onclick={() => (importer.step = "source")}>{t("import.back")}</button>
        <button class="primary" disabled={!importer.canImport} onclick={() => importer.apply()}>
          {#if importer.applying}<span class="spinner" aria-hidden="true"></span>{/if}
          {t("import.import")}
        </button>
      </div>
    </div>
  </section>
</div>

{#if dnd.drag}
  <div class="drag-label" style:left="{dnd.drag.x + 12}px" style:top="{dnd.drag.y + 8}px" aria-hidden="true">{dnd.drag.label}</div>
{/if}

<style>
  .arrange {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
  }
  .arrange.dragging {
    cursor: grabbing;
  }
  .columns {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: 1fr 1fr;
  }
  .col {
    display: flex;
    flex-direction: column;
    min-height: 0;
    min-width: 0;
    padding: 8px 0;
  }
  .col.dest {
    border-left: 1px solid var(--border);
  }
  .col header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 0 12px;
  }
  h3 {
    margin: 0;
    font-size: 14px;
  }
  h4 {
    margin: 10px 12px 4px;
    font-size: 12px;
    color: var(--muted);
  }
  .hint {
    margin: 4px 12px 6px;
    font-size: 12px;
    color: var(--muted);
  }
  .tree {
    flex: 1;
    min-height: 80px;
    overflow: auto;
    padding-bottom: 8px;
  }
  .project {
    margin: 6px 12px 0;
  }
  .envs {
    list-style: none;
    margin: 0;
    padding: 0 12px;
    display: flex;
    flex-direction: column;
    gap: 4px;
    max-height: 140px;
    overflow: auto;
  }
  .envs li {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .envs .check {
    flex: 1;
    min-width: 0;
  }
  .envs select {
    padding: 2px 6px;
    font-size: 12px;
  }
  .check {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 13px;
    cursor: pointer;
  }
  .check input {
    margin: 0;
    accent-color: var(--accent);
  }
  .name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .row-field {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    padding: 4px 12px;
  }
  .row-field select {
    padding: 2px 6px;
    font-size: 12px;
  }
  .bottom {
    flex: none;
    border-top: 1px solid var(--border);
    display: grid;
    grid-template-columns: 1fr 1fr;
    max-height: 45%;
    overflow: auto;
  }
  .options {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 10px 0;
  }
  .options .check {
    padding: 0 12px;
  }
  .secret {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 12px;
    flex-wrap: wrap;
  }
  .secret .check {
    padding: 0;
  }
  .secret input.mono {
    flex: 1;
    min-width: 140px;
    padding: 2px 8px;
    font-size: 12px;
  }
  input[aria-invalid="true"] {
    border-color: var(--err);
  }
  .invalid,
  .error {
    color: var(--err);
    font-size: 12px;
    margin: 0 12px;
    white-space: pre-wrap;
  }
  .summary {
    border-left: 1px solid var(--border);
    padding: 10px 12px;
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
  }
  .summary-text {
    margin: 0;
    font-weight: 600;
  }
  .warn-text {
    margin: 0;
    color: var(--warn);
    font-size: 12px;
  }
  .warnings summary {
    cursor: pointer;
    color: var(--warn);
    font-size: 13px;
  }
  .wgroup {
    margin: 6px 0;
    font-size: 12px;
  }
  .wgroup ul {
    margin: 2px 0 0;
    padding-left: 16px;
  }
  .wgroup li {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
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
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: auto;
  }
  .actions button {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .small {
    padding: 2px 8px;
    font-size: 12px;
  }
  .small-text {
    font-size: 11px;
  }
  .muted {
    color: var(--muted);
  }
  .pad {
    padding: 0 12px;
  }
  .drag-label {
    position: fixed;
    z-index: 70;
    pointer-events: none;
    padding: 2px 8px;
    border-radius: 5px;
    background: var(--accent);
    color: var(--accent-text);
    font-size: 12px;
    box-shadow: var(--shadow);
  }
  .spinner {
    width: 11px;
    height: 11px;
    border: 2px solid currentColor;
    border-top-color: transparent;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
