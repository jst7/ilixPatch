// Editable settings (authorization, variables) of a collection or folder (D39), one session per
// directory so unsaved edits survive switching tabs. Saved with saveCollectionAuth / saveFolderAuth /
// saveCollectionVariables, then the project is reloaded.
import { api, type Auth, type Collection, type Folder, type ProjectEntry } from "$lib/api";
import { canonicalAuth, cleanAuth } from "$lib/auth";
import { ws, type KV } from "./workspace.svelte";

export interface ContainerInfo {
  kind: "collection" | "folder";
  projectPath: string;
  projectName: string;
  collection: Collection;
  /** null for a collection. */
  folder: Folder | null;
  /** Folders from the collection root down to (excluding) this folder's parent chain end — i.e. its ancestors. */
  ancestors: Folder[];
}

export function findContainer(entries: ProjectEntry[], path: string): ContainerInfo | null {
  for (const entry of entries) {
    const project = entry.project;
    if (!project) continue;
    for (const collection of project.collections) {
      const base = { projectPath: entry.path, projectName: project.name, collection };
      if (collection.path === path) return { ...base, kind: "collection", folder: null, ancestors: [] };
      const walk = (folder: Folder, chain: Folder[]): ContainerInfo | null => {
        for (const sub of folder.folders) {
          if (sub.path === path) return { ...base, kind: "folder", folder: sub, ancestors: chain };
          const found = walk(sub, [...chain, sub]);
          if (found) return found;
        }
        return null;
      };
      const found = walk(collection.root, []);
      if (found) return found;
    }
  }
  return null;
}

function rowsOf(rec: Record<string, string>): KV[] {
  return [...Object.entries(rec).map(([key, value]) => ({ key, value })), { key: "", value: "" }];
}

function recOf(rows: KV[]): Record<string, string> {
  const out: Record<string, string> = {};
  for (const { key, value } of rows) if (key.trim()) out[key.trim()] = value;
  return out;
}

const canonVars = (rec: Record<string, string>) => JSON.stringify(Object.entries(rec).sort(([a], [b]) => (a < b ? -1 : 1)));

/** Collections store "inherit" (absent) as no auth; show it as such. */
function displayAuth(kind: "collection" | "folder", a: Auth | undefined | null): Auth {
  const v = a ? (JSON.parse(JSON.stringify(a)) as Auth) : { type: "inherit" as const };
  if (kind === "collection" && v.type === "inherit") return { type: "no_auth" };
  return v;
}

export class ContainerSession {
  readonly path: string;
  auth = $state<Auth>({ type: "inherit" });
  variables = $state<KV[]>([{ key: "", value: "" }]);
  saving = $state(false);
  error = $state<string | null>(null);
  private origAuth = $state<string | null>(null);
  private origVars = $state("[]");
  private loaded = false;

  constructor(path: string) {
    this.path = path;
  }

  get info(): ContainerInfo | null {
    return findContainer(ws.entries, this.path);
  }

  /** Load from the tree (first time, or after a save/reload). */
  reset() {
    const info = this.info;
    if (!info) return;
    const auth = info.kind === "collection" ? info.collection.auth : info.folder!.auth;
    this.auth = displayAuth(info.kind, auth);
    this.origAuth = this.canonical(this.auth);
    const vars = info.kind === "collection" ? info.collection.variables : {};
    this.variables = rowsOf(vars);
    this.origVars = canonVars(vars);
    this.loaded = true;
  }

  ensureLoaded() {
    if (!this.loaded) this.reset();
  }

  private canonical(a: Auth): string | null {
    // For collections no_auth and inherit both mean "none".
    if (a.type === "no_auth" || a.type === "inherit") return this.info?.kind === "collection" ? null : canonicalAuth(a);
    return canonicalAuth(a);
  }

  get authDirty(): boolean {
    return this.loaded && this.canonical(this.auth) !== this.origAuth;
  }

  get varsDirty(): boolean {
    return this.loaded && this.info?.kind === "collection" && canonVars(recOf(this.variables)) !== this.origVars;
  }

  get dirty(): boolean {
    return this.authDirty || this.varsDirty;
  }

  revert() {
    this.reset();
    this.error = null;
  }

  async save() {
    const info = this.info;
    if (!info || this.saving || !this.dirty) return;
    this.saving = true;
    this.error = null;
    try {
      if (this.authDirty) {
        const a = this.auth;
        if (info.kind === "collection") {
          await api.saveCollectionAuth(this.path, a.type === "no_auth" || a.type === "inherit" ? null : cleanAuth(a));
        } else {
          await api.saveFolderAuth(this.path, a.type === "inherit" ? null : cleanAuth(a));
        }
      }
      if (this.varsDirty) await api.saveCollectionVariables(this.path, recOf(this.variables));
      await ws.reload(info.projectPath);
      this.reset();
    } catch (e) {
      this.error = String(e);
    } finally {
      this.saving = false;
    }
  }
}

const sessions = new Map<string, ContainerSession>();

export function containerSession(path: string): ContainerSession {
  let s = sessions.get(path);
  if (!s) sessions.set(path, (s = new ContainerSession(path)));
  return s;
}

export function dropContainerSession(path: string) {
  sessions.delete(path);
}
