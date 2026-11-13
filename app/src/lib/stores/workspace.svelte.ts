// Central UI state: registered projects, tree expansion, selection, per-request drafts and responses.
import {
  api,
  type Extract,
  type Collection,
  type Folder,
  type HttpMethod,
  type ProjectEntry,
  type Project,
  type VarUpdate,
  type Request,
  type RequestFile,
  type Scripts,
  type HttpVersionPref,
  type RequestSettings,
  type Auth,
  type GitRepoInfo,
  type HistoryEntry,
  type SendResult,
  type RequestError,
  type SendVariables,
} from "$lib/api";
import { ask } from "@tauri-apps/plugin-dialog";
import { t as translate } from "$lib/i18n/index.svelte";
import { runtimeVars } from "./runtimeVars.svelte";
import { tabGroups } from "./tabGroups.svelte";
import { toRequestError } from "$lib/requestError";
import { history } from "./history.svelte";
import { dropEnvSession, envSession, envStore, type EnvChoice, type ResolvedEnv } from "./environments.svelte";
import { canonicalAuth, cleanAuth, effectiveAuth, type EffectiveAuth } from "$lib/auth";
import { containerSession, dropContainerSession } from "./containerSettings.svelte";

// ---------- localStorage helpers (may be unavailable / throw) ----------

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

const LS_EXPANDED = "ilixpatch.expanded";
const LS_ENVS = "ilixpatch.envByCollection";
const LS_SELECTED = "ilixpatch.selected";

// ---------- Drafts ----------

export interface KV {
  key: string;
  value: string;
}

export type BodyMode = "none" | "json" | "text" | "form";

/** Editable form of a Request. Key/value lists always end with one empty row. */
export interface Draft {
  name: string;
  method: HttpMethod;
  url: string;
  query: KV[];
  headers: KV[];
  bodyMode: BodyMode;
  /** Content for json/text bodies (kept when switching between modes). */
  bodyContent: string;
  /** Preserved for "form" bodies, which the UI does not edit. */
  formFields: Record<string, string>;
  /** Pre-request / tests JavaScript ("" = none). */
  preRequest: string;
  tests: string;
  /** Per-request network overrides (null = use the global setting). */
  settings: DraftSettings;
  /** Authorization (D39); { type: "inherit" } = use the folder/collection auth. */
  auth: Auth;
  /** Variables from the response (D48). */
  extract: Extract[];
}

export interface DraftSettings {
  verify_ssl: boolean | null;
  follow_redirects: boolean | null;
  max_redirects: number | null;
  timeout_ms: number | null;
  http_version: HttpVersionPref | null;
}

function draftSettings(s: RequestSettings | undefined): DraftSettings {
  return {
    verify_ssl: s?.verify_ssl ?? null,
    follow_redirects: s?.follow_redirects ?? null,
    max_redirects: s?.max_redirects ?? null,
    timeout_ms: s?.timeout_ms ?? null,
    http_version: s?.http_version ?? null,
  };
}

/** Only the overrides that are set; undefined when none (the field is then omitted). */
function requestSettings(s: DraftSettings | RequestSettings | undefined): RequestSettings | undefined {
  if (!s) return undefined;
  const out: RequestSettings = {};
  for (const [k, v] of Object.entries(s)) if (v !== null && v !== undefined) (out as Record<string, unknown>)[k] = v;
  return Object.keys(out).length ? out : undefined;
}

function toRows(rec: Record<string, string> | undefined): KV[] {
  return [...Object.entries(rec ?? {}).map(([key, value]) => ({ key, value })), { key: "", value: "" }];
}

function fromRows(rows: KV[]): Record<string, string> {
  const out: Record<string, string> = {};
  for (const { key, value } of rows) {
    const k = key.trim();
    if (k) out[k] = value;
  }
  return out;
}

export function draftFromRequest(r: Request): Draft {
  const body = r.body ?? null;
  return {
    name: r.name,
    method: r.method,
    url: r.url,
    query: toRows(r.query),
    headers: toRows(r.headers),
    bodyMode: body ? body.type : "none",
    bodyContent: body && body.type !== "form" ? body.content : "",
    formFields: body && body.type === "form" ? { ...body.fields } : {},
    preRequest: r.scripts?.pre_request ?? "",
    tests: r.scripts?.tests ?? "",
    settings: draftSettings(r.settings),
    auth: r.auth ? (JSON.parse(JSON.stringify(r.auth)) as Auth) : { type: "inherit" },
    extract: (r.extract ?? []).map((e) => ({ ...e, path: e.path ?? "", enabled: e.enabled ?? true })),
  };
}

export function requestFromDraft(d: Draft): Request {
  let body: Request["body"] = null;
  if (d.bodyMode === "json" || d.bodyMode === "text") body = { type: d.bodyMode, content: d.bodyContent };
  else if (d.bodyMode === "form") body = { type: "form", fields: { ...d.formFields } };
  return {
    name: d.name,
    method: d.method,
    url: d.url,
    headers: fromRows(d.headers),
    query: fromRows(d.query),
    body,
    ...scriptsFromDraft(d),
    ...(requestSettings(d.settings) ? { settings: requestSettings(d.settings) } : {}),
    ...(d.auth && d.auth.type !== "inherit" ? { auth: cleanAuth(d.auth) } : {}),
    ...(cleanExtract(d.extract).length ? { extract: cleanExtract(d.extract) } : {}),
  };
}

/** Empty scripts are omitted entirely (matches the backend, which skips empty values). */
function scriptsFromDraft(d: Draft): Pick<Request, "scripts"> {
  const pre = d.preRequest.trim() ? d.preRequest : null;
  const tests = d.tests.trim() ? d.tests : null;
  if (pre === null && tests === null) return {};
  const scripts: Scripts = {};
  if (pre !== null) scripts.pre_request = pre;
  if (tests !== null) scripts.tests = tests;
  return { scripts };
}

function sortRec(rec: Record<string, string> | undefined): [string, string][] {
  return Object.entries(rec ?? {}).sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0));
}

