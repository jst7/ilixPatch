<script lang="ts">
  // Import tab (D31): Source → Arrange → Report. State lives in the import store (single wizard).
  import { onMount } from "svelte";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { open } from "@tauri-apps/plugin-dialog";
  import { t } from "$lib/i18n/index.svelte";
  import { ws } from "$lib/stores/workspace.svelte";
  import { importer, type ImportStep } from "$lib/stores/import.svelte";
  import { warningText } from "$lib/import/ui.svelte";
  import ImportArrange from "./ImportArrange.svelte";

  let over = $state(false);

  // Real file paths come from Tauri's drag & drop events (the DOM only gets file names).
  onMount(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    getCurrentWebview()
      .onDragDropEvent((event) => {
        if (importer.step !== "source") return;
        const p = event.payload;
        if (p.type === "over" || p.type === "enter") over = true;
        else if (p.type === "leave") over = false;
        else if (p.type === "drop") {
          over = false;
          if (p.paths.length) void importer.addPaths(p.paths);
        }
      })
      .then((fn) => {
        if (disposed) fn();
        else unlisten = fn;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  });

  async function chooseFiles() {
    try {
      const res = await open({
        multiple: true,
        title: t("import.source.chooseFiles"),
        filters: [{ name: t("import.source.filterName"), extensions: ["json", "zip"] }],
      });
      const list = Array.isArray(res) ? res : typeof res === "string" ? [res] : [];
      if (list.length) await importer.addPaths(list);
    } catch (e) {
      ws.fail(e);
    }
  }

  async function chooseFolder() {
    try {
      const dir = await open({ directory: true, multiple: false, title: t("import.source.chooseFolder") });
      if (typeof dir === "string") await importer.addPaths([dir]);
    } catch (e) {
      ws.fail(e);
    }
  }

  const preview = $derived(importer.preview);
  const found = $derived.by(() => {
    if (!preview) return null;
    let requests = 0;
    const walk = (n: { kind: string; children: { kind: string; children: unknown[] }[] }) => {
      if (n.kind === "request") requests++;
      for (const c of n.children) walk(c as typeof n);
    };
    preview.collections.forEach(walk);
    return {
      collections: preview.collections.length,
      requests,
      environments: preview.environments.length,
      globals: preview.globals?.length ?? 0,
    };
  });
  const canContinue = $derived(!!found && (found.collections > 0 || found.environments > 0 || found.globals > 0));

  const STEPS: { id: ImportStep; label: () => string }[] = [
    { id: "source", label: () => t("import.step.source") },
    { id: "arrange", label: () => t("import.step.arrange") },
    { id: "report", label: () => t("import.step.report") },
  ];

  const basename = (p: string) => p.split(/[\\/]/).filter(Boolean).pop() ?? p;
  const report = $derived(importer.report);
  const relToProject = (p: string) => (importer.project && p.startsWith(importer.project) ? p.slice(importer.project.length + 1) : p);

  function openFile(path: string) {
    if (ws.index.has(path)) ws.select(path);
    else ws.reveal(path);
  }

  function openProject() {
    if (!importer.project) return;
    ws.setExpanded(importer.project, true);
    ws.reveal(importer.project);
  }
</script>

<section class="import-view">
  <header>
    <h2>{t("import.title")}</h2>
    <ol class="steps" aria-label={t("import.steps")}>
      {#each STEPS as s, i (s.id)}
        <li class:current={importer.step === s.id} aria-current={importer.step === s.id ? "step" : undefined}>
          <span class="num">{i + 1}</span>{s.label()}
        </li>
      {/each}
    </ol>
  </header>

  {#if importer.step === "source"}
    <div class="source">
      <div class="drop" class:over role="region" aria-label={t("import.source.drop")}>
        <svg viewBox="0 0 24 24" width="28" height="28" fill="none" stroke="currentColor" stroke-width="1.5" aria-hidden="true"
          ><path d="M12 15V4M7.5 8.5L12 4l4.5 4.5M4 15v3a2 2 0 002 2h12a2 2 0 002-2v-3" /></svg
        >
        <p class="drop-title">{t("import.source.drop")}</p>
        <div class="buttons">
          <button class="primary" onclick={chooseFiles}>{t("import.source.chooseFiles")}</button>
          <button onclick={chooseFolder}>{t("import.source.chooseFolder")}</button>
        </div>
        <p class="muted small-text">{t("import.source.formats")}</p>
      </div>

      {#if importer.paths.length}
        <ul class="paths">
          {#each importer.paths as p (p)}
            {@const err = preview?.errors.find((e) => e.file === p)}
            <li>
              <span class="mono path" title={p}>{basename(p)}</span>
              {#if err}<span class="error">{err.message}</span>{/if}
              <button class="ghost x" aria-label={t("import.source.remove")} title={t("import.source.remove")} onclick={() => importer.removePath(p)}>×</button>
            </li>
          {/each}
          {#each preview?.errors.filter((e) => !importer.paths.includes(e.file)) ?? [] as err (err.file)}
            <li><span class="mono path" title={err.file}>{basename(err.file)}</span><span class="error">{err.message}</span></li>
          {/each}
        </ul>
      {/if}

      {#if importer.loading}<p class="muted">{t("import.source.reading")}</p>{/if}
      {#if importer.error}<p class="error" role="alert">{importer.error}</p>{/if}
      {#if found}
        <p class="found">
          {t("import.source.found", { collections: found.collections, requests: found.requests, environments: found.environments })}
          {#if found.globals}· {t("import.globals", { count: found.globals })}{/if}
        </p>
      {/if}

      <div class="actions">
        {#if importer.paths.length}<button onclick={() => importer.reset()}>{t("import.source.clear")}</button>{/if}
        <button class="primary" disabled={!canContinue || importer.loading} onclick={() => (importer.step = "arrange")}>{t("import.continue")}</button>
      </div>

      <details class="help">
        <summary>{t("import.help.title")}</summary>
        <ul>
          <li>{t("import.help.collection")}</li>
          <li>{t("import.help.environment")}</li>
          <li>{t("import.help.dump")}</li>
        </ul>
      </details>
    </div>
  {:else if importer.step === "arrange" && preview}
    <ImportArrange />
  {:else if importer.step === "report" && report}
    <div class="report">
      <p class="done">
        {t("import.report.done", {
          collections: report.collections,
          folders: report.folders,
          requests: report.requests,
          environments: report.environments,
        })}
      </p>
      {#if report.skipped}<p class="muted">{t("import.report.skipped", { count: report.skipped })}</p>{/if}
      {#if report.warnings.length}
        <details>
          <summary class="warn-text">{t("import.warningsCount", { count: report.warnings.length })}</summary>
          <ul class="list">
            {#each report.warnings as w, i (i)}<li>{warningText(w)}</li>{/each}
          </ul>
        </details>
      {/if}
      {#if report.files.length}
        <h3>{t("import.report.files", { count: report.files.length })}</h3>
        <ul class="list files">
          {#each report.files as f (f)}
            <li><button class="link mono" onclick={() => openFile(f)} title={f}>{relToProject(f)}</button></li>
          {/each}
        </ul>
      {/if}
      <div class="actions">
        <button onclick={() => importer.reset()}>{t("import.report.more")}</button>
        <button class="primary" onclick={openProject}>{t("import.report.openProject")}</button>
      </div>
    </div>
  {/if}
</section>

<style>
  .import-view {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    overflow: hidden;
  }
  header {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 10px 16px;
    border-bottom: 1px solid var(--border);
    flex-wrap: wrap;
  }
  h2 {
    margin: 0;
    font-size: 16px;
  }
  .steps {
    display: flex;
    gap: 14px;
    list-style: none;
    margin: 0;
    padding: 0;
    font-size: 13px;
    color: var(--muted);
  }
  .steps li {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .steps li.current {
    color: var(--text);
    font-weight: 600;
  }
  .num {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    border: 1px solid currentColor;
    font-size: 11px;
  }
  .steps li.current .num {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--accent-text);
  }
  .source,
  .report {
    flex: 1;
    overflow: auto;
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    max-width: 760px;
  }
  .drop {
    border: 2px dashed var(--border);
    border-radius: 10px;
    padding: 24px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    color: var(--muted);
    text-align: center;
  }
  .drop.over {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 8%, transparent);
    color: var(--text);
  }
  .drop-title {
    margin: 0;
    font-weight: 600;
    color: var(--text);
  }
  .buttons {
    display: flex;
    gap: 8px;
  }
  .paths {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .paths li {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
  }
  .path {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .x {
    margin-left: auto;
    padding: 0 6px;
  }
  .error {
    color: var(--err);
    font-size: 12px;
    white-space: pre-wrap;
  }
  .found {
    margin: 0;
    font-weight: 600;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
  .help {
    font-size: 13px;
    color: var(--muted);
  }
  .help summary {
    cursor: pointer;
  }
  .help ul {
    margin: 6px 0 0;
    padding-left: 18px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .muted {
    color: var(--muted);
  }
  .small-text {
    font-size: 12px;
    margin: 0;
  }
  .done {
    margin: 0;
    font-weight: 600;
    font-size: 15px;
  }
  .warn-text {
    color: var(--warn);
    cursor: pointer;
  }
  h3 {
    margin: 4px 0 0;
    font-size: 13px;
  }
  .list {
    margin: 0;
    padding-left: 18px;
    font-size: 12px;
  }
  .files {
    list-style: none;
    padding: 0;
    max-height: 320px;
    overflow: auto;
  }
  .link {
    border: none;
    background: none;
    padding: 1px 0;
    color: var(--accent);
    font-size: 12px;
    text-align: left;
  }
  .link:hover {
    text-decoration: underline;
  }
</style>
