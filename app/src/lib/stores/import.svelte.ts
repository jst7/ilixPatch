// State of the Postman import wizard (D31). One wizard at a time (single "import" tab); kept here so switching
// tabs doesn't lose the arrangement.
import {
  api,
  type EnvScope,
  type ImportPlacement,
  type ImportPlan,
  type ImportPreview,
  type ImportReport,
} from "$lib/api";
import { ws } from "./workspace.svelte";
import { envStore } from "./environments.svelte";
import {
  buildPlan,
  defaultPlacements,
  indexSource,
  place,
  rename,
  setChecked,
  summarize,
  unplace,
  validPattern,
  type DestKind,
  type SourceEntry,
} from "$lib/import/plan";

export const DEFAULT_SECRET_PATTERN = "token|secret|password|api[_-]?key|auth";

export type ImportStep = "source" | "arrange" | "report";

export interface ImportEnvChoice {
  checked: boolean;
  scope: EnvScope;
}

function errorText(e: unknown): string {
  return e instanceof Error ? e.message : typeof e === "string" ? e : JSON.stringify(e);
}

class ImportWizard {
  step = $state<ImportStep>("source");
  /** Files and folders chosen so far. */
  paths = $state<string[]>([]);
  loading = $state(false);
  error = $state<string | null>(null);
  preview = $state.raw<ImportPreview | null>(null);

  // Arrange
  project = $state<string | null>(null);
  excluded = $state.raw<Set<string>>(new Set());
  placements = $state.raw<ImportPlacement[]>([]);
  envs = $state<Record<string, ImportEnvChoice>>({});
  globals = $state<"skip" | "merge">("skip");
  onConflict = $state<ImportPlan["options"]["on_conflict"]>("rename");
  convertAuth = $state(true);
  keepScripts = $state(true);
  inlineParentScripts = $state(true);
  secretEnabled = $state(true);
  secretPattern = $state(DEFAULT_SECRET_PATTERN);
  /** Source node selected for the keyboard "Place into…" action / highlighted from a warning. */
  selected = $state<string | null>(null);
  /** Expanded source / ghost nodes. */
  expanded = $state<Record<string, boolean>>({});

  // Apply
  applying = $state(false);
  report = $state.raw<ImportReport | null>(null);

  index = $derived(indexSource(this.preview?.collections ?? []));
  summary = $derived(summarize(this.index, this.preview?.collections ?? [], this.excluded, this.placements));
  patternValid = $derived(!this.secretEnabled || validPattern(this.secretPattern));
  envCount = $derived(Object.values(this.envs).filter((e) => e.checked).length);
  canImport = $derived(
    !!this.preview &&
      !!this.project &&
      !this.applying &&
      this.patternValid &&
      (this.summary.collections + this.summary.folders + this.summary.requests > 0 ||
        this.envCount > 0 ||
        (this.globals === "merge" && !!this.preview.globals?.length)),
  );

  /** Open the wizard (single tab); `project` preselects the destination. */
  open(project: string | null = null) {
    if (project) this.setProject(project);
    ws.openTab({ kind: "import" });
  }

  reset() {
    this.step = "source";
    this.paths = [];
    this.preview = null;
    this.error = null;
    this.excluded = new Set();
    this.placements = [];
    this.envs = {};
    this.globals = "skip";
    this.selected = null;
    this.expanded = {};
    this.report = null;
  }

  async addPaths(paths: string[]) {
    const next = [...this.paths];
    for (const p of paths) if (!next.includes(p)) next.push(p);
    this.paths = next;
    await this.loadPreview();
  }

  removePath(path: string) {
    this.paths = this.paths.filter((p) => p !== path);
    void this.loadPreview();
  }

  private previewSeq = 0;