/** Order-insensitive serialization used for dirty checks (backend omits empty maps/body). */
function canonical(r: Request): string {
  const b = r.body ?? null;
  return JSON.stringify([
    r.name,
    r.method,
    r.url,
    sortRec(r.headers),
    sortRec(r.query),
    b === null ? null : b.type === "form" ? ["form", sortRec(b.fields)] : [b.type, b.content],
    r.scripts?.pre_request?.trim() ? r.scripts.pre_request : null,
    r.scripts?.tests?.trim() ? r.scripts.tests : null,
    Object.entries(requestSettings(r.settings) ?? {}).sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0)),
    canonicalAuth(r.auth),
    cleanExtract(r.extract ?? []).map((e) => [e.var, e.from, e.path, e.scope, e.enabled]),
  ]);
}

/** Rules worth saving: with a variable name (the path is dropped where it is unused). */
function cleanExtract(list: Extract[] | undefined): Extract[] {
  return (list ?? [])
    .filter((e) => e.var.trim())
    .map((e) => ({
      var: e.var.trim(),
      from: e.from,
      path: e.from === "status" || e.from === "body" ? "" : e.path,
      scope: e.scope,
      enabled: e.enabled,
    }));
}

// ---------- Tree index ----------

export interface RequestLocation {
  projectPath: string;
  projectName: string;
  collection: Collection;
  /** Folders between the collection root and the request (excluding root). */
  folders: Folder[];
  file: RequestFile;
}

function buildIndex(entries: ProjectEntry[]): Map<string, RequestLocation> {
  const map = new Map<string, RequestLocation>();
  for (const entry of entries) {
    if (!entry.project) continue;
    for (const collection of entry.project.collections) {
      const walk = (folder: Folder, chain: Folder[]) => {
        for (const file of folder.requests) {
          map.set(file.path, {
            projectPath: entry.path,
            projectName: entry.project!.name,
            collection,
            folders: chain,
            file,
          });
        }
        for (const sub of folder.folders) walk(sub, [...chain, sub]);
      };
      walk(collection.root, []);
    }
  }
  return map;
}

/** True when `path` is `root` or lies inside it (handles / and \ separators). */
export function isWithin(path: string, root: string): boolean {
  return path === root || path.startsWith(root + "/") || path.startsWith(root + "\\");
}

/** Any node of the tree (project, collection, folder or request), keyed by path. */
export interface NodeInfo {
  kind: "project" | "collection" | "folder" | "request";
  name: string;
  projectPath: string;
  /** Names of the containing collection/folders, e.g. "Users / Admin" (empty for projects). */
  hint: string;
}

function buildNodes(entries: ProjectEntry[]): Map<string, NodeInfo> {
  const map = new Map<string, NodeInfo>();
  for (const entry of entries) {
    if (!entry.project) continue;
    const projectPath = entry.path;
    map.set(projectPath, { kind: "project", name: entry.project.name, projectPath, hint: "" });
    for (const collection of entry.project.collections) {
      map.set(collection.path, { kind: "collection", name: collection.name, projectPath, hint: entry.project.name });
      const walk = (folder: Folder, chain: string[]) => {
        const hint = chain.join(" / ");
        for (const file of folder.requests) {
          map.set(file.path, { kind: "request", name: file.request.name, projectPath, hint });
        }
        for (const sub of folder.folders) {
          map.set(sub.path, { kind: "folder", name: sub.name, projectPath, hint });
          walk(sub, [...chain, sub.name]);
        }
      };
      walk(collection.root, [collection.name]);
    }
  }
  return map;
}

// ---------- Open tabs ----------

/** Something open in the main area, listed in the right-hand tabs sidebar. */
export type OpenTab =
  | { kind: "request"; path: string }
  | { kind: "runner"; target: string }
  | { kind: "load"; target: string }
  | { kind: "git"; repo: string }
  | { kind: "history"; id: number }
  /** Environments of a project, or the global ones (project = null) (D28). */
  | { kind: "environments"; project: string | null }
  /** 3-way merge resolver for one conflicted file (D24). */
  | { kind: "merge"; repo: string; path: string }
  /** Postman import wizard (D31); single instance, not restored on restart. */
  | { kind: "import" }
  /** Settings (authorization, variables) of a collection or folder directory (D39). */
  | { kind: "container"; path: string }
  /** A request that is not in any project yet (D55); saved to a collection on demand. */
  | { kind: "scratch"; id: string };

/** Key of a history tab; also the key of its in-memory draft and response (not a file path). */
export function historyKey(id: number): string {
  return `history:${id}`;
}

/** Key of an unsaved request tab (D55); also the key of its draft and response. */
export function scratchKey(id: string): string {
  return `scratch:${id}`;
}

/** Key of the in-memory draft of a tab that has no request file (history or unsaved request). */
export function draftOnlyKey(tab: OpenTab): string | null {
  if (tab.kind === "history") return historyKey(tab.id);
  if (tab.kind === "scratch") return scratchKey(tab.id);
  return null;
}

/** Stable identity of a tab (a request/target can only be open once per kind). */
export function tabKey(tab: OpenTab): string {
  if (tab.kind === "request") return `request:${tab.path}`;
  if (tab.kind === "history") return historyKey(tab.id);
  if (tab.kind === "scratch") return scratchKey(tab.id);
  if (tab.kind === "environments") return `env:${tab.project ?? "global"}`;
  if (tab.kind === "git") return `git:${tab.repo}`;
  if (tab.kind === "merge") return `merge:${tab.path}`;
  if (tab.kind === "import") return "import";
  if (tab.kind === "container") return `container:${tab.path}`;
  return `${tab.kind}:${tab.target}`;
}

