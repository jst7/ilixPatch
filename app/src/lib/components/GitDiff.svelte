<script lang="ts">
  // Working-tree diff pane of the Changes sub-tab: HEAD version vs. the file on disk.
  import { api, type GitFileDiff, type GitFileStatus } from "$lib/api";
  import { t } from "$lib/i18n/index.svelte";
  import { git, gitError } from "$lib/stores/git.svelte";
  import { relativePath, stateColor, stateLetter } from "$lib/git/status";
  import DiffBody from "./DiffBody.svelte";

  let { repo, file }: { repo: string; file: GitFileStatus } = $props();

  let diff = $state<GitFileDiff | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(false);

  // Reload when the file changes or the repo's status is refreshed (the file may have changed on disk).
  $effect(() => {
    const path = file.path;
    void git.statusVersion[repo];
    let cancelled = false;
    loading = true;
    api
      .gitDiffFile(path)
      .then((d) => {
        if (cancelled) return;
        diff = d;
        error = null;
      })
      .catch((e) => {
        if (cancelled) return;
        diff = null;
        error = gitError(e);
      })
      .finally(() => {
        if (!cancelled) loading = false;
      });
    return () => {
      cancelled = true;
    };
  });
</script>

<div class="git-diff">
  <div class="head">
    <span class="letter mono" style:color={stateColor(file.state)}>{stateLetter(file.state)}</span>
    <span class="mono path" title={file.path}>{relativePath(file.path, repo)}</span>
    {#if file.old_path}<span class="muted mono">← {relativePath(file.old_path, repo)}</span>{/if}
  </div>

  <div class="body">
    {#if error}
      <p class="error" role="alert">{error}</p>
    {:else if diff && diff.path === file.path}
      <DiffBody old={diff.old} new={diff.new} path={file.path} oldTitle={t("git.diffHead")} newTitle={t("git.diffWorking")} />
    {:else if loading}
      <p class="muted pad">{t("git.loading")}</p>
    {/if}
  </div>
</div>

<style>
  .git-diff {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-width: 0;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    border-bottom: 1px solid var(--border);
    font-size: 12px;
    min-height: 36px;
  }
  .letter {
    font-weight: 700;
  }
  .path {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .body {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .muted {
    color: var(--muted);
  }
  .pad {
    padding: 0 12px;
  }
  .error {
    color: var(--err);
    padding: 0 12px;
    white-space: pre-wrap;
  }
</style>
