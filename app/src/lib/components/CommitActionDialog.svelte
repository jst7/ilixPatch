<script lang="ts">
  // Dialogs for history actions that need options (D27): revert, reset current branch, new branch from a commit.
  import type { GitResetMode } from "$lib/api";
  import { t } from "$lib/i18n/index.svelte";
  import { git, type CommitAction } from "$lib/stores/git.svelte";

  let { repo, action }: { repo: string; action: CommitAction } = $props();

  const view = $derived(git.view(repo));
  const busy = $derived(!!view.busy.history);
  const c = $derived(action.commit);

  let dialog: HTMLDialogElement | undefined = $state();
  let noCommit = $state(false);
  let mode = $state<GitResetMode>("mixed");
  let confirmHash = $state("");
  let branchName = $state("");
  let switchTo = $state(true);
  let nameInput: HTMLInputElement | undefined = $state();

  $effect(() => {
    if (dialog && !dialog.open) {
      dialog.showModal();
      if (action.kind === "branch") nameInput?.focus();
    }
  });

  const hardOk = $derived(mode !== "hard" || confirmHash.trim().toLowerCase() === c.short_id.toLowerCase());
  const nameOk = $derived(/^[^\s~^:?*[\\]+$/.test(branchName.trim()) && !branchName.trim().startsWith("-"));
  const canSubmit = $derived(!busy && (action.kind === "reset" ? hardOk : action.kind === "branch" ? nameOk : true));

  function close() {
    if (busy) return;
    view.action = null;
  }

  async function submit() {
    if (!canSubmit) return;
    let done = true;
    if (action.kind === "revert") await git.revertCommit(repo, c, noCommit);
    else if (action.kind === "reset") done = await git.resetTo(repo, c, mode);
    else done = await git.createBranchAt(repo, branchName.trim(), c, switchTo);
    // Keep the dialog open on failure (the error is shown as a toast) so the user can adjust and retry.
    if (done) view.action = null;
  }

  const MODES: { mode: GitResetMode; label: () => string; hint: () => string }[] = [
    { mode: "soft", label: () => t("git.hist.resetSoft"), hint: () => t("git.hist.resetSoftHint") },
    { mode: "mixed", label: () => t("git.hist.resetMixed"), hint: () => t("git.hist.resetMixedHint") },
    { mode: "hard", label: () => t("git.hist.resetHard"), hint: () => t("git.hist.resetHardHint") },
  ];

  const title = $derived(
    action.kind === "revert" ? t("git.hist.revertTitle") : action.kind === "reset" ? t("git.hist.resetTitle") : t("git.hist.newBranchTitle"),
  );
</script>

<dialog
  bind:this={dialog}
  aria-labelledby="commit-action-title"
  oncancel={(e) => {
    e.preventDefault();
    close();
  }}
  onclose={() => (view.action = null)}
  onclick={(e) => e.target === dialog && close()}
>
  <form
    class="content"
    onsubmit={(e) => {
      e.preventDefault();
      void submit();
    }}
  >
    <h2 id="commit-action-title">{title}</h2>
    <p class="commit"><span class="mono sha">{c.short_id}</span> {c.summary}</p>

    {#if action.kind === "revert"}
      <p>{t("git.hist.revertExplain")}</p>
      <label class="check">
        <input type="checkbox" bind:checked={noCommit} />
        <span>{t("git.hist.revertNoCommit")}</span>
      </label>
    {:else if action.kind === "reset"}
      <p>{t("git.hist.resetExplain")}</p>
      <div class="modes" role="radiogroup" aria-label={t("git.hist.resetMode")}>
        {#each MODES as m (m.mode)}
          <label class="mode" class:danger={m.mode === "hard"} class:active={mode === m.mode}>
            <input type="radio" name="reset-mode" value={m.mode} bind:group={mode} />
            <span>
              <strong>{m.label()}</strong>
              <span class="hint">{m.hint()}</span>
            </span>
          </label>
        {/each}
      </div>
      {#if mode === "hard"}
        <div class="warning" role="alert">
          <p>{t("git.hist.resetHardWarning")}</p>
          <label class="field">
            <span>{t("git.hist.resetTypeHash", { id: c.short_id })}</span>
            <input class="mono" bind:value={confirmHash} autocomplete="off" spellcheck="false" />
          </label>
        </div>
      {/if}
    {:else}
      <label class="field">
        <span>{t("git.newBranchName")}</span>
        <input bind:this={nameInput} class="mono" bind:value={branchName} autocomplete="off" spellcheck="false" aria-invalid={!!branchName && !nameOk} />
      </label>
      <label class="check">
        <input type="checkbox" bind:checked={switchTo} />
        <span>{t("git.hist.switchToBranch")}</span>
      </label>
    {/if}

    <div class="actions">
      <button type="button" onclick={close} disabled={busy}>{t("tabs.cancel")}</button>
      <button type="submit" class={action.kind === "reset" && mode === "hard" ? "danger-btn" : "primary"} disabled={!canSubmit}>
        {#if busy}<span class="spinner" aria-hidden="true"></span>{/if}
        {action.kind === "revert" ? t("git.hist.revertOk") : action.kind === "reset" ? t("git.hist.resetOk") : t("git.create")}
      </button>
    </div>
  </form>
</dialog>

<style>
  dialog {
    width: min(520px, calc(100vw - 32px));
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
    gap: 10px;
    font-size: 13px;
  }
  h2 {
    margin: 0;
    font-size: 16px;
  }
  p {
    margin: 0;
  }
  .commit {
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sha {
    color: var(--accent);
  }
  .check,
  .mode {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    cursor: pointer;
  }
  .check input,
  .mode input {
    margin: 2px 0 0;
    accent-color: var(--accent);
  }
  .modes {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .mode {
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 8px 10px;
  }
  .mode.active {
    border-color: var(--accent);
  }
  .mode.danger strong {
    color: var(--err);
  }
  .mode.danger.active {
    border-color: var(--err);
  }
  .mode > span {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .hint {
    color: var(--muted);
    font-size: 12px;
  }
  .warning {
    border: 1px solid var(--err);
    border-radius: 8px;
    padding: 8px 10px;
    color: var(--err);
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .field input {
    color: var(--text);
  }
  input[aria-invalid="true"] {
    border-color: var(--err);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 4px;
  }
  .actions button {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .danger-btn {
    background: var(--err);
    border-color: var(--err);
    color: var(--accent-text);
    font-weight: 600;
  }
  .spinner {
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