/** Filesystem path a tab refers to (history tabs have none: their key is returned instead). */
export function tabPath(tab: OpenTab): string {
  if (tab.kind === "history") return historyKey(tab.id);
  if (tab.kind === "scratch") return scratchKey(tab.id);
  if (tab.kind === "environments") return tab.project ?? "env:global";
  if (tab.kind === "merge") return tab.path;
  if (tab.kind === "import") return "import";
  if (tab.kind === "container") return tab.path;
  return tab.kind === "request" ? tab.path : tab.kind === "git" ? tab.repo : tab.target;
}

/** State of an open history tab. */
export interface HistoryTabState {
  /** null while loading (or if loading failed). */
  entry: HistoryEntry | null;
  error: string | null;
  /** True once re-sent: the response shown is the live one, not the stored one. */
  live: boolean;
  /** Canonical form of the stored request, for the unsaved-changes check. */
  original: string;
}

function responseFromHistory(entry: HistoryEntry): ResponseState {
  const r = entry.response;
  if (!r) return { loading: false, result: null, error: entry.error ? toRequestError(entry.error) : null };
  return {
    loading: false,
    error: null,
    result: {
      status: r.status,
      status_text: r.status_text,
      headers: r.headers,
      elapsed_ms: entry.elapsed_ms,
      size_bytes: entry.size_bytes,
      body: r.body,
      tests: entry.tests ?? [],
      logs: entry.logs ?? [],
      script_error: null,
      variable_updates: [],
      network: null,
      body_handle: null,
    },
  };
}

const LS_TABS = "ilixpatch.tabs";
/** Drafts of unsaved request tabs (D55), by id: they survive a restart like the tabs themselves. */
const LS_SCRATCH = "ilixpatch.scratch";

/** An empty request for a new unsaved tab. */
function blankRequest(name: string): Request {
  return { name, method: "GET", url: "", headers: {}, query: {}, body: null };
}

/** Free a response body kept by the backend (D41), unless `next` still uses the same body. */
function releaseBody(old: SendResult | null | undefined, next: SendResult | null = null) {
  const id = old?.body_handle?.id;
  if (id && id !== next?.body_handle?.id) void api.responseRelease(id).catch(() => {});
}

// ---------- Workspace ----------

export type CreateKind = "collection" | "folder" | "request";

export interface PendingCreate {
  kind: CreateKind;
  /** Project path (for collections) or collection/folder directory. */
  parentPath: string;
  projectPath: string;
}

export interface MenuItem {
  label: string;
  action: () => void;
  danger?: boolean;
}

export interface MenuState {
  x: number;
  y: number;
  items: MenuItem[];
}

export interface ResponseState {
  loading: boolean;
  result: SendResult | null;
  /** Descriptive error of the last send (D36). */
  error: RequestError | null;
  /** Id of the send in progress (for cancelRequest). */
  requestId?: string | null;
  /** Date.now() when the send started (for the elapsed timer). */
  startedAt?: number | null;
  /** The last send was cancelled by the user. */
  cancelled?: boolean;
}


class Workspace {
  entries = $state<ProjectEntry[]>([]);
  loaded = $state(false);
  /** Open tabs, in opening (or user-reordered) order. */
  tabs = $state<OpenTab[]>([]);
  /** `tabKey` of the active tab. */
  activeKey = $state<string | null>(null);
  /** Path (in the tree) to scroll into view and focus; consumed by the tree row. */
  revealPath = $state<string | null>(null);
  /** Git repositories found in each project (by project path); empty when git is unavailable. */
  repos = $state<Record<string, GitRepoInfo[]>>({});
  /** Open history tabs by entry id. */
  historyTabs = $state<Record<number, HistoryTabState>>({});
  /** Unsaved request tabs (D55): canonical form of the request they started from, to tell if they changed. */
  scratchTabs = $state<Record<string, { original: string }>>({});
  /** Set to a draft key (history / unsaved tab) to ask its header to open the "Save to collection" picker (Cmd/Ctrl+S). */
  saveAsRequest = $state<string | null>(null);
  drafts = $state<Record<string, Draft>>({});
  responses = $state<Record<string, ResponseState>>({});
  expanded = $state<Record<string, boolean>>(lsGet(LS_EXPANDED, {}));
  envByCollection = $state<Record<string, string>>(lsGet(LS_ENVS, {}));
  pendingCreate = $state<PendingCreate | null>(null);
  menu = $state.raw<MenuState | null>(null);
  notice = $state<string | null>(null);
  saving = $state(false);

  index = $derived(buildIndex(this.entries));
  nodes = $derived(buildNodes(this.entries));
  /** Git repo info keyed by repo root path. */
  repoByRoot = $derived(new Map(Object.values(this.repos).flat().map((r) => [r.root, r] as const)));
  activeTab = $derived(this.tabs.find((tab) => tabKey(tab) === this.activeKey) ?? null);
  /** Path of the request shown in the main area (the active tab, when it is a request). */
  selectedPath = $derived(this.activeTab?.kind === "request" ? this.activeTab.path : null);
  selected =$derived(this.selectedPath ? (this.index.get(this.selectedPath) ?? null) : null);

  // ----- notices -----
  private noticeTimer: ReturnType<typeof setTimeout> | undefined;

  fail(e: unknown) {
    this.notice = String(e);
    clearTimeout(this.noticeTimer);
    this.noticeTimer = setTimeout(() => (this.notice = null), 8000);
  }

  // ----- expansion -----
  isExpanded(path: string, fallback = false): boolean {
    return this.expanded[path] ?? fallback;
  }

  setExpanded(path: string, value: boolean) {
    this.expanded[path] = value;
    lsSet(LS_EXPANDED, this.expanded);
  }

  toggle(path: string, fallback = false) {
    this.setExpanded(path, !this.isExpanded(path, fallback));
  }

  // ----- environments (D28) -----
  /** Project that contains a collection. */
  projectOf(collection: Collection): Project | null {
    const p = this.nodes.get(collection.path)?.projectPath;
    return (p && this.entries.find((e) => e.path === p)?.project) || null;
  }

  /** Environment chosen for a project (persisted per project). */
  envChoice(projectPath: string | null): EnvChoice | null {
    return envStore.choiceFor(projectPath);
  }

