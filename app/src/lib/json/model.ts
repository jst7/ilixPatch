// JSON helpers shared by the rich viewer (D63) and the response comparator (D64).
// Paths use the same syntax as response variables (D48): data.items[0]["a.b"].

export type Json = null | boolean | number | string | Json[] | { [k: string]: Json };
export type JsonKind = "null" | "boolean" | "number" | "string" | "array" | "object";

export function kindOf(v: Json): JsonKind {
  if (v === null) return "null";
  if (Array.isArray(v)) return "array";
  return typeof v as "boolean" | "number" | "string" | "object";
}

const IDENT = /^[A-Za-z_$][\w$]*$/;

/** Child path: `a.b`, `a[0]`, `a["x.y"]`. */
export function childPath(parent: string, key: string | number): string {
  if (typeof key === "number") return `${parent}[${key}]`;
  if (IDENT.test(key)) return parent ? `${parent}.${key}` : key;
  return `${parent}[${JSON.stringify(key)}]`;
}

/** One-line preview of a value (collapsed nodes, table cells). */
export function summary(v: Json, max = 60): string {
  const cut = (s: string) => (s.length > max ? s.slice(0, max - 1) + "…" : s);
  switch (kindOf(v)) {
    case "array":
      return `[${(v as Json[]).length}]`;
    case "object": {
      const keys = Object.keys(v as object);
      return cut(`{${keys.slice(0, 4).join(", ")}${keys.length > 4 ? ", …" : ""}}`);
    }
    case "string":
      return cut(JSON.stringify(v));
    default:
      return String(v);
  }
}

/** Columns for showing an array as a table: arrays of objects (≥ 1), union of keys in order of appearance. */
export function tableColumns(v: Json): string[] | null {
  if (!Array.isArray(v) || v.length === 0) return null;
  if (!v.every((x) => kindOf(x) === "object")) return null;
  const cols: string[] = [];
  const seen = new Set<string>();
  for (const row of v as { [k: string]: Json }[]) {
    for (const k of Object.keys(row)) {
      if (!seen.has(k)) {
        seen.add(k);
        cols.push(k);
      }
    }
    if (cols.length > 60) break;
  }
  return cols;
}

/** Stats of a document: nodes, depth. */
export function stats(v: Json): { nodes: number; depth: number } {
  let nodes = 0;
  let depth = 0;
  const walk = (x: Json, d: number) => {
    nodes++;
    depth = Math.max(depth, d);
    if (Array.isArray(x)) for (const c of x) walk(c, d + 1);
    else if (x && typeof x === "object") for (const c of Object.values(x)) walk(c, d + 1);
  };
  walk(v, 0);
  return { nodes, depth };
}

/** Paths of the nodes whose key or (scalar) value contains `term` (case-insensitive). */
export function search(v: Json, term: string, limit = 2000): string[] {
  const q = term.trim().toLowerCase();
  if (!q) return [];
  const out: string[] = [];
  const walk = (x: Json, path: string, key: string | null) => {
    if (out.length >= limit) return;
    const scalar = x === null || typeof x !== "object";
    if ((key !== null && key.toLowerCase().includes(q)) || (scalar && String(x).toLowerCase().includes(q))) out.push(path);
    if (Array.isArray(x)) x.forEach((c, i) => walk(c, childPath(path, i), null));
    else if (x && typeof x === "object") for (const [k, c] of Object.entries(x)) walk(c, childPath(path, k), k);
  };
  walk(v, "", null);
  return out;
}

/** Every ancestor path of `path` (for auto-expanding search results): "a.b[0].c" → ["", "a", "a.b", "a.b[0]"]. */
export function ancestors(path: string): string[] {
  const out = [""];
  const re = /(\.?[A-Za-z_$][\w$]*|\[[^\]]*\])/g;
  let acc = "";
  const parts = path.match(re) ?? [];
  for (let i = 0; i < parts.length - 1; i++) {
    acc += parts[i];
    out.push(acc.startsWith(".") ? acc.slice(1) : acc);
  }
  return out;
}

