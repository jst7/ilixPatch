// Environments (D28): project and global environments, the per-project selection used to send
// requests, and the editing sessions behind the "Environments" tabs.
// Does not import the workspace store (the workspace imports this one).
import {
  api,
  type Collection,
  type EnvironmentDoc,
  type EnvScope,
  type EnvSelection,
  type EnvVariable,
  type Project,
} from "$lib/api";

function lsGet<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    return raw ? (JSON.parse(raw) as T) : fallback;
  } catch {
    return fallback;
  }
}

function lsSet(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // ignore
  }
}

const LS_SELECTION = "ilixpatch.envSelection";
const SEP = "\u0000";

/** What the environment selector can pick: a project/global environment or a legacy collection one. */
export type EnvChoice = EnvSelection | { scope: "collection"; name: string };

/** The environment that applies to a request, resolved against what exists. */
export interface ResolvedEnv {
  choice: EnvChoice;
  /** Identity of the environment (scope + owner + name); keys runtime overrides. */
  id: string;
  name: string;
  variables: Record<string, string>;
}

export function sameChoice(a: EnvChoice | null, b: EnvChoice | null): boolean {
  return (a?.scope ?? null) === (b?.scope ?? null) && (a?.name ?? null) === (b?.name ?? null);
}

/** Enabled variables with a key, as a plain record. */
export function enabledVars(vars: EnvVariable[]): Record<string, string> {
  const out: Record<string, string> = {};
  for (const v of vars) if (v.enabled && v.key.trim()) out[v.key.trim()] = v.value;
  return out;
}

class EnvStore {
  /** Global environments (user level). */
  globalEnvs = $state<EnvironmentDoc[]>([]);
  /** Global variables (pm.globals). */
  globals = $state<EnvVariable[]>([]);
  /** Selected environment by project path. */
  selection = $state<Record<string, EnvChoice | null>>(lsGet(LS_SELECTION, {}));

  globalVars = $derived(enabledVars(this.globals));

  /** Reload global environments and globals. Errors (backend not available yet) mean "none". */
  async refreshGlobal() {
    try {
      this.globalEnvs = await api.envList("global", null);
    } catch {
      this.globalEnvs = [];
    }
    try {
      this.globals = await api.globalsGet();
    } catch {
      this.globals = [];
    }
  }

  choiceFor(projectPath: string | null): EnvChoice | null {
    return projectPath ? (this.selection[projectPath] ?? null) : null;
  }

  setChoice(projectPath: string, choice: EnvChoice | null) {
    this.selection[projectPath] = choice ? { scope: choice.scope, name: choice.name } as EnvChoice : null;
    lsSet(LS_SELECTION, this.selection);
  }

  /** Resolve a choice for a request of `collection` in `project` (null when it no longer exists). */
  resolve(project: Project | null, collection: Collection | null, choice: EnvChoice | null): ResolvedEnv | null {
    if (!choice) return null;
    if (choice.scope === "project") {
      const env = project?.environments?.find((e) => e.name === choice.name);
      if (!project || !env) return null;
      return { choice, id: ["project", project.path, env.name].join(SEP), name: env.name, variables: { ...env.variables } };
    }
    if (choice.scope === "global") {
      const env = this.globalEnvs.find((e) => e.name === choice.name);
      if (!env) return null;
      return { choice, id: ["global", env.name].join(SEP), name: env.name, variables: enabledVars(env.variables) };
    }
    const env = collection?.environments.find((e) => e.name === choice.name);
    if (!collection || !env) return null;
    return {
      choice,
      id: ["collection", collection.path, env.name].join(SEP),
      name: env.name,
      variables: { ...env.variables },
    };
  }
}

export const envStore = new EnvStore();

// ---------- Editing sessions (one per Environments tab) ----------

export interface EnvRow {
  key: string;
  value: string;
  secret: boolean;
  enabled: boolean;
  /** UI only: show a secret value in clear text. */
  reveal?: boolean;
}