  setEnvChoice(projectPath: string, choice: EnvChoice | null) {
    envStore.setChoice(projectPath, choice);
  }

  /** The environment that applies to requests of `collection` (null = none / no longer exists). */
  resolveEnv(collection: Collection | null): ResolvedEnv | null {
    // D55: requests without a collection (unsaved tabs, history of deleted requests) use global environments.
    if (!collection) return envStore.resolve(null, null, envStore.noProjectChoice);
    const project = this.projectOf(collection);
    let choice = this.envChoice(project?.path ?? null);
    // Before D28 the selection was per collection (legacy collection environments).
    if (!choice && project && !(project.path in envStore.selection)) {
      const legacy = this.envByCollection[collection.path];
      if (legacy) choice = { scope: "collection", name: legacy };
    }
    return envStore.resolve(project, collection, choice);
  }

  /** Name of the environment that applies to `collection` ("" = none). Kept for older callers. */
  envFor(collection: Collection): string {
    return this.resolveEnv(collection)?.name ?? "";
  }

  /** Legacy: select a collection environment by name ("" = none) for the collection's project. */
  setEnv(collectionPath: string, name: string) {
    const p = this.nodes.get(collectionPath)?.projectPath;
    if (p) this.setEnvChoice(p, name ? { scope: "collection", name } : null);
  }

  /** Store the variable writes of a send in the runtime layer (environment writes go to the selected env). */
  applyVarUpdates(collection: Collection | null, updates: VarUpdate[]) {
    runtimeVars.apply(collection?.path ?? "", this.resolveEnv(collection)?.id ?? null, updates);
  }

  /** Effective variables (file values + runtime overrides), flattened: globals < collection < environment. */
  variablesFor(collection: Collection | null): Record<string, string> {
    const v = this.sendVariablesFor(collection);
    return { ...v.globals, ...v.collection, ...v.environment };
  }

  /** Scoped variables for a send, with the runtime layer applied (runtime value wins over file value). */
  sendVariablesFor(collection: Collection | null): SendVariables {
    const env = this.resolveEnv(collection);
    return runtimeVars.sendVariables(
      collection?.path ?? "",
      collection?.variables ?? {},
      env?.id ?? null,
      env?.name ?? null,
      env?.variables ?? {},
      envStore.globalVars,
    );
  }

  // ----- drafts / dirty -----
  isDirty(path: string): boolean {
    const draft = this.drafts[path];
    const loc = this.index.get(path);
    if (!draft || !loc) return false;
    return canonical(requestFromDraft(draft)) !== canonical(loc.file.request);
  }

  private ensureDraft(path: string) {
    const loc = this.index.get(path);
    if (loc && !this.drafts[path]) this.drafts[path] = draftFromRequest(loc.file.request);
  }

  /** Throw away unsaved changes of a request. */
  revert(path: string) {
    const loc = this.index.get(path);
    if (loc) this.drafts[path] = draftFromRequest(loc.file.request);
  }

  /** Open (or activate) a request in a tab. */
  select(path: string) {
    this.openTab({ kind: "request", path });
  }

  // ----- tabs -----
  private persistTabs() {
    lsSet(LS_TABS, { tabs: this.tabs, active: this.activeKey });
  }

  /** Open `tab` (appending it if it is not open yet) and make it active. */
  openTab(tab: OpenTab) {
    const key = tabKey(tab);
    if (!this.tabs.some((t) => tabKey(t) === key)) this.tabs.push(tab);
    if (tab.kind === "request") this.ensureDraft(tab.path);
    if (tab.kind === "history" && !this.historyTabs[tab.id]) void this.loadHistoryTab(tab.id, false);
    this.activeKey = key;
    this.persistTabs();
  }

  activateTab(key: string) {
    const tab = this.tabs.find((t) => tabKey(t) === key);
    if (!tab) return;
    if (tab.kind === "request") this.ensureDraft(tab.path);
    if (tab.kind === "history" && !this.historyTabs[tab.id]) void this.loadHistoryTab(tab.id, false);
    this.activeKey = key;
    this.persistTabs();
  }

  /** Activate the next (+1) or previous (-1) tab, wrapping around. */
  cycleTab(step: 1 | -1) {
    const n = this.tabs.length;
    if (n === 0) return;
    const i = this.tabs.findIndex((t) => tabKey(t) === this.activeKey);
    const next = i < 0 ? 0 : (i + step + n) % n;
    this.activateTab(tabKey(this.tabs[next]));
  }

  /** Move the tab at index `from` so it ends up at index `to`. */
  moveTab(from: number, to: number) {
    if (from === to || from < 0 || from >= this.tabs.length) return;
    const [tab] = this.tabs.splice(from, 1);
    this.tabs.splice(Math.max(0, Math.min(to, this.tabs.length)), 0, tab);
    this.persistTabs();
  }

  isTabDirty(tab: OpenTab): boolean {
    if (tab.kind === "history") return this.isHistoryDirty(tab.id);
    if (tab.kind === "scratch") return this.isScratchDirty(tab.id);
    if (tab.kind === "environments") return envSession(tab.project).dirty;
    if (tab.kind === "container") return containerSession(tab.path).dirty;
    return tab.kind === "request" && this.isDirty(tab.path);
  }

  /** Forget a response and free its body in the backend store (D41). */
  private dropResponse(key: string) {
    const r = this.responses[key];
    if (!r) return;
    if (r.loading && r.requestId) void api.cancelRequest(r.requestId).catch(() => {});
    releaseBody(r.result);
    delete this.responses[key];
  }

