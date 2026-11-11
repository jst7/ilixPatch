// Pure line-diff algorithms for the merge resolver and the two-way comparator (D24).
// No imports on purpose: `selftest.mjs` loads this file directly with Node's type stripping.
//
// Text model: a text is split into lines that KEEP their terminator ("a\n", "b\r\n"); only the last line
// may lack one. Joining the lines gives back the exact text, so a missing trailing newline is a real change.
// All ranges are half-open line index ranges [start, end) into such arrays.

/** A maximal non-matching region between two sequences. */
export interface Hunk {
  aStart: number;
  aEnd: number;
  bStart: number;
  bEnd: number;
}

export type Range = [start: number, end: number];

/** Split text into lines that keep their "\n" terminator. "" → []. */
export function splitLines(text: string | null | undefined): string[] {
  if (!text) return [];
  const out: string[] = [];
  let start = 0;
  for (;;) {
    const i = text.indexOf("\n", start);
    if (i < 0) break;
    out.push(text.slice(start, i + 1));
    start = i + 1;
  }
  if (start < text.length) out.push(text.slice(start));
  return out;
}

export function joinLines(lines: readonly string[], range?: Range): string {
  return (range ? lines.slice(range[0], range[1]) : lines).join("");
}

/** Beyond this many edits Myers gives up and reports the rest as one replaced block (bounds time and memory). */
const MAX_EDITS = 2500;

/**
 * Myers O((N+M)·D) diff of two sequences of interned ids. Returns the hunks (non-matching regions) in order.
 * Common prefix/suffix are trimmed first, so typical edits on large files are cheap.
 */
function myers(a: Int32Array, b: Int32Array): Hunk[] {
  let pre = 0;
  const n0 = a.length;
  const m0 = b.length;
  while (pre < n0 && pre < m0 && a[pre] === b[pre]) pre++;
  let suf = 0;
  while (suf < n0 - pre && suf < m0 - pre && a[n0 - 1 - suf] === b[m0 - 1 - suf]) suf++;
  const N = n0 - pre - suf;
  const M = m0 - pre - suf;
  if (N === 0 && M === 0) return [];
  if (N === 0 || M === 0) return [{ aStart: pre, aEnd: pre + N, bStart: pre, bEnd: pre + M }];

  const max = N + M;
  const off = max + 1;
  const v = new Int32Array(2 * max + 3);
  const trace: Int32Array[] = [];
  let D = -1;
  const limit = Math.min(max, MAX_EDITS);
  outer: for (let d = 0; d <= limit; d++) {
    for (let k = -d; k <= d; k += 2) {
      let x = k === -d || (k !== d && v[off + k - 1] < v[off + k + 1]) ? v[off + k + 1] : v[off + k - 1] + 1;
      let y = x - k;
      while (x < N && y < M && a[pre + x] === b[pre + y]) {
        x++;
        y++;
      }
      v[off + k] = x;
      if (x >= N && y >= M) {
        D = d;
        break outer;
      }
    }
    trace.push(v.slice(off - d, off + d + 1));
  }
  if (D < 0) return [{ aStart: pre, aEnd: pre + N, bStart: pre, bEnd: pre + M }];

  // Backtrack collecting matched pairs (in reverse).
  const ma: number[] = [];
  const mb: number[] = [];
  let x = N;
  let y = M;
  for (let d = D; d > 0; d--) {
    const prev = trace[d - 1];
    const get = (kk: number) => prev[kk + d - 1];
    const k = x - y;
    const down = k === -d || (k !== d && get(k - 1) < get(k + 1));
    const prevK = down ? k + 1 : k - 1;
    const prevX = get(prevK);
    const prevY = prevX - prevK;
    const startX = down ? prevX : prevX + 1;
    const startY = startX - k;
    while (x > startX && y > startY) {
      x--;
      y--;
      ma.push(x);
      mb.push(y);
    }
    x = prevX;
    y = prevY;
  }
  while (x > 0 && y > 0) {
    x--;
    y--;
    ma.push(x);
    mb.push(y);
  }

  const hunks: Hunk[] = [];
  let pa = 0;
  let pb = 0;
  for (let i = ma.length - 1; i >= 0; i--) {
    const ia = ma[i];
    const ib = mb[i];
    if (ia > pa || ib > pb) hunks.push({ aStart: pre + pa, aEnd: pre + ia, bStart: pre + pb, bEnd: pre + ib });
    pa = ia + 1;
    pb = ib + 1;
  }
  if (pa < N || pb < M) hunks.push({ aStart: pre + pa, aEnd: pre + N, bStart: pre + pb, bEnd: pre + M });
  return hunks;
}

