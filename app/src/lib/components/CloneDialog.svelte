<script lang="ts">
  // Clone a git repository (D26): URL, destination, optional branch / shallow depth / HTTPS credentials,
  // live progress and cancel. Credentials are never persisted and are cleared when the dialog closes.
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { api, type GitCloneEvent } from "$lib/api";
  import { t } from "$lib/i18n/index.svelte";
  import { ws } from "$lib/stores/workspace.svelte";
  import { git, gitError } from "$lib/stores/git.svelte";

  let { open, onclose }: { open: boolean; onclose: () => void } = $props();

  const LS_PARENT = "ilixpatch.cloneParent";

  let dialog: HTMLDialogElement | undefined = $state();
  let urlInput: HTMLInputElement | undefined = $state();

  let url = $state("");
  let parent = $state(loadParent());
  let dirName = $state("");
  /** True once the user edits the folder name (it stops following the URL). */
  let dirEdited = $state(false);
  let branch = $state("");
  let shallow = $state(false);
  let depth = $state(1);
  let showCreds = $state(false);
  let username = $state("");
  let token = $state("");
  let addAsProject = $state(true);

  let cloning = $state(false);
  let cancelling = $state(false);
  let progress = $state<{ phase: string; percent: number | null; message: string } | null>(null);
  let error = $state<string | null>(null);
  /** Show validation messages only after the first submit attempt. */
  let submitted = $state(false);

  function loadParent(): string {
    try {
      return localStorage.getItem(LS_PARENT) ?? "";
    } catch {
      return "";
    }
  }

  $effect(() => {
    if (!dialog) return;
    if (open && !dialog.open) {
      dialog.showModal();
      urlInput?.focus();
    }
    if (!open && dialog.open) dialog.close();
  });

  /** Repo name from a clone URL: last path segment without ".git". */
  function deriveName(u: string): string {
    const clean = u.trim().replace(/[\\/]+$/, "").replace(/\.git$/i, "");
    const seg = clean.split(/[\\/:]/).pop() ?? "";
    return seg.replace(/[^\w.-]+/g, "-").replace(/^[.-]+/, "");
  }

  $effect(() => {
    if (!dirEdited) dirName = deriveName(url);
  });

  const trimmedUrl = $derived(url.trim());
  const urlError = $derived.by(() => {
    if (!trimmedUrl) return t("clone.error.urlRequired");
    if (trimmedUrl.startsWith("-")) return t("clone.error.urlInvalid");
    if (!/^(https?:\/\/[^\s]+|ssh:\/\/[^\s]+|file:\/\/[^\s]+|git@[^\s:]+:[^\s]+)$/i.test(trimmedUrl)) return t("clone.error.urlInvalid");
    return null;
  });
  const nameError = $derived.by(() => {
    const n = dirName.trim();
    if (!n) return t("clone.error.nameRequired");
    if (n.startsWith("-") || n === "." || n === ".." || /[\\/]/.test(n)) return t("clone.error.nameInvalid");
    return null;
  });
  const parentError = $derived(parent ? null : t("clone.error.parentRequired"));
  const depthError = $derived(shallow && !(Number.isInteger(depth) && depth >= 1) ? t("clone.error.depth") : null);
  const valid = $derived(!urlError && !nameError && !parentError && !depthError);

  const sep = $derived(parent.includes("\\") && !parent.includes("/") ? "\\" : "/");
  const target = $derived(parent && dirName.trim() ? `${parent.replace(/[\\/]+$/, "")}${sep}${dirName.trim()}` : "");

  async function chooseParent() {
    try {
      const dir = await openDialog({ directory: true, multiple: false, title: t("clone.chooseParent"), defaultPath: parent || undefined });
      if (typeof dir === "string") {
        parent = dir;
        try {
          localStorage.setItem(LS_PARENT, dir);
        } catch {}
      }
    } catch (e) {
      error = gitError(e);
    }
  }

  function onevent(e: GitCloneEvent) {
    if (e.type === "progress") progress = { phase: e.phase, percent: e.percent, message: e.message };
  }

  async function clone() {
    submitted = true;
    if (!valid || cloning) return;
    cloning = true;
    cancelling = false;
    error = null;
    progress = null;
    try {
      const result = await api.gitClone(
        {
          url: trimmedUrl,
          parent_dir: parent,
          dir_name: dirName.trim(),
          branch: branch.trim() || null,
          depth: shallow ? depth : null,
          username: showCreds && username.trim() ? username.trim() : null,
          token: showCreds && token ? token : null,
          add_as_project: addAsProject,
        },
        onevent,
      );
      const name = result.project?.name ?? dirName.trim();
      if (result.project) await showProject(result.project.path || result.path);
      git.toast("ok", t("clone.done", { name }));
      close(true);
    } catch (e) {
      error = cancelling ? t("clone.cancelled") : gitError(e);
    } finally {
      cloning = false;
      cancelling = false;
      progress = null;
    }
  }

  /** Reload the project list, then expand, reveal and refresh the new project. */
  async function showProject(path: string) {
    try {
      ws.entries = await api.listProjects();
    } catch (e) {
      ws.fail(e);
      return;
    }
    const entry = ws.entries.find((x) => x.path === path) ?? ws.entries.find((x) => x.project?.path === path);
    if (!entry) return;
    ws.setExpanded(entry.path, true);
    ws.reveal(entry.path);
    void ws.refreshRepos(entry.path);
  }

  async function cancel() {
    if (!cloning) return;
    cancelling = true;
    try {
      await api.gitCancelClone();
    } catch (e) {
      error = gitError(e);
    }
  }

  /** Close and forget the credentials. While cloning, closing is refused (cancel first). */
  function close(force = false) {
    if (cloning && !force) return;
    username = "";
    token = "";
    showCreds = false;
    error = null;
    submitted = false;
    progress = null;
    if (force) {
      url = "";
      branch = "";
      dirEdited = false;
    }
    onclose();
  }