  /** Remove tabs without asking; if the active one goes, activate its nearest remaining neighbour. */
  private dropTabs(keys: Set<string>) {
    if (keys.size === 0) return;
    // D50: a closed tab leaves its folder.
    for (const k of keys) tabGroups.assign(k, null);
    const activeIndex = this.tabs.findIndex((t) => tabKey(t) === this.activeKey);
    const activeGoes = this.activeKey !== null && keys.has(this.activeKey);
    for (const tab of this.tabs) {
      // Closing a request tab discards its draft (the caller already confirmed) and frees its body (D41).
      if (tab.kind === "request" && keys.has(tabKey(tab))) {
        delete this.drafts[tab.path];
        this.dropResponse(tab.path);
      }
      if (tab.kind === "environments" && keys.has(tabKey(tab))) dropEnvSession(tab.project);
      if (tab.kind === "container" && keys.has(tabKey(tab))) dropContainerSession(tab.path);
      if (tab.kind === "history" && keys.has(tabKey(tab))) {
        const key = historyKey(tab.id);
        delete this.drafts[key];
        this.dropResponse(key);
        delete this.historyTabs[tab.id];
      }
      if (tab.kind === "scratch" && keys.has(tabKey(tab))) {
        const key = scratchKey(tab.id);
        delete this.drafts[key];
        this.dropResponse(key);
        delete this.scratchTabs[tab.id];
      }
    }
    const before = this.tabs;
    this.tabs = before.filter((t) => !keys.has(tabKey(t)));
    if (before.some((t) => t.kind === "scratch" && keys.has(tabKey(t)))) this.persistScratch();
    if (activeGoes || (this.activeKey && !this.tabs.some((t) => tabKey(t) === this.activeKey))) {
      // Prefer the first surviving tab after the closed one, else the last one before it.
      const after = before.slice(activeIndex + 1).find((t) => !keys.has(tabKey(t)));
      const prev = before
        .slice(0, Math.max(activeIndex, 0))
        .reverse()
        .find((t) => !keys.has(tabKey(t)));
      const next = after ?? prev ?? null;
      this.activeKey = next ? tabKey(next) : null;
      if (next?.kind === "request") this.ensureDraft(next.path);
    }
    this.persistTabs();
  }

  /**
   * Close tabs by key. Dirty request tabs need confirmation (one native dialog for all of them);
   * if the user cancels, only the clean tabs are closed. Returns true if everything was closed.
   */
  async closeTabs(keys: string[]): Promise<boolean> {
    const targets = this.tabs.filter((t) => keys.includes(tabKey(t)));
    if (targets.length === 0) return true;
    const dirty = targets.filter((t) => this.isTabDirty(t));
    let discard = true;
    if (dirty.length > 0) {
      const first = dirty[0];
      const name =
        first.kind === "environments"
          ? first.project === null
            ? translate("env.globalTitle")
            : translate("env.projectTitle", { name: this.nodes.get(first.project)?.name ?? first.project })
          : first.kind === "container"
            ? (this.nodes.get(first.path)?.name ?? "")
            : (this.drafts[tabPath(first)]?.name ?? "");
      try {
        discard = await ask(
          dirty.length === 1 ? translate("tabs.discardOne", { name }) : translate("tabs.discardMany", { count: dirty.length }),
          { title: translate("tabs.discardTitle"), kind: "warning", okLabel: translate("tabs.discard"), cancelLabel: translate("tabs.cancel") },
        );
      } catch (e) {
        this.fail(e);
        discard = false;
      }
    }
    const close = new Set(targets.filter((t) => discard || !this.isTabDirty(t)).map(tabKey));
    this.dropTabs(close);
    return close.size === targets.length;
  }

  /** Close one tab (default: the active one). */
  closeTab(key: string | null = this.activeKey): Promise<boolean> {
    return key ? this.closeTabs([key]) : Promise.resolve(true);
  }

  closeOtherTabs(key: string) {
    return this.closeTabs(this.tabs.map(tabKey).filter((k) => k !== key));
  }

  closeAllTabs() {
    return this.closeTabs(this.tabs.map(tabKey));
  }

  /** Close every tab without unsaved changes. */
  closeSavedTabs() {
    this.dropTabs(new Set(this.tabs.filter((t) => !this.isTabDirty(t)).map(tabKey)));
  }

  /** Close tabs whose request/target no longer exists (inside `root`, if given). */
  private pruneTabs(root?: string) {
    const gone = this.tabs.filter((t) => {
      if (t.kind === "history" || t.kind === "scratch" || t.kind === "merge" || t.kind === "import") return false;
      if (t.kind === "environments" && t.project === null) return false;
      const p = tabPath(t);
      return (!root || isWithin(p, root)) && !this.nodes.has(p);
    });
    this.dropTabs(new Set(gone.map(tabKey)));
  }

  /** Expand every ancestor of `path` in the tree and ask its row to scroll into view. */
  reveal(path: string) {
    for (const [p, node] of this.nodes) {
      if (node.kind !== "request" && p !== path && isWithin(path, p)) this.setExpanded(p, true);
    }
    this.revealPath = path;
  }

  // ----- history -----
  /** Reload the history list (call after each send). */
  refreshHistory() {
    return history.refresh();
  }

  /**
   * Auth used when sending the draft at `key` (request path or history key): its own auth, or the
   * nearest folder/collection auth when it inherits (collection "inherit" = no auth).
   */
  effectiveAuthFor(key: string): EffectiveAuth {
    const own = this.drafts[key]?.auth ?? null;
    let loc = this.index.get(key) ?? null;
    if (!loc && key.startsWith("history:")) {
      const p = this.historyTabs[Number(key.slice(8))]?.entry?.request_path;
      loc = (p && this.index.get(p)) || null;
    }
    return effectiveAuth(own, loc?.collection ?? null, loc?.folders ?? []);
  }

  /** Open the settings tab (authorization, variables) of a collection or folder directory. */
  openContainer(path: string) {
    this.openTab({ kind: "container", path });
  }

  /** Open the Environments tab of a project, or the global one (null). */
  openEnvironments(project: string | null) {
    this.openTab({ kind: "environments", project });
  }

  /** Open a history entry in its own tab (re-focusing it if that entry is already open). */
  openHistory(id: number) {
    this.openTab({ kind: "history", id });
  }

