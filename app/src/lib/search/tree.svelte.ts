// Project-tree search: a normalized index of every node (rebuilt only when projects load/reload,
// since it is derived from ws.entries), and the filter result for the current query.
import type { Folder, ProjectEntry, RequestFile } from "$lib/api";
import { ws } from "$lib/stores/workspace.svelte";
import { allTermsIn, isEmptyQuery, normalize, parseQuery, type Query } from "./match";

interface IndexNode {
  path: string;
  kind: "project" | "collection" | "folder" | "request";
  /** Normalized own name. */
  name: string;
  /** Normalized names of the ancestors (project / collection / folders), "\n"-joined. */
  ctx: string;
  /** Requests only. */
  method?: string;
  url?: string;
  file?: RequestFile;
  children: IndexNode[];
}

function buildIndex(entries: ProjectEntry[]): IndexNode[] {
  const roots: IndexNode[] = [];
  for (const entry of entries) {
    const project = entry.project;
    if (!project) continue;
    const pName = normalize(project.name);
    const root: IndexNode = { path: entry.path, kind: "project", name: pName, ctx: "", children: [] };
    roots.push(root);
    for (const c of project.collections) {
      const cName = normalize(c.name);
      const cNode: IndexNode = { path: c.path, kind: "collection", name: cName, ctx: pName, children: [] };
      root.children.push(cNode);
      const walk = (folder: Folder, into: IndexNode, ctx: string) => {
        for (const sub of folder.folders) {
          const fName = normalize(sub.name);
          const fNode: IndexNode = { path: sub.path, kind: "folder", name: fName, ctx, children: [] };
          into.children.push(fNode);
          walk(sub, fNode, `${ctx}\n${fName}`);
        }
        for (const file of folder.requests) {
          into.children.push({
            path: file.path,
            kind: "request",
            name: normalize(file.request.name),
            ctx,
            method: file.request.method.toLowerCase(),
            url: normalize(file.request.url),
            file,
            children: [],
          });
        }
      };
      walk(c.root, cNode, `${pName}\n${cName}`);
    }
  }
  return roots;
}

/** Normalized searchable content of a request (headers, query, body, scripts); cached per file object. */
const contentCache = new WeakMap<RequestFile, string>();
function contentOf(file: RequestFile): string {
  let c = contentCache.get(file);
  if (c === undefined) {
    const r = file.request;
    const body = r.body ? (r.body.type === "form" ? Object.entries(r.body.fields).flat().join("\n") : r.body.content) : "";
    c = normalize(
      [
        ...Object.entries(r.headers ?? {}).flat(),
        ...Object.entries(r.query ?? {}).flat(),
        body,
        r.scripts?.pre_request ?? "",
        r.scripts?.tests ?? "",
      ].join("\n"),
    );
    contentCache.set(file, c);
  }
  return c;
}

export interface TreeSearchResult {
  query: Query;
  /** Paths of the nodes to show (matches and their ancestors). */
  visible: Set<string>;
  /** Matching requests, in tree order. */
  requests: string[];
}

function run(index: IndexNode[], q: Query, content: boolean): TreeSearchResult {
  const visible = new Set<string>();
  const requests: string[] = [];
  const self = (n: IndexNode): boolean => {
    if (q.method !== null && (n.kind !== "request" || n.method !== q.method)) return false;
    if (q.within.length && !q.within.every((w) => n.ctx.includes(w))) return false;
    const hay = [n.name, n.ctx];
    if (n.kind === "request") {
      hay.push(n.url ?? "", n.method ?? "");
      if (content && n.file) hay.push(contentOf(n.file));
    }
    return allTermsIn(q.terms, hay);
  };
  const visit = (n: IndexNode): boolean => {
    let any = false;
    for (const c of n.children) if (visit(c)) any = true;
    const match = self(n);
    if (match && n.kind === "request") requests.push(n.path);
    if (match || any) {
      visible.add(n.path);
      return true;
    }
    return false;
  };
  for (const r of index) visit(r);
  return { query: q, visible, requests };
}

class TreeSearch {
  /** Text in the box (updates on every keystroke). */
  input = $state("");
  /** Debounced query actually applied. */
  query = $state("");
  /** Also search headers, query params, bodies and scripts. */
  content = $state(false);
  /** Containers the user collapsed while filtering (filtering expands everything that is shown). */
  collapsed = $state<Record<string, boolean>>({});

  index = $derived(buildIndex(ws.entries));
  parsed = $derived(parseQuery(this.query));
  result = $derived(isEmptyQuery(this.parsed) ? null : run(this.index, this.parsed, this.content));
  /** Normalized terms to highlight. */
  terms = $derived(this.result ? this.parsed.terms : []);

  private timer: ReturnType<typeof setTimeout> | undefined;

  setInput(v: string) {
    this.input = v;
    clearTimeout(this.timer);
    this.timer = setTimeout(() => this.apply(), 100);
  }

  apply() {
    clearTimeout(this.timer);
    if (this.query !== this.input) {
      this.query = this.input;
      this.collapsed = {};
    }
  }

  clear() {
    this.input = "";
    this.apply();
  }

  /** Expansion while filtering (independent from the persisted expansion state). */
  isExpanded(path: string): boolean {
    return !this.collapsed[path];
  }

  toggle(path: string) {
    this.collapsed[path] = !this.collapsed[path];
  }
}

export const treeSearch = new TreeSearch();
