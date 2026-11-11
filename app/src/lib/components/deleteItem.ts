import { ask } from "@tauri-apps/plugin-dialog";
import { t } from "$lib/i18n/index.svelte";
import { ws } from "$lib/stores/workspace.svelte";

/** Confirm with a native dialog, then delete a request file, folder or collection. */
export async function confirmDelete(path: string, name: string, projectPath: string) {
  let ok = false;
  try {
    ok = await ask(t("tree.deleteConfirm", { name }), { title: t("tree.deleteTitle"), kind: "warning" });
  } catch (e) {
    ws.fail(e);
  }
  if (ok) await ws.deleteItem(path, projectPath);
}