  private async loadHistoryTab(id: number, restoring: boolean) {
    const key = historyKey(id);
    this.historyTabs[id] = { entry: null, error: null, live: false, original: "" };
    try {
      const entry = await api.historyGet(id);
      if (!this.tabs.some((t) => tabKey(t) === key)) return; // closed meanwhile
      this.drafts[key] = draftFromRequest(entry.request);
      this.responses[key] = responseFromHistory(entry);
      this.historyTabs[id] = { entry, error: null, live: false, original: canonical(entry.request) };
    } catch (e) {
      if (restoring) this.dropTabs(new Set([key]));
      else if (this.historyTabs[id]) this.historyTabs[id].error = String(e);
    }
  }

  isHistoryDirty(id: number): boolean {
    const st = this.historyTabs[id];
    const draft = this.drafts[historyKey(id)];
    return !!st?.entry && !!draft && canonical(requestFromDraft(draft)) !== st.original;
  }

  /** Folder (or collection) of the request a history entry came from, if it still exists: where to save it. */
  historyDefaultTarget(id: number): string | null {
    const p = this.historyTabs[id]?.entry?.request_path;
    const loc = p ? this.index.get(p) : undefined;
    return loc ? (loc.folders.at(-1)?.path ?? loc.collection.path) : null;
  }

  /** Collection a history entry came from, if its request file still exists. */
  historyCollection(id: number): Collection | null {
    const p = this.historyTabs[id]?.entry?.request_path;
    return (p && this.index.get(p)?.collection) || null;
  }

  /** Close the tabs of deleted history entries (no confirmation: the entries are gone). */
  closeHistoryTabs(ids: number[] | "all") {
    const keys = this.tabs
      .filter((t) => t.kind === "history" && (ids === "all" || ids.includes(t.id)))
      .map(tabKey);
    this.dropTabs(new Set(keys));
  }

  /** Send the (possibly edited) request of a history tab; the live response replaces the stored one. */
  async sendHistory(id: number) {
    const key = historyKey(id);
    const st = this.historyTabs[id];
    const draft = this.drafts[key];
    if (!st?.entry || !draft || this.responses[key]?.loading) return;
    const collection = this.historyCollection(id);
    const variables = this.sendVariablesFor(collection);
    const context = {
      project_path: st.entry.project_path,
      request_path: st.entry.request_path,
      environment: variables.environment_name,
    };
    const requestId = crypto.randomUUID();
    const previous = this.responses[key]?.result ?? null;
    this.responses[key] = { loading: true, result: previous, error: null, requestId, startedAt: Date.now() };
    try {
      const request = requestFromDraft(draft);
      request.auth = cleanAuth(this.effectiveAuthFor(key).auth);
      const result = await api.sendRequest(request, variables, context, requestId);
      this.applyVarUpdates(collection, result.variable_updates ?? []);
      releaseBody(previous, result);
      this.responses[key] = { loading: false, result, error: null };
    } catch (e) {
      const err = toRequestError(e);
      if (err.kind !== "cancelled") releaseBody(previous);
      this.responses[key] =
        err.kind === "cancelled"
          ? { loading: false, result: previous, error: null, cancelled: true }
          : { loading: false, result: null, error: err };
    }
    if (this.historyTabs[id]) this.historyTabs[id].live = true;
    // History list refreshes on the backend's `history://recorded` event.
  }

  /**
   * Save a history tab's draft as a new request file in `folderPath` (collection or folder dir),
   * then turn the tab into a normal request tab for that file. Returns true on success.
   */
  saveHistoryAs(id: number, folderPath: string, projectPath: string, name: string): Promise<boolean> {
    return this.saveDraftAs(historyKey(id), folderPath, projectPath, name, () => delete this.historyTabs[id]);
  }

  /** Save an unsaved request tab (D55) as a new request file, like {@link saveHistoryAs}. */
  async saveScratchAs(id: string, folderPath: string, projectPath: string, name: string): Promise<boolean> {
    const ok = await this.saveDraftAs(scratchKey(id), folderPath, projectPath, name, () => delete this.scratchTabs[id]);
    if (ok) this.persistScratch();
    return ok;
  }

  /**
   * Write the draft at `key` (a tab without a file) as a new request in `folderPath`, then turn its tab
   * into a normal request tab for that file, keeping the response. `forget` drops the tab's own state.
   */
  private async saveDraftAs(
    key: string,
    folderPath: string,
    projectPath: string,
    name: string,
    forget: () => void,
  ): Promise<boolean> {
    const draft = this.drafts[key];
    name = name.trim();
    if (!draft || !name) return false;
    try {
      const file = await api.createRequest(folderPath, name);
      await api.saveRequest(file.path, requestFromDraft({ ...draft, name }));
      await this.reload(projectPath);
      if (!this.index.has(file.path)) return true;
      const tab: OpenTab = { kind: "request", path: file.path };
      const response = this.responses[key];
      delete this.drafts[key];
      delete this.responses[key];
      forget();
      if (response) this.responses[file.path] = response;
      const i = this.tabs.findIndex((t) => tabKey(t) === key);
      if (i >= 0) this.tabs[i] = tab;
      else this.tabs.push(tab);
      this.ensureDraft(file.path);
      if (this.activeKey === key || i < 0) this.activeKey = tabKey(tab);
      this.persistTabs();
      return true;
    } catch (e) {
      this.fail(e);
      await this.reload(projectPath);
      return false;
    }
  }

  // ----- unsaved requests (D55) -----
  /** Open a new, empty request that belongs to no project yet. */
  newScratch() {
    const id = crypto.randomUUID().slice(0, 8);
    const request = blankRequest(translate("scratch.defaultName"));
    this.drafts[scratchKey(id)] = draftFromRequest(request);
    this.scratchTabs[id] = { original: canonical(request) };
    this.openTab({ kind: "scratch", id });
    this.persistScratch();
  }