export interface EnvItem {
  /** Local identity (stable while the tab lives). */
  uid: number;
  /** Saved name; null for a new, unsaved environment. */
  original: string | null;
  /** Canonical form of the saved document ("" when unsaved). */
  saved: string;
  name: string;
  rows: EnvRow[];
  /** D49: UI colour and icon (null = default). */
  color: string | null;
  icon: string | null;
}

const emptyRow = (): EnvRow => ({ key: "", value: "", secret: false, enabled: true });

export function rowsFrom(vars: EnvVariable[]): EnvRow[] {
  return [...vars.map((v) => ({ key: v.key, value: v.value, secret: v.secret, enabled: v.enabled })), emptyRow()];
}

export function varsFrom(rows: EnvRow[]): EnvVariable[] {
  return rows
    .filter((r) => r.key.trim())
    .map((r) => ({ key: r.key.trim(), value: r.value, secret: r.secret, enabled: r.enabled }));
}

function canonical(name: string, vars: EnvVariable[], color: string | null = null, icon: string | null = null): string {
  return JSON.stringify([name.trim(), vars.map((v) => [v.key, v.value, v.secret, v.enabled]), color, icon]);
}

let nextUid = 1;

export class EnvSession {
  readonly scope: EnvScope;
  readonly project: string | null;
  items = $state<EnvItem[]>([]);
  /** Selected item uid, or GLOBALS for the global variables (global tab only). */
  selected = $state<number | null>(null);
  globals = $state<EnvRow[]>([emptyRow()]);
  globalsSaved = $state(canonical("", []));
  loading = $state(false);
  loaded = $state(false);
  error = $state<string | null>(null);
  saving = $state(false);
  /** Bumped to ask the editor to focus the name field (context menu "Rename"). */
  focusName = $state(0);

  static readonly GLOBALS = 0;

  constructor(project: string | null) {
    this.project = project;
    this.scope = project === null ? "global" : "project";
  }

  isItemDirty(item: EnvItem): boolean {
    return canonical(item.name, varsFrom(item.rows), item.color, item.icon) !== item.saved;
  }

  globalsDirty = $derived(canonical("", varsFrom(this.globals)) !== this.globalsSaved);
  get dirty(): boolean {
    return this.items.some((i) => this.isItemDirty(i)) || (this.scope === "global" && this.globalsDirty);
  }

  private itemFrom(doc: EnvironmentDoc, uid = nextUid++): EnvItem {
    return {
      uid,
      original: doc.name,
      saved: canonical(doc.name, doc.variables, doc.color ?? null, doc.icon ?? null),
      name: doc.name,
      rows: rowsFrom(doc.variables),
      color: doc.color ?? null,
      icon: doc.icon ?? null,
    };
  }

  /** Environment to select by name once loaded (opened from a link, D47). */
  pendingName: string | null = null;

  /** Select an environment by name now, or after the next load. */
  selectByName(name: string) {
    const it = this.items.find((i) => i.name === name);
    if (it) this.selected = it.uid;
    else this.pendingName = name;
  }

  async load() {
    this.loading = true;
    try {
      const docs = await api.envList(this.scope, this.project);
      this.items = docs.map((d) => this.itemFrom(d));
      if (this.scope === "global") {
        const globals = await api.globalsGet();
        this.globals = rowsFrom(globals);
        this.globalsSaved = canonical("", globals);
      }
      this.error = null;
    } catch (e) {
      this.error = String(e);
    } finally {
      this.loading = false;
      this.loaded = true;
    }
    if (this.pendingName !== null) {
      const it = this.items.find((i) => i.name === this.pendingName);
      this.pendingName = null;
      if (it) this.selected = it.uid;
    }
    if (this.selected === null || !this.exists(this.selected)) {
      this.selected = this.scope === "global" ? EnvSession.GLOBALS : (this.items[0]?.uid ?? null);
    }
  }

  private exists(uid: number): boolean {
    return (uid === EnvSession.GLOBALS && this.scope === "global") || this.items.some((i) => i.uid === uid);
  }

