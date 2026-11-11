// Field-level ("semantic") diff of two versions of a request .toml file (D16: "URL changed", "Header added"…).
import { parse } from "smol-toml";
import type { Request } from "$lib/api";
import { diffLines, diffStats, splitLines } from "./diff";

export type SemField =
  | "request"
  | "name"
  | "method"
  | "url"
  | "header"
  | "query"
  | "body"
  | "bodyType"
  | "form"
  | "preRequest"
  | "tests";

export interface SemChange {
  kind: "added" | "removed" | "changed";
  field: SemField;
  /** Header / query / form field name. */
  key?: string;
  /** Old and new values for short scalar fields. */
  old?: string;
  new?: string;
  /** Line counts for multi-line fields (body content, scripts). */
  added?: number;
  removed?: number;
}

/** Parse a request file; null when the text isn't TOML or doesn't look like a request (name + url). */
export function parseRequest(text: string): Request | null {
  try {
    const data = parse(text) as Record<string, unknown>;
    if (typeof data.name !== "string" || typeof data.url !== "string") return null;
    return data as unknown as Request;
  } catch {
    return null;
  }
}

function str(v: unknown): string {
  return typeof v === "string" ? v : v === undefined || v === null ? "" : String(v);
}

function map(rec: unknown): Record<string, string> {
  if (!rec || typeof rec !== "object") return {};
  return Object.fromEntries(Object.entries(rec as Record<string, unknown>).map(([k, v]) => [k, str(v)]));
}

function diffMap(field: SemField, a: unknown, b: unknown, out: SemChange[]) {
  const ma = map(a);
  const mb = map(b);
  for (const [k, v] of Object.entries(mb)) {
    if (!(k in ma)) out.push({ kind: "added", field, key: k, new: v });
    else if (ma[k] !== v) out.push({ kind: "changed", field, key: k, old: ma[k], new: v });
  }
  for (const [k, v] of Object.entries(ma)) if (!(k in mb)) out.push({ kind: "removed", field, key: k, old: v });
}

function diffText(field: SemField, a: string, b: string, out: SemChange[]) {
  const hasA = a.trim() !== "";
  const hasB = b.trim() !== "";
  if (!hasA && !hasB) return;
  if (a === b) return;
  const { added, removed } = diffStats(diffLines(splitLines(a), splitLines(b)));
  out.push({ kind: !hasA ? "added" : !hasB ? "removed" : "changed", field, added, removed });
}

function bodyType(r: Request): string {
  const b = r.body as { type?: unknown } | null | undefined;
  return b && typeof b === "object" ? str(b.type) : "";
}

function bodyContent(r: Request): string {
  const b = r.body as { content?: unknown } | null | undefined;
  return b && typeof b === "object" ? str(b.content) : "";
}

function bodyFields(r: Request): unknown {
  const b = r.body as { fields?: unknown } | null | undefined;
  return b && typeof b === "object" ? b.fields : undefined;
}

/**
 * Compare two versions of a request file (null = file absent). Returns null when either present side
 * isn't a request (then only the text diff makes sense).
 */
export function semanticDiff(oldText: string | null, newText: string | null): SemChange[] | null {
  const a = oldText === null ? null : parseRequest(oldText);
  const b = newText === null ? null : parseRequest(newText);
  if ((oldText !== null && !a) || (newText !== null && !b)) return null;
  if (!a && !b) return null;
  if (!a) return [{ kind: "added", field: "request", new: b!.name }];
  if (!b) return [{ kind: "removed", field: "request", old: a.name }];

  const out: SemChange[] = [];
  const scalar = (field: SemField, x: string, y: string) => {
    if (x !== y) out.push({ kind: "changed", field, old: x, new: y });
  };
  scalar("name", str(a.name), str(b.name));
  scalar("method", str(a.method) || "GET", str(b.method) || "GET");
  scalar("url", str(a.url), str(b.url));
  diffMap("query", a.query, b.query, out);
  diffMap("header", a.headers, b.headers, out);

  const ta = bodyType(a);
  const tb = bodyType(b);
  if (ta !== tb) {
    if (!ta) out.push({ kind: "added", field: "bodyType", new: tb });
    else if (!tb) out.push({ kind: "removed", field: "bodyType", old: ta });
    else out.push({ kind: "changed", field: "bodyType", old: ta, new: tb });
  }
  if (ta === "form" || tb === "form") {
    diffMap("form", ta === "form" ? bodyFields(a) : undefined, tb === "form" ? bodyFields(b) : undefined, out);
  }
  if (ta !== "form" && tb !== "form") {
    const ca = bodyContent(a);
    const cb = bodyContent(b);
    if (ca !== cb) {
      const { added, removed } = diffStats(diffLines(splitLines(ca), splitLines(cb)));
      out.push({ kind: "changed", field: "body", added, removed });
    }
  }

  diffText("preRequest", str(a.scripts?.pre_request), str(b.scripts?.pre_request), out);
  diffText("tests", str(a.scripts?.tests), str(b.scripts?.tests), out);
  return out;
}