// ---------- comparison (D64) ----------

export interface NormalizeOptions {
  /** Object keys in alphabetical order. */
  sortKeys: boolean;
  /** Array elements in a canonical order (order differences disappear). */
  sortArrays: boolean;
  /** Trim strings and collapse inner whitespace. */
  trimStrings: boolean;
  /** Paths to drop before comparing; `*` matches any key or index (e.g. `*.updatedAt`, `meta.requestId`, `items[*].id`). */
  ignore: string[];
}

function canonical(v: Json): string {
  return JSON.stringify(v);
}

/** `*.x` = x at any depth; `[*]` = any index; `*` = any key. */
function globToRegex(glob: string): RegExp {
  let g = glob.trim();
  const anyDepth = g.startsWith("*.");
  if (anyDepth) g = g.slice(2);
  let re = "";
  for (let i = 0; i < g.length; i++) {
    if (g.startsWith("[*]", i)) {
      re += "\\[\\d+\\]";
      i += 2;
    } else if (g[i] === "*") re += "[^.\\[\\]]+";
    else re += g[i].replace(/[.+?^${}()|[\]\\]/g, "\\$&");
  }
  return new RegExp(anyDepth ? `^(?:.*\\.)?${re}$` : `^${re}$`);
}

export function normalize(v: Json, opts: NormalizeOptions, path = ""): Json | undefined {
  const ignore = opts.ignore.filter((g) => g.trim()).map(globToRegex);
  const walk = (x: Json, p: string): Json | undefined => {
    if (p && ignore.some((r) => r.test(p))) return undefined;
    if (Array.isArray(x)) {
      const items = x.map((c, i) => walk(c, childPath(p, i))).filter((c): c is Json => c !== undefined);
      return opts.sortArrays ? items.sort((a, b) => (canonical(a) < canonical(b) ? -1 : canonical(a) > canonical(b) ? 1 : 0)) : items;
    }
    if (x && typeof x === "object") {
      const keys = Object.keys(x);
      if (opts.sortKeys) keys.sort();
      const out: { [k: string]: Json } = {};
      for (const k of keys) {
        const c = walk(x[k], childPath(p, k));
        if (c !== undefined) out[k] = c;
      }
      return out;
    }
    if (typeof x === "string" && opts.trimStrings) return x.trim().replace(/\s+/g, " ");
    return x;
  };
  return walk(v, path);
}

export interface Change {
  path: string;
  kind: "added" | "removed" | "changed";
  old?: Json;
  new?: Json;
}

/** Structural differences between two values (arrays compared index by index). */
export function diff(a: Json, b: Json, path = "", out: Change[] = [], limit = 5000): Change[] {
  if (out.length >= limit) return out;
  const ka = kindOf(a);
  const kb = kindOf(b);
  if (ka !== kb || (ka !== "array" && ka !== "object")) {
    if (canonical(a) !== canonical(b)) out.push({ path: path || "(root)", kind: "changed", old: a, new: b });
    return out;
  }
  if (ka === "array") {
    const x = a as Json[];
    const y = b as Json[];
    for (let i = 0; i < Math.max(x.length, y.length); i++) {
      const p = childPath(path, i);
      if (i >= x.length) out.push({ path: p, kind: "added", new: y[i] });
      else if (i >= y.length) out.push({ path: p, kind: "removed", old: x[i] });
      else diff(x[i], y[i], p, out, limit);
    }
    return out;
  }
  const x = a as { [k: string]: Json };
  const y = b as { [k: string]: Json };
  for (const k of Object.keys(x)) {
    const p = childPath(path, k);
    if (!(k in y)) out.push({ path: p, kind: "removed", old: x[k] });
    else diff(x[k], y[k], p, out, limit);
  }
  for (const k of Object.keys(y)) if (!(k in x)) out.push({ path: childPath(path, k), kind: "added", new: y[k] });
  return out;
}

/** Parse JSON, or null when it isn't JSON. */
export function tryParse(text: string): Json | undefined {
  try {
    return JSON.parse(text) as Json;
  } catch {
    return undefined;
  }
}
