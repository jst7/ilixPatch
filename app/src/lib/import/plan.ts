// Pure logic of the Postman import wizard (D31): source index, check/exclusion rules, placements, drop validation,
// summary and the ImportPlan. No Svelte or Tauri imports so `selftest.mjs` can run it under plain Node.
import type { EnvScope, ImportNode, ImportPlacement, ImportPlan } from "../api";

export type CheckState = "all" | "some" | "none";
export type DestKind = "project" | "collection" | "folder";
export type PlaceAs = ImportPlacement["as"];

export interface SourceEntry {
  node: ImportNode;
  parent: string | null;
  depth: number;
}

/** Every source node by id, with its parent. */
export function indexSource(collections: ImportNode[]): Map<string, SourceEntry> {
  const map = new Map<string, SourceEntry>();
  const walk = (node: ImportNode, parent: string | null, depth: number) => {
    map.set(node.id, { node, parent, depth });
    for (const c of node.children) walk(c, node.id, depth + 1);
  };
  for (const c of collections) walk(c, null, 0);
  return map;
}

/** Ids of the ancestors of `id`, nearest first. */
export function ancestors(index: Map<string, SourceEntry>, id: string): string[] {
  const out: string[] = [];
  let p = index.get(id)?.parent ?? null;
  while (p) {
    out.push(p);
    p = index.get(p)?.parent ?? null;
  }
  return out;
}

export function descendants(node: ImportNode): string[] {
  const out: string[] = [];
  const walk = (n: ImportNode) => {
    for (const c of n.children) {
      out.push(c.id);
      walk(c);
    }
  };
  walk(node);
  return out;
}

/** True when `id` or one of its ancestors is unchecked. */
export function isExcluded(index: Map<string, SourceEntry>, excluded: ReadonlySet<string>, id: string): boolean {
  return excluded.has(id) || ancestors(index, id).some((a) => excluded.has(a));
}

/** Tri-state of a checkbox: a container is "all" when every descendant is checked. */
export function checkState(index: Map<string, SourceEntry>, excluded: ReadonlySet<string>, id: string): CheckState {
  if (isExcluded(index, excluded, id)) return "none";
  const node = index.get(id)?.node;
  if (!node || node.children.length === 0) return "all";
  const inner = (n: ImportNode): CheckState => {
    if (excluded.has(n.id)) return "none";
    if (n.children.length === 0) return "all";
    const states = n.children.map(inner);
    if (states.every((s) => s === "all")) return "all";
    if (states.every((s) => s === "none")) return "none";
    return "some";
  };
  return inner(node);
}

/**
 * Check or uncheck a node. Checking a node under an unchecked ancestor re-checks the ancestors but keeps
 * their other children unchecked. Returns the new excluded set (minimal: descendants of excluded ids omitted).
 */
export function setChecked(index: Map<string, SourceEntry>, excluded: ReadonlySet<string>, id: string, checked: boolean): Set<string> {
  const out = new Set(excluded);
  const entry = index.get(id);
  if (!entry) return out;
  for (const d of descendants(entry.node)) out.delete(d);
  if (!checked) {
    out.add(id);
    return out;
  }
  out.delete(id);
  // If an ancestor is unchecked, re-check every ancestor up to the highest unchecked one, keeping their
  // other children (off the path to `id`) unchecked.
  const chain = ancestors(index, id);
  let top = -1;
  chain.forEach((a, i) => {
    if (out.has(a)) top = i;
  });
  let child = id;
  for (let i = 0; i <= top; i++) {
    const a = chain[i];
    out.delete(a);
    for (const c of index.get(a)!.node.children) if (c.id !== child) out.add(c.id);
    child = a;
  }
  return out;
}

/** How `kind` lands in a destination of kind `dest`; null when not allowed. */
export function dropAs(kind: ImportNode["kind"], dest: DestKind): PlaceAs | null {
  if (dest === "project") return kind === "request" ? null : "collection";
  return kind === "request" ? "request" : "folder";
}

/** The nearest placed ancestor of `id` (the placement that already covers it), if any. */
export function coveringPlacement(index: Map<string, SourceEntry>, placements: ImportPlacement[], id: string): ImportPlacement | null {
  for (const a of ancestors(index, id)) {
    const p = placements.find((x) => x.source_id === a);
    if (p) return p;
  }
  return null;
}

