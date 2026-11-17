// Postman export (D58): a whole project into a folder, or one collection into a file.
import { ask, message, open, save } from "@tauri-apps/plugin-dialog";
import { api, type ExportReport } from "$lib/api";
import { t, type MessageKey } from "$lib/i18n/index.svelte";
import EN from "$lib/i18n/locales/en";
import { ws } from "$lib/stores/workspace.svelte";

const slug = (name: string) =>
  name
    .normalize("NFKD")
    .replace(/[̀-ͯ]/g, "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "") || "collection";

const dirOf = (p: string) => p.replace(/[\\/][^\\/]*$/, "") || p;

/** Summary dialog: what was written, secrets left empty and warnings (translated by code). */
async function showReport(report: ExportReport, dir: string) {
  const lines = [
    t("export.done", {
      collections: report.collections,
      requests: report.requests,
      environments: report.environments,
      dir,
    }),
  ];
  if (report.secrets_omitted) lines.push(t("export.secretsOmitted", { count: report.secrets_omitted }));
  if (report.warnings.length) {
    lines.push("", t("export.warnings"));
    for (const w of report.warnings) {
      const key = `export.warn.${w.code}`;
      lines.push(`• ${w.item}: ${key in EN ? t(key as MessageKey) : w.message}`);
    }
  }
  await message(lines.join("\n"), { title: t("export.doneTitle"), kind: report.warnings.length ? "warning" : "info" });
}

/** Secret values in the project environments that have a value. */
async function secretCount(projectPath: string): Promise<number> {
  const docs = await api.envList("project", projectPath);
  return docs.reduce((n, d) => n + d.variables.filter((v) => v.secret && v.value).length, 0);
}

export async function exportProjectToPostman(projectPath: string) {
  try {
    const dir = await open({ directory: true, multiple: false, title: t("export.chooseFolder") });
    if (typeof dir !== "string") return;
    const secrets = await secretCount(projectPath);
    const includeSecrets =
      secrets > 0 &&
      (await ask(t("export.secretsAsk", { count: secrets }), {
        title: t("export.secretsTitle"),
        kind: "warning",
        okLabel: t("export.secretsInclude"),
        cancelLabel: t("export.secretsOmit"),
      }));
    const report = await api.exportPostmanProject(projectPath, dir, includeSecrets);
    await showReport(report, dir);
  } catch (e) {
    ws.fail(e);
  }
}

export async function exportCollectionToPostman(collectionPath: string, name: string) {
  try {
    const file = await save({
      title: t("export.saveCollection"),
      defaultPath: `${slug(name)}.postman_collection.json`,
      filters: [{ name: "Postman", extensions: ["json"] }],
    });
    if (!file) return;
    const report = await api.exportPostmanCollection(collectionPath, file, false);
    await showReport(report, dirOf(file));
  } catch (e) {
    ws.fail(e);
  }
}
