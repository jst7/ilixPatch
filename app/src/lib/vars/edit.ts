// Where a `{{variable}}` comes from and how to change it (URL bar hover, D43).
// Precedence of a send: environment > collection > globals; in each scope a runtime value
// (set by a script this session) wins over the file value.
import { api, type Collection, type EnvVariable } from "$lib/api";
import { ws } from "$lib/stores/workspace.svelte";
import { envStore, enabledVars } from "$lib/stores/environments.svelte";
import { runtimeVars } from "$lib/stores/runtimeVars.svelte";
import { DYNAMIC_VARS } from "$lib/url/tokens";

export type FileScope = "global" | "collection" | "environment";

export interface VarLayer {
  scope: FileScope;
  /** Collection or environment name (empty for globals). */
  label: string;
  /** Value in the file (undefined = not defined there). */
  file: string | undefined;
  /** Runtime value (undefined = none, null = unset by a script). */
  runtime: string | null | undefined;
  /** The value can be saved in this scope (a legacy collection environment cannot). */
  editable: boolean;
}

export interface VarInfo {
  name: string;
  dynamic: boolean;
  /** Effective value for a send (null = undefined). */
  value: string | null;
  /** Scope the effective value comes from. */
  source: FileScope | null;
  /** The effective value is a runtime (session) value. */
  fromRuntime: boolean;
  /** From highest to lowest precedence: environment, collection, global (only existing scopes). */
  layers: VarLayer[];
}

export function varInfo(collection: Collection | null, name: string): VarInfo {
  const env = collection ? ws.resolveEnv(collection) : null;
  const layers: VarLayer[] = [];
  if (env) {
    layers.push({
      scope: "environment",
      label: env.name,
      file: env.variables[name],
      runtime: runtimeVars.environmentOverrides(env.id)[name],
      editable: env.choice.scope !== "collection",
    });
  }
  if (collection) {
    layers.push({
      scope: "collection",
      label: collection.name,
      file: collection.variables[name],
      runtime: runtimeVars.collectionOverrides(collection.path)[name],
      editable: true,
    });
  }
  layers.push({
    scope: "global",
    label: "",
    file: enabledVars(envStore.globals)[name],
    runtime: runtimeVars.globals[name],
    editable: true,
  });

  for (const l of layers) {
    if (l.runtime === null) continue; // unset by a script: hides the file value of this scope
    const v = l.runtime ?? l.file;
    if (v !== undefined) {
      return { name, dynamic: false, value: v, source: l.scope, fromRuntime: l.runtime !== undefined, layers };
    }
  }
  return { name, dynamic: DYNAMIC_VARS.has(name), value: null, source: null, fromRuntime: false, layers };
}

/** "ok" (defined), "dynamic" ({{$guid}}…) or "missing": the colour of a variable in the URL bar. */
export function varStatus(collection: Collection | null, name: string): "ok" | "dynamic" | "missing" {
  const i = varInfo(collection, name);
  return i.value !== null ? "ok" : i.dynamic ? "dynamic" : "missing";
}

function upsert(list: EnvVariable[], name: string, value: string): EnvVariable[] {
  const i = list.findIndex((v) => v.key.trim() === name);
  if (i === -1) return [...list, { key: name, value, secret: false, enabled: true }];
  return list.map((v, j) => (j === i ? { ...v, value, enabled: true } : v));
}

function without(list: EnvVariable[], name: string): EnvVariable[] {
  return list.filter((v) => v.key.trim() !== name);
}

type Edit = { kind: "set"; value: string } | { kind: "remove" };

/**
 * Save `value` for `name` in `target` and remove it from `removeFrom` (to move it).
 * Clears the runtime value of the touched scopes so the saved value applies, then reloads.
 */
export async function saveVar(
  collection: Collection | null,
  name: string,
  value: string,
  target: FileScope,
  removeFrom: FileScope[] = [],
): Promise<void> {
  const edits = new Map<FileScope, Edit>();
  for (const s of removeFrom) if (s !== target) edits.set(s, { kind: "remove" });
  edits.set(target, { kind: "set", value });

  const env = collection ? ws.resolveEnv(collection) : null;
  const projectPath = collection ? (ws.projectOf(collection)?.path ?? null) : null;
  let reloadProject = false;
  let reloadGlobal = false;

  for (const [scope, edit] of edits) {
    const apply = (list: EnvVariable[]) => (edit.kind === "set" ? upsert(list, name, edit.value) : without(list, name));
    if (scope === "global") {
      await api.globalsSave(apply(await api.globalsGet()));
      reloadGlobal = true;
    } else if (scope === "collection") {
      if (!collection) throw new Error("no collection");
      const rec = { ...collection.variables };
      if (edit.kind === "set") rec[name] = edit.value;
      else delete rec[name];
      await api.saveCollectionVariables(collection.path, rec);
      reloadProject = true;
    } else {
      if (!env || env.choice.scope === "collection") throw new Error("no editable environment");
      const scopeKind = env.choice.scope;
      const owner = scopeKind === "project" ? projectPath : null;
      const doc = (await api.envList(scopeKind, owner)).find((d) => d.name === env.name);
      if (!doc) throw new Error(`environment ${env.name} not found`);
      await api.envSave(scopeKind, owner, { ...doc, variables: apply(doc.variables) }, doc.name);
      if (scopeKind === "project") reloadProject = true;
      else reloadGlobal = true;
    }
    runtimeVars.resetKey(scope, name, collection?.path ?? "", env?.id ?? null);
  }
  if (reloadGlobal) await envStore.refreshGlobal();
  if (reloadProject && projectPath) await ws.reload(projectPath);
}