/**
 * Place a source node into a destination dir. A node is placed at most once (re-placing moves it, keeping its
 * rename). Placing a node already covered by a placed ancestor moves it out: it becomes its own placement and
 * the ancestor's copy skips it (see `placedSubtree`). Returns null when the drop isn't allowed.
 */
export function place(
  index: Map<string, SourceEntry>,
  placements: ImportPlacement[],
  sourceId: string,
  targetParent: string,
  destKind: DestKind,
): ImportPlacement[] | null {
  const entry = index.get(sourceId);
  if (!entry) return null;
  const as = dropAs(entry.node.kind, destKind);
  if (!as) return null;
  const prev = placements.find((p) => p.source_id === sourceId);
  const next = placements.filter((p) => p.source_id !== sourceId);
  next.push({ source_id: sourceId, target_parent: targetParent, as, name: prev?.name ?? null });
  return next;
}

export function unplace(placements: ImportPlacement[], sourceId: string): ImportPlacement[] {
  return placements.filter((p) => p.source_id !== sourceId);
}

export function rename(placements: ImportPlacement[], sourceId: string, name: string): ImportPlacement[] {
  const n = name.trim();
  return placements.map((p) => (p.source_id === sourceId ? { ...p, name: n ? n : null } : p));
}

/** Default arrangement: every checked collection as a new collection at the project root. */
export function defaultPlacements(collections: ImportNode[], excluded: ReadonlySet<string>, projectPath: string): ImportPlacement[] {
  return collections
    .filter((c) => !excluded.has(c.id))
    .map((c) => ({ source_id: c.id, target_parent: projectPath, as: "collection" as const, name: null }));
}

/** Children of a placed node that are imported with it (checked and not placed elsewhere). */
export function placedChildren(node: ImportNode, excluded: ReadonlySet<string>, placed: ReadonlySet<string>): ImportNode[] {
  return node.children.filter((c) => !excluded.has(c.id) && !placed.has(c.id));
}

export interface Summary {
  collections: number;
  folders: number;
  requests: number;
  /** Checked nodes that no placement covers (they won't be imported). */
  unplaced: number;
}

export function summarize(
  index: Map<string, SourceEntry>,
  collections: ImportNode[],
  excluded: ReadonlySet<string>,
  placements: ImportPlacement[],
): Summary {
  const s: Summary = { collections: 0, folders: 0, requests: 0, unplaced: 0 };
  const placed = new Set(placements.filter((p) => !isExcluded(index, excluded, p.source_id)).map((p) => p.source_id));
  const count = (n: ImportNode, as: PlaceAs | null) => {
    const kind = as ?? n.kind;
    if (kind === "collection") s.collections++;
    else if (kind === "folder") s.folders++;
    else s.requests++;
    for (const c of placedChildren(n, excluded, placed)) count(c, c.kind === "request" ? "request" : "folder");
  };
  for (const p of placements) {
    if (!placed.has(p.source_id)) continue;
    count(index.get(p.source_id)!.node, p.as);
  }
  // Unplaced: checked nodes whose nearest placement-or-self chain has none.
  const walk = (n: ImportNode, covered: boolean) => {
    if (excluded.has(n.id)) return;
    const here = covered || placed.has(n.id);
    if (!here && n.kind === "request") s.unplaced++;
    for (const c of n.children) walk(c, here);
  };
  for (const c of collections) walk(c, false);
  return s;
}

/** Placements whose source is unchecked are dropped from the plan. */
export function buildPlan(args: {
  session: string;
  projectPath: string;
  index: Map<string, SourceEntry>;
  excluded: ReadonlySet<string>;
  placements: ImportPlacement[];
  environments: { source_id: string; scope: EnvScope; name: string | null }[];
  globals: "skip" | "merge";
  options: ImportPlan["options"];
}): ImportPlan {
  const { index, excluded } = args;
  return {
    session: args.session,
    project_path: args.projectPath,
    placements: args.placements.filter((p) => index.has(p.source_id) && !isExcluded(index, excluded, p.source_id)),
    excluded: [...excluded].filter((id) => index.has(id)),
    environments: args.environments,
    globals: args.globals,
    options: args.options,
  };
}

/** Valid case-insensitive regex (or empty). */
export function validPattern(p: string): boolean {
  try {
    new RegExp(p, "i");
    return true;
  } catch {
    return false;
  }
}