</script>

<dialog
  bind:this={dialog}
  aria-labelledby="clone-title"
  oncancel={(e) => {
    e.preventDefault();
    close();
  }}
  onclick={(e) => e.target === dialog && close()}
>
  <form
    class="content"
    onsubmit={(e) => {
      e.preventDefault();
      void clone();
    }}
  >
    <header>
      <h2 id="clone-title">{t("clone.title")}</h2>
      <button type="button" class="ghost" onclick={() => close()} disabled={cloning} aria-label={t("settings.close")}>✕</button>
    </header>

    <label class="field">
      <span>{t("clone.url")}</span>
      <input
        bind:this={urlInput}
        bind:value={url}
        class="mono"
        type="text"
        autocomplete="off"
        autocapitalize="off"
        spellcheck="false"
        placeholder={t("clone.urlPlaceholder")}
        disabled={cloning}
        aria-invalid={submitted && !!urlError}
      />
      {#if submitted && urlError}<span class="invalid">{urlError}</span>{/if}
    </label>

    <div class="field">
      <span>{t("clone.parent")}</span>
      <div class="inline">
        <input class="mono grow" type="text" readonly value={parent} placeholder={t("clone.parentPlaceholder")} aria-label={t("clone.parent")} />
        <button type="button" onclick={chooseParent} disabled={cloning}>{t("clone.browse")}</button>
      </div>
      {#if submitted && parentError}<span class="invalid">{parentError}</span>{/if}
    </div>

    <label class="field">
      <span>{t("clone.dirName")}</span>
      <input
        bind:value={dirName}
        class="mono"
        type="text"
        autocomplete="off"
        spellcheck="false"
        disabled={cloning}
        oninput={() => (dirEdited = true)}
        aria-invalid={submitted && !!nameError}
      />
      {#if submitted && nameError}<span class="invalid">{nameError}</span>{/if}
      {#if target}<span class="hint">{t("clone.target", { path: target })}</span>{/if}
    </label>

    <label class="field">
      <span>{t("clone.branch")}</span>
      <input bind:value={branch} class="mono" type="text" autocomplete="off" spellcheck="false" placeholder={t("clone.branchPlaceholder")} disabled={cloning} />
    </label>

    <div class="inline">
      <label class="check">
        <input type="checkbox" bind:checked={shallow} disabled={cloning} />
        <span>{t("clone.shallow")}</span>
      </label>
      {#if shallow}
        <label class="inline depth">
          <span>{t("clone.depth")}</span>
          <input type="number" min="1" step="1" bind:value={depth} disabled={cloning} aria-invalid={!!depthError} />
        </label>
      {/if}
    </div>
    {#if depthError}<span class="invalid">{depthError}</span>{/if}

    <details bind:open={showCreds}>
      <summary>{t("clone.credentials")}</summary>
      <div class="creds">
        <p class="hint">{t("clone.credentialsHint")}</p>
        <label class="field">
          <span>{t("clone.username")}</span>
          <input bind:value={username} type="text" autocomplete="off" spellcheck="false" disabled={cloning} />
        </label>
        <label class="field">
          <span>{t("clone.token")}</span>
          <input bind:value={token} type="password" autocomplete="off" disabled={cloning} />
        </label>
      </div>
    </details>

    <label class="check">
      <input type="checkbox" bind:checked={addAsProject} disabled={cloning} />
      <span>{t("clone.addAsProject")}</span>
    </label>

    {#if cloning}
      <div class="progress" role="status" aria-live="polite">
        <div
          class="bar"
          class:indeterminate={progress?.percent == null}
          role="progressbar"
          aria-label={t("clone.progress")}
          aria-valuemin="0"
          aria-valuemax="100"
          aria-valuenow={progress?.percent ?? undefined}
        >
          <div class="fill" style:width={progress?.percent != null ? `${Math.min(100, Math.max(0, progress.percent))}%` : undefined}></div>
        </div>
        <span class="phase">
          {cancelling ? t("clone.cancelling") : progress ? (progress.percent != null ? `${progress.phase} — ${Math.round(progress.percent)}%` : progress.phase) : t("clone.starting")}
        </span>
      </div>
    {/if}

    {#if error}<p class="error" role="alert">{error}</p>{/if}

    <div class="actions">
      {#if cloning}
        <button type="button" class="danger" onclick={cancel} disabled={cancelling}>{t("clone.cancel")}</button>
      {:else}
        <button type="button" onclick={() => close()}>{t("tabs.cancel")}</button>
      {/if}
      <button type="submit" class="primary" disabled={cloning || (submitted && !valid)}>{t("clone.submit")}</button>
    </div>
  </form>
</dialog>

<style>
  dialog {
    width: min(560px, calc(100vw - 32px));
    max-height: calc(100vh - 64px);
    padding: 0;
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--surface);
    color: var(--text);
  }
  dialog::backdrop {
    background: color-mix(in srgb, var(--bg) 55%, transparent);
  }
  .content {
    padding: 16px 20px 20px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  h2 {
    margin: 0;
    font-size: 17px;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 13px;
  }
  .field > span:first-child {
    color: var(--muted);
  }
  input,
  button {
    background: var(--surface-2);
  }
  input[aria-invalid="true"] {
    border-color: var(--err);
  }
  .inline {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .grow {
    flex: 1;
    min-width: 0;
  }
  .check {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    cursor: pointer;
  }
  .check input {
    margin: 0;
    accent-color: var(--accent);
  }
  .depth {
    font-size: 13px;
    color: var(--muted);
  }
  .depth input {
    width: 80px;
  }
  .hint {
    color: var(--muted);
    font-size: 12px;
    margin: 0;
    word-break: break-all;
  }
  .invalid {
    color: var(--err);
    font-size: 12px;
  }
  details {
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 6px 10px;
  }
  summary {
    cursor: pointer;
    font-size: 13px;
  }
  .creds {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 8px 0 4px;
  }
  .progress {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .bar {
    position: relative;
    height: 6px;
    border-radius: 3px;
    background: var(--surface-2);
    border: 1px solid var(--border);
    overflow: hidden;
  }
  .fill {
    height: 100%;
    background: var(--accent);
    transition: width 0.2s;
  }
  .indeterminate .fill {
    position: absolute;
    width: 35%;
    animation: slide 1.2s ease-in-out infinite;
  }
  @keyframes slide {
    from {
      left: -35%;
    }
    to {
      left: 100%;
    }
  }
  .phase {
    font-size: 12px;
    color: var(--muted);
  }
  .error {
    margin: 0;
    color: var(--err);
    font-size: 13px;
    white-space: pre-wrap;
    word-break: break-word;
    max-height: 160px;
    overflow: auto;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
  button.danger {
    color: var(--err);
    border-color: var(--err);
  }
</style>
