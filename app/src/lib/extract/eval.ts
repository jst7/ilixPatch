// Variables from a response (D48): preview of the `[[extract]]` rules in the UI.
// Mirrors crates/runner/src/extract.rs (which is what applies them on every send).
import type { Extract } from "$lib/api";

type Step = { key: string } | { index: number };

export function parsePath(path: string): Step[] {
  let p = path.trim();
  if (p.startsWith("$")) p = p.slice(1);
  const steps: Step[] = [];
  let key = "";
  const flush = () => {
    if (key) steps.push({ key });
    key = "";
  };
  for (let i = 0; i < p.length; i++) {
    const c = p[i];
    if (c === ".") flush();
    else if (c === "[") {
      flush();
      const q = p[i + 1];
      if (q === '"' || q === "'") {
        const end = p.indexOf(q, i + 2);
        if (end === -1 || p[end + 1] !== "]") throw new Error(`unclosed [ in path \`${path}\``);
        steps.push({ key: p.slice(i + 2, end) });
        i = end + 1;
      } else {
        const end = p.indexOf("]", i + 1);
        if (end === -1) throw new Error(`unclosed [ in path \`${path}\``);
        const inner = p.slice(i + 1, end).trim();
        steps.push(/^\d+$/.test(inner) ? { index: Number(inner) } : { key: inner });
        i = end;
      }
    } else key += c;
  }
  flush();
  return steps;
}

/** Value at `path` (strings unquoted, other values as JSON); undefined when missing. */
export function jsonPath(doc: unknown, path: string): string | undefined {
  let v: unknown = doc;
  for (const s of parsePath(path)) {
    if ("index" in s) v = Array.isArray(v) ? v[s.index] : undefined;
    else if (Array.isArray(v) && /^\d+$/.test(s.key)) v = v[Number(s.key)];
    else v = v !== null && typeof v === "object" && !Array.isArray(v) ? (v as Record<string, unknown>)[s.key] : undefined;
    if (v === undefined) return undefined;
  }
  return typeof v === "string" ? v : JSON.stringify(v);
}

export type Preview = { ok: true; value: string } | { ok: false; error: string };

/** What a rule gives for a response (body: its text, or null when not available in the UI). */
export function previewRule(
  r: Extract,
  res: { status: number; headers: [string, string][]; body: string | null },
): Preview {
  try {
    switch (r.from) {
      case "status":
        return { ok: true, value: String(res.status) };
      case "header": {
        const h = res.headers.find(([k]) => k.toLowerCase() === r.path.trim().toLowerCase());
        return h ? { ok: true, value: h[1] } : { ok: false, error: "not found" };
      }
      case "body":
        return res.body === null ? { ok: false, error: "no body" } : { ok: true, value: res.body };
      case "regex": {
        if (res.body === null) return { ok: false, error: "no body" };
        const m = new RegExp(r.path).exec(res.body);
        return m ? { ok: true, value: m[1] ?? m[0] } : { ok: false, error: "not found" };
      }
      default: {
        if (res.body === null) return { ok: false, error: "no body" };
        let doc: unknown;
        try {
          doc = JSON.parse(res.body);
        } catch {
          return { ok: false, error: "not JSON" };
        }
        const v = jsonPath(doc, r.path);
        return v === undefined ? { ok: false, error: "not found" } : { ok: true, value: v };
      }
    }
  } catch (e) {
    return { ok: false, error: (e as Error).message };
  }
}