  async loadPreview() {
    const seq = ++this.previewSeq;
    if (this.paths.length === 0) {
      this.preview = null;
      this.error = null;
      return;
    }
    this.loading = true;
    try {
      const preview = await api.importPostmanPreview(this.paths);
      if (seq !== this.previewSeq) return;
      this.preview = preview;
      this.error = null;
      this.excluded = new Set();
      this.envs = Object.fromEntries(preview.environments.map((e) => [e.id, { checked: true, scope: "project" as EnvScope }]));
      this.globals = preview.globals?.length ? "merge" : "skip";
      this.expanded = Object.fromEntries(preview.collections.map((c) => [c.id, true]));
      this.resetPlacements();
    } catch (e) {
      if (seq !== this.previewSeq) return;
      this.preview = null;
      this.error = errorText(e);
    } finally {
      if (seq === this.previewSeq) this.loading = false;
    }
  }

  setProject(path: string | null) {
    if (path === this.project) return;
    this.project = path;
    this.resetPlacements();
  }

  resetPlacements() {
    this.placements = this.project && this.preview ? defaultPlacements(this.preview.collections, this.excluded, this.project) : [];
  }

  toggle(id: string, checked: boolean) {
    this.excluded = setChecked(this.index, this.excluded, id, checked);
    const entry = this.index.get(id);
    if (!entry) return;
    if (!checked) {
      // Unchecked nodes and their descendants lose their placements.
      const gone = new Set([id]);
      const walk = (n: typeof entry.node) => n.children.forEach((c) => (gone.add(c.id), walk(c)));
      walk(entry.node);
      this.placements = this.placements.filter((p) => !gone.has(p.source_id));
    } else if (entry.parent === null && this.project && !this.placements.some((p) => p.source_id === id)) {
      // Re-checking a top-level collection places it at the project root again.
      this.placements = [...this.placements, { source_id: id, target_parent: this.project, as: "collection", name: null }];
    }
  }

  /** Place a source node into a destination dir; returns false when the drop isn't allowed. */
  placeInto(sourceId: string, targetParent: string, destKind: DestKind): boolean {
    const next = place(this.index, this.placements, sourceId, targetParent, destKind);
    if (!next) return false;
    // Placing an unchecked node checks it.
    if (this.excluded.has(sourceId) || [...this.excluded].some((x) => this.isUnder(sourceId, x))) {
      this.excluded = setChecked(this.index, this.excluded, sourceId, true);
    }
    this.placements = next;
    return true;
  }

  private isUnder(id: string, ancestor: string): boolean {
    let p: SourceEntry | undefined = this.index.get(id);
    while (p?.parent) {
      if (p.parent === ancestor) return true;
      p = this.index.get(p.parent);
    }
    return false;
  }

  unplace(sourceId: string) {
    this.placements = unplace(this.placements, sourceId);
  }

  rename(sourceId: string, name: string) {
    this.placements = rename(this.placements, sourceId, name);
  }

  plan(): ImportPlan | null {
    if (!this.preview || !this.project) return null;
    return buildPlan({
      session: this.preview.session,
      projectPath: this.project,
      index: this.index,
      excluded: this.excluded,
      placements: this.placements,
      environments: Object.entries(this.envs)
        .filter(([, e]) => e.checked)
        .map(([source_id, e]) => ({ source_id, scope: e.scope, name: null })),
      globals: this.globals,
      options: {
        on_conflict: this.onConflict,
        convert_auth: this.convertAuth,
        keep_scripts: this.keepScripts,
        inline_parent_scripts: this.inlineParentScripts,
        secret_pattern: this.secretEnabled && this.secretPattern.trim() ? this.secretPattern.trim() : null,
      },
    });
  }

  async apply() {
    const plan = this.plan();
    if (!plan || !this.canImport) return;
    this.applying = true;
    this.error = null;
    try {
      this.report = await api.importPostmanApply(plan);
      this.step = "report";
      await ws.reload(plan.project_path);
      ws.setExpanded(plan.project_path, true);
      if (plan.environments.some((e) => e.scope === "global") || plan.globals === "merge") void envStore.refreshGlobal();
    } catch (e) {
      this.error = errorText(e);
    } finally {
      this.applying = false;
    }
  }
}

export const importer = new ImportWizard();
