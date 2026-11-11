// Actions of the Environments tabs that touch both the environments store and the workspace.
import { ask } from "@tauri-apps/plugin-dialog";
import { api } from "$lib/api";
import { t } from "$lib/i18n/index.svelte";
import { envSession, envStore, peekEnvSession, type EnvItem } from "$lib/stores/environments.svelte";
import { ws } from "$lib/stores/workspace.svelte";

/** After a change on disk: refresh what the selectors/sending use. */
async function refreshScope(project: string | null) {
  if (project) await ws.reload(project);
  else await envStore.refreshGlobal();
}

/** Save every dirty environment of a tab (Cmd/Ctrl+S). */
export async function saveEnvTab(project: string | null) {
  const session = envSession(project);
  if (!session.dirty) return;
  const error = await session.saveAll({ required: t("env.nameRequired"), taken: t("env.nameTaken") });
  await refreshScope(project);
  if (error) ws.fail(error);
}

export async function deleteEnv(project: string | null, item: EnvItem) {
  let ok = false;
  try {
    ok = await ask(t("env.deleteConfirm", { name: item.name || item.original || "" }), {
      title: t("env.deleteTitle"),
      kind: "warning",
    });
  } catch (e) {
    ws.fail(e);
  }
  if (!ok) return;
  const session = envSession(project);
  if (item.original !== null) {
    try {
      await api.envDelete(session.scope, project, item.original);
    } catch (e) {
      ws.fail(e);
      return;
    }
  }
  session.removeLocal(item.uid);
  await refreshScope(project);
}

/**
 * Copy a saved environment to the other scope: project → global, or global → `toProject`.
 * Returns the name of the created copy (made unique by the backend), or null on failure.
 */
export async function copyEnv(project: string | null, item: EnvItem, toProject: string | null): Promise<string | null> {
  if (item.original === null) return null;
  try {
    const from =
      project === null
        ? ({ scope: "global", name: item.original } as const)
        : ({ scope: "project", project_path: project, name: item.original } as const);
    const doc = await api.envCopy(from, toProject === null ? "global" : "project", toProject, null);
    await refreshScope(toProject);
    // Keep an open tab of the destination in sync (unless it has unsaved edits).
    const dest = peekEnvSession(toProject);
    if (dest?.loaded && !dest.dirty) await dest.load();
    return doc.name;
  } catch (e) {
    ws.fail(e);
    return null;
  }
}
