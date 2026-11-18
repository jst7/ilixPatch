// State of one rich JSON view (D63): expanded nodes, selection, search and array paging.
import { SvelteMap, SvelteSet } from "svelte/reactivity";
import { ancestors, childPath, kindOf, search, type Json } from "./model";

export const PAGE = 100;

export class JsonViewState {
  expanded = new SvelteSet<string>([""]);
  /** Items shown per expanded array (paging of big arrays). */
  shown = new SvelteMap<string, number>();
  selected = $state<string | null>(null);
  query = $state("");
  matches = $state<string[]>([]);
  matchSet = $state(new Set<string>());
  current = $state(0);

  toggle(path: string) {
    if (this.expanded.has(path)) this.expanded.delete(path);
    else this.expanded.add(path);
  }

  /** Expand everything down to `depth` levels (Infinity = all), collapse the rest. */
  expandTo(root: Json, depth: number) {
    this.expanded.clear();
    const walk = (v: Json, path: string, d: number) => {
      if (d >= depth) return;
      if (Array.isArray(v)) {
        this.expanded.add(path);
        v.slice(0, this.limit(path)).forEach((c, i) => walk(c, childPath(path, i), d + 1));
      } else if (v && typeof v === "object") {
        this.expanded.add(path);
        for (const [k, c] of Object.entries(v)) walk(c, childPath(path, k), d + 1);
      }
    };
    walk(root, "", 0);
  }

  collapseAll() {
    this.expanded.clear();
    this.expanded.add("");
  }

  limit(path: string): number {
    return this.shown.get(path) ?? PAGE;
  }

  showMore(path: string) {
    this.shown.set(path, this.limit(path) + PAGE);
  }

  runSearch(root: Json) {
    this.matches = search(root, this.query);
    this.matchSet = new Set(this.matches);
    this.current = 0;
    if (this.matches.length) this.reveal(root, this.matches[0]);
  }

  step(root: Json, delta: number) {
    if (!this.matches.length) return;
    this.current = (this.current + delta + this.matches.length) % this.matches.length;
    this.reveal(root, this.matches[this.current]);
  }

  /** Expand the ancestors of `path` (and page arrays far enough to show it), then select it. */
  reveal(root: Json, path: string) {
    for (const a of ancestors(path)) this.expanded.add(a);
    // Arrays: make sure the index on the path is within the shown page.
    const re = /\[(\d+)\]/g;
    let m: RegExpExecArray | null;
    while ((m = re.exec(path))) {
      const parent = path.slice(0, m.index);
      const idx = Number(m[1]);
      if (idx >= this.limit(parent)) this.shown.set(parent, Math.ceil((idx + 1) / PAGE) * PAGE);
    }
    this.selected = path;
    void root;
  }
}

/** The value at `path` (same syntax as childPath). */
export function valueAt(root: Json, path: string): Json | undefined {
  if (!path) return root;
  const re = /(?:^|\.)([A-Za-z_$][\w$]*)|\[(\d+)\]|\[("(?:[^"\\]|\\.)*")\]/g;
  let v: Json | undefined = root;
  let m: RegExpExecArray | null;
  while ((m = re.exec(path)) && v !== undefined) {
    const key: string | number = m[1] ?? (m[2] !== undefined ? Number(m[2]) : (JSON.parse(m[3]) as string));
    if (typeof key === "number") v = Array.isArray(v) ? v[key] : undefined;
    else v = v && typeof v === "object" && !Array.isArray(v) ? (v as Record<string, Json>)[key] : undefined;
  }
  return v;
}

export { kindOf };
