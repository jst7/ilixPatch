// Search matching shared by the project tree, the open tabs and the history list.
// - normalize(): lower-case + strip diacritics, so "Peticion" finds "Petición".
// - parseQuery(): whitespace-separated terms (AND), plus optional `method:post` and `in:<name>` filters.
// - prepare()/ranges(): highlight ranges in the ORIGINAL text, even when normalization changes lengths.
// Plain TypeScript with erasable syntax only, so `node lib/search/selftest.mjs` can import it directly.

/** Lower-case and strip combining marks (accents). */
export function normalize(s: string): string {
  return s.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();
}

/** Normalized text plus a map back to the original string, for highlighting. */
export interface Prepared {
  text: string;
  norm: string;
  /** map[i] = index in `text` of the character that produced norm[i]; map[norm.length] = text.length. */
  map: number[];
}

export function prepare(text: string): Prepared {
  let norm = "";
  const map: number[] = [];
  let i = 0;
  for (const ch of text) {
    const n = normalize(ch);
    for (let k = 0; k < n.length; k++) map.push(i);
    norm += n;
    i += ch.length;
  }
  map.push(text.length);
  return { text, norm, map };
}

export interface Query {
  /** Normalized free-text terms; every one must match (AND). */
  terms: string[];
  /** Normalized HTTP method from `method:xxx` (requests only). */
  method: string | null;
  /** Normalized names from `in:xxx`; an ancestor name must contain each. */
  within: string[];
}

export function parseQuery(q: string): Query {
  const out: Query = { terms: [], method: null, within: [] };
  for (const raw of q.trim().split(/\s+/)) {
    if (!raw) continue;
    const tok = normalize(raw);
    if (tok.startsWith("method:") && tok.length > 7) out.method = tok.slice(7);
    else if (tok.startsWith("in:") && tok.length > 3) out.within.push(tok.slice(3));
    else out.terms.push(tok);
  }
  return out;
}

export function isEmptyQuery(q: Query): boolean {
  return q.terms.length === 0 && q.method === null && q.within.length === 0;
}

/** True when every term occurs in at least one of the (normalized) haystacks. */
export function allTermsIn(terms: string[], haystacks: string[]): boolean {
  return terms.every((t) => haystacks.some((h) => h.includes(t)));
}

/** Merged, sorted [start, end) ranges in the original text where any term occurs. */
export function ranges(p: Prepared, terms: string[]): [number, number][] {
  const found: [number, number][] = [];
  for (const t of terms) {
    if (!t) continue;
    let from = 0;
    for (;;) {
      const i = p.norm.indexOf(t, from);
      if (i < 0) break;
      const start = p.map[i];
      // End: original index of the char after the last matched normalized char.
      let end = p.map[i + t.length];
      if (end === start) end = start + 1;
      found.push([start, Math.max(end, p.map[i + t.length - 1] + 1)]);
      from = i + 1;
    }
  }
  found.sort((a, b) => a[0] - b[0] || a[1] - b[1]);
  const merged: [number, number][] = [];
  for (const r of found) {
    const last = merged[merged.length - 1];
    if (last && r[0] <= last[1]) last[1] = Math.max(last[1], r[1]);
    else merged.push([r[0], r[1]]);
  }
  return merged;
}

export interface Part {
  text: string;
  hit: boolean;
}

/** Split `text` into plain and highlighted parts for the given terms. */
export function highlight(text: string, terms: string[]): Part[] {
  if (!text || terms.length === 0) return [{ text, hit: false }];
  const rs = ranges(prepare(text), terms);
  if (rs.length === 0) return [{ text, hit: false }];
  const parts: Part[] = [];
  let at = 0;
  for (const [s, e] of rs) {
    if (s > at) parts.push({ text: text.slice(at, s), hit: false });
    parts.push({ text: text.slice(s, e), hit: true });
    at = e;
  }
  if (at < text.length) parts.push({ text: text.slice(at), hit: false });
  return parts;
}