  /** Changed since it was opened (closing it then asks for confirmation). */
  isScratchDirty(id: string): boolean {
    const draft = this.drafts[scratchKey(id)];
    const st = this.scratchTabs[id];
    return !!draft && !!st && canonical(requestFromDraft(draft)) !== st.original;
  }

  /** Store the drafts of all unsaved request tabs (called on changes, debounced by the page). */
  persistScratch() {
    const out: Record<string, Request> = {};
    for (const tab of this.tabs) {
      if (tab.kind !== "scratch") continue;
      const draft = this.drafts[scratchKey(tab.id)];
      if (draft) out[tab.id] = requestFromDraft(draft);
    }
    lsSet(LS_SCRATCH, out);
  }

  /** Send an unsaved request with the environment chosen for requests without a project. */
  async sendScratch(id: string) {
    const key = scratchKey(id);
    const draft = this.drafts[key];
    if (!draft || this.responses[key]?.loading) return;
    const variables = this.sendVariablesFor(null);
    const context = { project_path: null, request_path: null, environment: variables.environment_name };
    const requestId = crypto.randomUUID();
    const previous = this.responses[key]?.result ?? null;
    this.responses[key] = { loading: true, result: previous, error: null, requestId, startedAt: Date.now() };
    try {
      const request = requestFromDraft(draft);
      request.auth = cleanAuth(this.effectiveAuthFor(key).auth);
      const result = await api.sendRequest(request, variables, context, requestId);
      this.applyVarUpdates(null, result.variable_updates ?? []);
      releaseBody(previous, result);
      this.responses[key] = { loading: false, result, error: null };
    } catch (e) {
      const err = toRequestError(e);
      if (err.kind !== "cancelled") releaseBody(previous);
      this.responses[key] =
        err.kind === "cancelled"
          ? { loading: false, result: previous, error: null, cancelled: true }
          : { loading: false, result: null, error: err };
    }
    this.persistScratch();
  }

  /** Folder or collection of the most recently used request tab: where an unsaved request is offered to go. */
  lastRequestFolder(): string | null {
    for (let i = this.tabs.length - 1; i >= 0; i--) {
      const tab = this.tabs[i];
      if (tab.kind !== "request") continue;
      const loc = this.index.get(tab.path);
      if (loc) return loc.folders.at(-1)?.path ?? loc.collection.path;
    }
    return null;
  }

  // ----- git -----
  private reposRefreshedAt = 0;

  /** Re-read the git repos of one project. Errors (git missing, command not available) mean "no repos". */
  async refreshRepos(projectPath: string) {
    const entry = this.entries.find((e) => e.path === projectPath);
    if (!entry?.project) {
      delete this.repos[projectPath];
      return;
    }
    try {
      this.repos[projectPath] = await api.gitRepos(projectPath);
    } catch {
      this.repos[projectPath] = [];
    }
  }

  /** Refresh repos of every project; unless `force`, at most once every few seconds (e.g. on window focus). */
  async refreshAllRepos(force = false) {
    const now = Date.now();
    if (!force && now - this.reposRefreshedAt < 5000) return;
    this.reposRefreshedAt = now;
    await Promise.all(this.entries.map((e) => this.refreshRepos(e.path)));
  }

  /** Open the 3-way merge resolver for a conflicted file of `repo` (absolute paths). */
  openMerge(repo: string, path: string) {
    this.openTab({ kind: "merge", repo, path });
  }

  /** `git init` in `dir` (a project, collection or folder directory). */
  async initGit(dir: string, projectPath: string) {
    try {
      await api.gitInit(dir);
    } catch (e) {
      this.fail(e);
    }
    await this.refreshRepos(projectPath);
  }

  // ----- loading -----
  async init() {
    try {
      this.entries = await api.listProjects();
    } catch (e) {
      this.fail(e);
    }
    this.loaded = true;
    void this.refreshAllRepos(true);
    void envStore.refreshGlobal();
    const saved = lsGet<{ tabs?: OpenTab[]; active?: string | null } | null>(LS_TABS, null);
    if (saved && Array.isArray(saved.tabs)) {
      const valid = saved.tabs.filter(
        (t) =>
          t &&
          ((t.kind === "request" && typeof t.path === "string") ||
            ((t.kind === "runner" || t.kind === "load") && typeof t.target === "string") ||
            (t.kind === "container" && typeof t.path === "string") ||
            (t.kind === "git" && typeof t.repo === "string") ||
            (t.kind === "merge" && typeof t.repo === "string" && typeof t.path === "string") ||
            (t.kind === "history" && typeof t.id === "number") ||
            (t.kind === "scratch" && typeof t.id === "string") ||
            (t.kind === "environments" && (t.project === null || typeof t.project === "string"))) &&
          // Git tabs may point at a repo enclosing the project (not a tree node); GitView handles missing repos.
          (t.kind === "history" ||
            t.kind === "scratch" ||
            t.kind === "git" ||
            t.kind === "merge" ||
            (t.kind === "environments" && t.project === null) ||
            this.nodes.has(tabPath(t))),
      );
      const seen = new Set<string>();
      this.tabs = valid.filter((t) => !seen.has(tabKey(t)) && !!seen.add(tabKey(t)));
      // History tabs are restored by id; the ones whose entry no longer exists are dropped.
      for (const tab of this.tabs) if (tab.kind === "history") void this.loadHistoryTab(tab.id, true);
      // Unsaved request tabs get their stored draft back; without one there is nothing to restore.
      const stored = lsGet<Record<string, Request>>(LS_SCRATCH, {});
      this.tabs = this.tabs.filter((tab) => tab.kind !== "scratch" || !!stored[tab.id]);
      for (const tab of this.tabs) {
        if (tab.kind !== "scratch") continue;
        this.drafts[scratchKey(tab.id)] = draftFromRequest(stored[tab.id]);
        this.scratchTabs[tab.id] = { original: canonical(blankRequest(stored[tab.id].name)) };
      }
      const active = this.tabs.find((t) => tabKey(t) === saved.active) ?? this.tabs[0];
      if (active) this.activateTab(tabKey(active));
      else this.persistTabs();
    } else {
      // Migrate the single selection used before tabs existed.
      const last = lsGet<string | null>(LS_SELECTED, null);
      if (last && this.index.has(last)) this.select(last);
    }
  }