  item(uid: number | null): EnvItem | null {
    return this.items.find((i) => i.uid === uid) ?? null;
  }

  uniqueName(base: string): string {
    const taken = new Set(this.items.map((i) => i.name.trim()));
    if (!taken.has(base)) return base;
    for (let n = 2; ; n++) if (!taken.has(`${base} (${n})`)) return `${base} (${n})`;
  }

  add(name: string, rows: EnvRow[] = [emptyRow()]): EnvItem {
    const item: EnvItem = { uid: nextUid++, original: null, saved: "", name: this.uniqueName(name), rows, color: null, icon: null };
    this.items.push(item);
    this.selected = item.uid;
    return item;
  }

  duplicate(uid: number, suffix: string) {
    const src = this.item(uid);
    if (!src) return;
    this.add(`${src.name} ${suffix}`, rowsFrom(varsFrom(src.rows)));
  }

  /** Remove locally (after a successful delete, or for unsaved items). */
  removeLocal(uid: number) {
    const i = this.items.findIndex((x) => x.uid === uid);
    if (i < 0) return;
    this.items.splice(i, 1);
    if (this.selected === uid) {
      const next = this.items[i] ?? this.items[i - 1];
      this.selected = next?.uid ?? (this.scope === "global" ? EnvSession.GLOBALS : null);
    }
  }

  /** Name problem of an item ("required" / "taken"), or null. */
  nameError(item: EnvItem): "required" | "taken" | null {
    const name = item.name.trim();
    if (!name) return "required";
    if (this.items.some((i) => i !== item && i.name.trim() === name)) return "taken";
    return null;
  }

  /**
   * Save every dirty environment (and the globals). Returns an error message (first failure) or null.
   * Items with an invalid name are skipped and reported.
   */
  async saveAll(messages: { required: string; taken: string }): Promise<string | null> {
    if (this.saving) return null;
    this.saving = true;
    let error: string | null = null;
    try {
      for (const item of this.items) {
        if (!this.isItemDirty(item)) continue;
        const problem = this.nameError(item);
        if (problem) {
          error ??= problem === "required" ? messages.required : messages.taken;
          continue;
        }
        try {
          const doc = await api.envSave(
            this.scope,
            this.project,
            { name: item.name.trim(), variables: varsFrom(item.rows), color: item.color, icon: item.icon },
            item.original,
          );
          const reveal = new Set(item.rows.filter((r) => r.reveal).map((r) => r.key));
          item.original = doc.name;
          item.name = doc.name;
          item.color = doc.color ?? null;
          item.icon = doc.icon ?? null;
          item.saved = canonical(doc.name, doc.variables, item.color, item.icon);
          item.rows = rowsFrom(doc.variables).map((r) => (reveal.has(r.key) ? { ...r, reveal: true } : r));
        } catch (e) {
          error ??= String(e);
        }
      }
      if (this.scope === "global" && this.globalsDirty) {
        try {
          const vars = varsFrom(this.globals);
          await api.globalsSave(vars);
          this.globalsSaved = canonical("", vars);
        } catch (e) {
          error ??= String(e);
        }
      }
    } finally {
      this.saving = false;
    }
    return error;
  }
}

const sessions = new Map<string, EnvSession>();

export function envSessionKey(project: string | null): string {
  return `env:${project ?? "global"}`;
}

/** The editing session of an Environments tab (created on first use). */
export function envSession(project: string | null): EnvSession {
  const key = envSessionKey(project);
  let s = sessions.get(key);
  if (!s) {
    s = new EnvSession(project);
    sessions.set(key, s);
  }
  return s;
}

/** Existing session (no creation), e.g. for dirty checks. */
export function peekEnvSession(project: string | null): EnvSession | undefined {
  return sessions.get(envSessionKey(project));
}

/** Forget a session when its tab closes (unsaved edits are discarded). */
export function dropEnvSession(project: string | null) {
  sessions.delete(envSessionKey(project));
}