/** Diff any two sequences of strings (lines or tokens). */
export function diffSequences(a: readonly string[], b: readonly string[]): Hunk[] {
  const ids = new Map<string, number>();
  const intern = (list: readonly string[]) => {
    const out = new Int32Array(list.length);
    for (let i = 0; i < list.length; i++) {
      let id = ids.get(list[i]);
      if (id === undefined) {
        id = ids.size;
        ids.set(list[i], id);
      }
      out[i] = id;
    }
    return out;
  };
  return myers(intern(a), intern(b));
}

// ---------- Two-way chunks ----------

export type DiffChunkKind = "equal" | "changed" | "added" | "removed";

/** Aligned chunk of a two-way diff ("added" = only on the right, "removed" = only on the left). */
export interface DiffChunk {
  kind: DiffChunkKind;
  left: Range;
  right: Range;
}

/** Hunks → alternating equal / change chunks covering both sequences completely. */
export function twoWayChunks(left: readonly string[], right: readonly string[]): DiffChunk[] {
  const chunks: DiffChunk[] = [];
  let l = 0;
  let r = 0;
  for (const h of diffSequences(left, right)) {
    if (h.aStart > l) chunks.push({ kind: "equal", left: [l, h.aStart], right: [r, h.bStart] });
    const kind: DiffChunkKind = h.aStart === h.aEnd ? "added" : h.bStart === h.bEnd ? "removed" : "changed";
    chunks.push({ kind, left: [h.aStart, h.aEnd], right: [h.bStart, h.bEnd] });
    l = h.aEnd;
    r = h.bEnd;
  }
  if (l < left.length || r < right.length) chunks.push({ kind: "equal", left: [l, left.length], right: [r, right.length] });
  return chunks;
}

// ---------- Three-way chunks (diff3) ----------

/**
 * - equal: no side changed these base lines
 * - ours / theirs: only that side changed them (auto-mergeable)
 * - both: both sides made the identical change (auto-mergeable)
 * - conflict: both sides changed overlapping (or adjacent) base lines differently
 */
export type MergeChunkKind = "equal" | "ours" | "theirs" | "both" | "conflict";

export interface MergeChunk {
  kind: MergeChunkKind;
  base: Range;
  ours: Range;
  theirs: Range;
}

/**
 * diff3: diff base→ours and base→theirs, then group hunks whose base ranges overlap or touch (touching
 * changes conflict, like git). Chunks cover all three texts completely and in order.
 */
export function diff3Chunks(base: readonly string[], ours: readonly string[], theirs: readonly string[]): MergeChunk[] {
  type Tagged = Hunk & { side: 0 | 1 };
  const all: Tagged[] = [
    ...diffSequences(base, ours).map((h) => ({ ...h, side: 0 as const })),
    ...diffSequences(base, theirs).map((h) => ({ ...h, side: 1 as const })),
  ].sort((x, y) => x.aStart - y.aStart || x.aEnd - y.aEnd);

  const chunks: MergeChunk[] = [];
  // Offset (side line − base line) in the unchanged region before the current position, per side.
  const delta = [0, 0];
  let basePos = 0;
  let i = 0;
  while (i < all.length) {
    const group: Tagged[] = [all[i]];
    let end = all[i].aEnd;
    i++;
    while (i < all.length && all[i].aStart <= end) {
      end = Math.max(end, all[i].aEnd);
      group.push(all[i]);
      i++;
    }
    const start = group[0].aStart;
    if (start > basePos) {
      chunks.push({
        kind: "equal",
        base: [basePos, start],
        ours: [basePos + delta[0], start + delta[0]],
        theirs: [basePos + delta[1], start + delta[1]],
      });
    }
    const ranges: Range[] = [0, 1].map((side) => {
      const hs = group.filter((h) => h.side === side);
      if (hs.length === 0) return [start + delta[side], end + delta[side]];
      const first = hs[0];
      const last = hs[hs.length - 1];
      return [first.bStart - (first.aStart - start), last.bEnd + (end - last.aEnd)];
    });
    const hasOurs = group.some((h) => h.side === 0);
    const hasTheirs = group.some((h) => h.side === 1);
    let kind: MergeChunkKind;
    if (hasOurs && hasTheirs) {
      kind = sameLines(ours, ranges[0], theirs, ranges[1]) ? "both" : "conflict";
    } else kind = hasOurs ? "ours" : "theirs";
    chunks.push({ kind, base: [start, end], ours: ranges[0], theirs: ranges[1] });
    delta[0] = ranges[0][1] - end;
    delta[1] = ranges[1][1] - end;
    basePos = end;
  }
  if (basePos < base.length || basePos + delta[0] < ours.length || basePos + delta[1] < theirs.length) {
    chunks.push({
      kind: "equal",
      base: [basePos, base.length],
      ours: [basePos + delta[0], ours.length],
      theirs: [basePos + delta[1], theirs.length],
    });
  }
  return chunks;
}

