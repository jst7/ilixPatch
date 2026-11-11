// History actions on one commit (D27), shared by the history rows' context menu and the details header.
import { ask } from "@tauri-apps/plugin-dialog";
import { t } from "$lib/i18n/index.svelte";
import { git, gitError, type CommitRef } from "$lib/stores/git.svelte";
import type { MenuItem } from "$lib/stores/workspace.svelte";

async function confirm(message: string, title: string, okLabel: string): Promise<boolean> {
  try {
    return await ask(message, { title, kind: "warning", okLabel, cancelLabel: t("tabs.cancel") });
  } catch (e) {
    git.toast("err", gitError(e));
    return false;
  }
}

export async function copyHash(id: string) {
  try {
    await navigator.clipboard.writeText(id);
    git.toast("ok", t("git.hist.copied", { id: id.slice(0, 12) }));
  } catch (e) {
    git.toast("err", gitError(e));
  }
}

export async function checkoutCommit(repo: string, c: CommitRef) {
  if (await confirm(t("git.hist.checkoutConfirm", { id: c.short_id }), t("git.hist.checkout"), t("git.hist.checkoutOk"))) {
    await git.checkoutCommit(repo, c);
  }
}

export async function cherryPick(repo: string, c: CommitRef) {
  if (await confirm(t("git.hist.cherryPickConfirm", { id: c.short_id, summary: c.summary }), t("git.hist.cherryPick"), t("git.hist.cherryPickOk"))) {
    await git.cherryPick(repo, c);
  }
}

export async function restoreFile(repo: string, path: string, name: string, c: CommitRef) {
  if (await confirm(t("git.hist.restoreConfirm", { name, id: c.short_id }), t("git.hist.restoreTitle"), t("git.hist.restoreOk"))) {
    await git.restoreFile(repo, path, c);
  }
}

/** Context-menu items for a commit. */
export function commitMenu(repo: string, c: CommitRef): MenuItem[] {
  const v = git.view(repo);
  return [
    { label: t("git.hist.copyHash"), action: () => void copyHash(c.id) },
    { label: t("git.hist.newBranch"), action: () => (v.action = { kind: "branch", commit: c }) },
    { label: t("git.hist.checkout"), action: () => void checkoutCommit(repo, c) },
    { label: t("git.hist.cherryPick"), action: () => void cherryPick(repo, c) },
    { label: t("git.hist.revert"), action: () => (v.action = { kind: "revert", commit: c }) },
    { label: t("git.hist.reset"), danger: true, action: () => (v.action = { kind: "reset", commit: c }) },
  ];
}