  /** Re-read one project from disk; refreshes clean drafts and drops drafts of deleted files. */
  async reload(projectPath: string) {
    const i = this.entries.findIndex((e) => e.path === projectPath);
    if (i < 0) return;
    const draftPaths = Object.keys(this.drafts).filter((p) => isWithin(p, projectPath));
    const clean = new Set(draftPaths.filter((p) => !this.isDirty(p)));
    try {
      const project = await api.reloadProject(projectPath);
      this.entries[i] = { path: projectPath, project, error: null };
    } catch (e) {
      this.entries[i] = { path: projectPath, project: null, error: String(e) };
    }
    for (const p of draftPaths) {
      const loc = this.index.get(p);
      if (!loc) delete this.drafts[p];
      else if (clean.has(p)) this.drafts[p] = draftFromRequest(loc.file.request);
    }
    for (const p of Object.keys(this.responses)) {
      if (isWithin(p, projectPath) && !this.index.has(p)) this.dropResponse(p);
    }
    this.pruneTabs(projectPath);
    void this.refreshRepos(projectPath);
    for (const tab of this.tabs) {
      if (tab.kind === "request" && isWithin(tab.path, projectPath)) this.ensureDraft(tab.path);
    }
  }

  // ----- project mutations -----
  async addProject(path: string) {
    try {
      await api.addProject(path);
      this.entries = await api.listProjects();
      this.setExpanded(path, true);
      void this.refreshRepos(path);
    } catch (e) {
      this.fail(e);
    }
  }

  async removeProject(path: string) {
    try {
      await api.removeProject(path);
      this.entries = this.entries.filter((e) => e.path !== path);
      for (const p of Object.keys(this.drafts)) if (isWithin(p, path)) delete this.drafts[p];
      for (const p of Object.keys(this.responses)) if (isWithin(p, path)) this.dropResponse(p);
      this.pruneTabs(path);
      delete this.repos[path];
    } catch (e) {
      this.fail(e);
    }
  }

  startCreate(kind: CreateKind, parentPath: string, projectPath: string) {
    this.setExpanded(parentPath, true);
    this.pendingCreate = { kind, parentPath, projectPath };
  }

  async commitCreate(name: string) {
    const pending = this.pendingCreate;
    this.pendingCreate = null;
    name = name.trim();
    if (!pending || !name) return;
    try {
      if (pending.kind === "collection") {
        const dir = await api.createCollection(pending.parentPath, name);
        await this.reload(pending.projectPath);
        this.setExpanded(dir, true);
      } else if (pending.kind === "folder") {
        const dir = await api.createFolder(pending.parentPath, name);
        await this.reload(pending.projectPath);
        this.setExpanded(dir, true);
      } else {
        const file = await api.createRequest(pending.parentPath, name);
        await this.reload(pending.projectPath);
        if (this.index.has(file.path)) this.select(file.path);
      }
    } catch (e) {
      this.fail(e);
      await this.reload(pending.projectPath);
    }
  }

  async deleteItem(path: string, projectPath: string) {
    try {
      await api.deleteItem(path);
    } catch (e) {
      this.fail(e);
    }
    await this.reload(projectPath);
  }

  // ----- request actions -----
  async save(path: string | null = this.selectedPath) {
    if (!path || this.saving) return;
    const loc = this.index.get(path);
    const draft = this.drafts[path];
    if (!loc || !draft || !this.isDirty(path)) return;
    this.saving = true;
    try {
      await api.saveRequest(path, requestFromDraft(draft));
      await this.reload(loc.projectPath);
    } catch (e) {
      this.fail(e);
    } finally {
      this.saving = false;
    }
  }

  /** Abort the send in progress for a request/history key (no-op when nothing is running). */
  cancelSend(key: string | null = this.activeKey ? this.activeSendKey() : null) {
    const id = key ? this.responses[key]?.requestId : null;
    if (key && id && this.responses[key]?.loading) void api.cancelRequest(id).catch(() => {});
  }

  /** Response key of the active tab (request path or history key), if it can send. */
  activeSendKey(): string | null {
    const tab = this.activeTab;
    if (!tab) return null;
    if (tab.kind === "request") return tab.path;
    if (tab.kind === "history") return historyKey(tab.id);
    if (tab.kind === "scratch") return scratchKey(tab.id);
    return null;
  }

  async send(path: string | null = this.selectedPath) {
    if (!path) return;
    const loc = this.index.get(path);
    const draft = this.drafts[path];
    if (!loc || !draft || this.responses[path]?.loading) return;
    const request = requestFromDraft(draft);
    request.auth = cleanAuth(this.effectiveAuthFor(path).auth);
    const variables = this.sendVariablesFor(loc.collection);
    const requestId = crypto.randomUUID();
    const previous = this.responses[path]?.result ?? null;
    this.responses[path] = { loading: true, result: previous, error: null, requestId, startedAt: Date.now() };
    const context = {
      project_path: loc.projectPath,
      request_path: loc.file.path,
      environment: variables.environment_name,
    };
    try {
      const result = await api.sendRequest(request, variables, context, requestId);
      this.applyVarUpdates(loc.collection, result.variable_updates ?? []);
      releaseBody(previous, result);
      this.responses[path] = { loading: false, result, error: null };
    } catch (e) {
      // A cancelled send keeps the previous response on screen and shows a notice.
      const err = toRequestError(e);
      if (err.kind !== "cancelled") releaseBody(previous);
      this.responses[path] =
        err.kind === "cancelled"
          ? { loading: false, result: previous, error: null, cancelled: true }
          : { loading: false, result: null, error: err };
    }
    // The backend records every send (also failed ones) and emits `history://recorded`;
    // the listener in +page.svelte refreshes the history list.
  }
}

export const ws = new Workspace();