function sameLines(a: readonly string[], ra: Range, b: readonly string[], rb: Range): boolean {
  if (ra[1] - ra[0] !== rb[1] - rb[0]) return false;
  for (let i = 0; i < ra[1] - ra[0]; i++) if (a[ra[0] + i] !== b[rb[0] + i]) return false;
  return true;
}

/** Initial result of a merge, with the line range each non-equal chunk occupies in it. */
export interface MergeResult {
  text: string;
  /** Per chunk (same indexes as the chunks): its line range in the result. */
  ranges: Range[];
  /** Per chunk: true when the chunk was auto-applied (or needed nothing). */
  applied: boolean[];
}

/**
 * Build the result: base text with every non-conflicting change applied (when `autoApply`), conflicts left as
 * their base lines (unresolved, like IntelliJ). With autoApply=false the result is just the base.
 */
export function buildResult(
  base: readonly string[],
  ours: readonly string[],
  theirs: readonly string[],
  chunks: readonly MergeChunk[],
  autoApply = true,
): MergeResult {
  const parts: string[] = [];
  const ranges: Range[] = [];
  const applied: boolean[] = [];
  let line = 0;
  for (const c of chunks) {
    let src: readonly string[] = base;
    let r: Range = c.base;
    let done = c.kind === "equal";
    if (autoApply && (c.kind === "ours" || c.kind === "both")) [src, r, done] = [ours, c.ours, true];
    else if (autoApply && c.kind === "theirs") [src, r, done] = [theirs, c.theirs, true];
    for (let i = r[0]; i < r[1]; i++) parts.push(src[i]);
    ranges.push([line, line + r[1] - r[0]]);
    applied.push(done);
    line += r[1] - r[0];
  }
  return { text: fixJoins(parts), ranges, applied };
}

/** Join lines, inserting "\n" where a line without terminator would otherwise be glued to the next one. */
export function fixJoins(parts: readonly string[]): string {
  let out = "";
  for (let i = 0; i < parts.length; i++) {
    out += parts[i];
    if (i < parts.length - 1 && parts[i] !== "" && !parts[i].endsWith("\n")) out += "\n";
  }
  return out;
}

/** Text of a git-style clean merge, or null when there are conflicts (used by the self-test). */
export function mergeClean(base: string | null, ours: string | null, theirs: string | null): string | null {
  const b = splitLines(base);
  const o = splitLines(ours);
  const t = splitLines(theirs);
  const chunks = diff3Chunks(b, o, t);
  if (chunks.some((c) => c.kind === "conflict")) return null;
  return buildResult(b, o, t, chunks, true).text;
}

// ---------- Word-level highlights ----------

const TOKEN_RE = /\w+|\s+|[^\w\s]/g;

/** Character ranges [from, to) that differ between two texts, at word granularity, for each side. */
export function wordDiff(a: string, b: string): { a: Range[]; b: Range[] } {
  const ta = a.match(TOKEN_RE) ?? [];
  const tb = b.match(TOKEN_RE) ?? [];
  const posA = offsets(ta);
  const posB = offsets(tb);
  const ra: Range[] = [];
  const rb: Range[] = [];
  for (const h of diffSequences(ta, tb)) {
    if (h.aEnd > h.aStart) pushRange(ra, posA[h.aStart], posA[h.aEnd]);
    if (h.bEnd > h.bStart) pushRange(rb, posB[h.bStart], posB[h.bEnd]);
  }
  return { a: ra, b: rb };
}

function offsets(tokens: string[]): number[] {
  const out = new Array<number>(tokens.length + 1);
  let p = 0;
  for (let i = 0; i < tokens.length; i++) {
    out[i] = p;
    p += tokens[i].length;
  }
  out[tokens.length] = p;
  return out;
}

function pushRange(list: Range[], from: number, to: number) {
  const last = list[list.length - 1];
  if (last && last[1] >= from) last[1] = Math.max(last[1], to);
  else list.push([from, to]);
}

// ---------- Mapping lines between aligned texts (synchronised scrolling) ----------

/**
 * Map a fractional line position in one text to another through aligned segments (each segment pairs a
 * range in `from` with a range in `to`). Inside a segment the position is scaled proportionally.
 */
export function mapLine(segments: readonly { from: Range; to: Range }[], line: number): number {
  if (segments.length === 0) return line;
  for (const s of segments) {
    const [fs, fe] = s.from;
    if (line < fe || (fe === fs && line <= fs)) {
      if (line <= fs) return s.to[0];
      const frac = (line - fs) / (fe - fs);
      return s.to[0] + frac * (s.to[1] - s.to[0]);
    }
  }
  const last = segments[segments.length - 1];
  return last.to[1] + (line - last.from[1]);
}
