// Line-based diff (Myers' O(ND) algorithm) for the Git view, plus helpers to collapse unchanged runs.
//
// Reasoning checks (no test runner in the frontend):
// - diffLines([], [])              → []
// - diffLines(["a"], [])           → [del a(1,-)]
// - diffLines([], ["a"])           → [add a(-,1)]
// - diffLines(["a","b","c"], ["a","x","c"]) → same a, del b, add x, same c (deletions before additions)
// - Identical inputs never enter the Myers loop: prefix trimming consumes everything.
// - When the edit distance exceeds MAX_EDITS the middle part is reported as "all deleted, then all added",
//   which is still a correct (if not minimal) diff and keeps memory bounded (trace is O(MAX_EDITS²)).

export type DiffKind = "same" | "add" | "del";

export interface DiffLine {
  kind: DiffKind;
  text: string;
  /** 1-based line number in the old file (null for added lines). */
  oldNo: number | null;
  /** 1-based line number in the new file (null for deleted lines). */
  newNo: number | null;
}

/** Above this many edits in the changed middle part, fall back to a non-minimal diff. */
const MAX_EDITS = 2000;

/** Split file contents into lines (null = file absent). A trailing newline doesn't create an empty last line. */
export function splitLines(text: string | null): string[] {
  if (text === null || text === "") return [];
  const lines = text.split(/\r?\n/);
  if (lines[lines.length - 1] === "") lines.pop();
  return lines;
}

type Op = [DiffKind, number, number]; // kind, index in a, index in b

/** Myers diff of two int sequences. Returns ops in order, or null when it needs more than maxD edits. */
function myers(a: Int32Array, b: Int32Array, maxD: number): Op[] | null {
  const n = a.length;
  const m = b.length;
  const max = n + m;
  const off = max + 1;
  const v = new Int32Array(2 * max + 3);
  const trace: Int32Array[] = [];
  let found = -1;
  outer: for (let d = 0; d <= Math.min(max, maxD); d++) {
    // Snapshot of v before step d, for k in [-d, d] (index k + d).
    trace.push(v.slice(off - d, off + d + 1));
    for (let k = -d; k <= d; k += 2) {
      let x = k === -d || (k !== d && v[off + k - 1] < v[off + k + 1]) ? v[off + k + 1] : v[off + k - 1] + 1;
      let y = x - k;
      while (x < n && y < m && a[x] === b[y]) {
        x++;
        y++;
      }
      v[off + k] = x;
      if (x >= n && y >= m) {
        found = d;
        break outer;
      }
    }
  }
  if (found < 0) return null;

  const ops: Op[] = [];
  let x = n;
  let y = m;
  for (let d = found; d > 0; d--) {
    const vp = trace[d];
    const at = (k: number) => vp[k + d];
    const k = x - y;
    const prevK = k === -d || (k !== d && at(k - 1) < at(k + 1)) ? k + 1 : k - 1;
    const prevX = at(prevK);
    const prevY = prevX - prevK;
    while (x > prevX && y > prevY) ops.push(["same", --x, --y]);
    if (x === prevX) ops.push(["add", x, --y]);
    else ops.push(["del", --x, y]);
  }
  while (x > 0 && y > 0) ops.push(["same", --x, --y]);
  return ops.reverse();
}

/** Line diff of two files. Deleted lines come before added lines within each change. */
export function diffLines(oldLines: string[], newLines: string[]): DiffLine[] {
  // Intern lines as ints so comparisons are cheap.
  const ids = new Map<string, number>();
  const intern = (lines: string[]) => {
    const out = new Int32Array(lines.length);
    lines.forEach((l, i) => {
      let id = ids.get(l);
      if (id === undefined) ids.set(l, (id = ids.size));
      out[i] = id;
    });
    return out;
  };
  const a = intern(oldLines);
  const b = intern(newLines);

  // Trim the common prefix and suffix (most edits are small and local).
  let start = 0;
  while (start < a.length && start < b.length && a[start] === b[start]) start++;
  let endA = a.length;
  let endB = b.length;
  while (endA > start && endB > start && a[endA - 1] === b[endB - 1]) {
    endA--;
    endB--;
  }

  const out: DiffLine[] = [];
  const same = (i: number, j: number) => out.push({ kind: "same", text: oldLines[i], oldNo: i + 1, newNo: j + 1 });
  const del = (i: number) => out.push({ kind: "del", text: oldLines[i], oldNo: i + 1, newNo: null });
  const add = (j: number) => out.push({ kind: "add", text: newLines[j], oldNo: null, newNo: j + 1 });

  for (let i = 0; i < start; i++) same(i, i);

  const ops = myers(a.subarray(start, endA), b.subarray(start, endB), MAX_EDITS);
  if (ops) {
    // Reorder each change block so its deletions come first (ops alternate del/add arbitrarily).
    let i = 0;
    while (i < ops.length) {
      if (ops[i][0] === "same") {
        same(ops[i][1] + start, ops[i][2] + start);
        i++;
        continue;
      }
      let j = i;
      while (j < ops.length && ops[j][0] !== "same") j++;
      for (let q = i; q < j; q++) if (ops[q][0] === "del") del(ops[q][1] + start);
      for (let q = i; q < j; q++) if (ops[q][0] === "add") add(ops[q][2] + start);
      i = j;
    }
  } else {
    for (let i = start; i < endA; i++) del(i);
    for (let j = start; j < endB; j++) add(j);
  }

  for (let i = endA, j = endB; i < a.length; i++, j++) same(i, j);
  return out;
}

/** Lines added / removed in a diff. */
export function diffStats(lines: DiffLine[]): { added: number; removed: number } {
  let added = 0;
  let removed = 0;
  for (const l of lines) {
    if (l.kind === "add") added++;
    else if (l.kind === "del") removed++;
  }
  return { added, removed };
}

export type DiffSegment =
  | { kind: "lines"; lines: DiffLine[] }
  /** A run of unchanged lines hidden behind an expander; `id` is stable for a given diff. */
  | { kind: "gap"; id: number; lines: DiffLine[] };

/**
 * Collapse unchanged runs, keeping `context` lines around each change. Runs that would hide fewer than
 * `minHidden` lines stay visible (an expander for 1–2 lines is just noise).
 */
export function segmentDiff(lines: DiffLine[], context = 3, minHidden = 4): DiffSegment[] {
  const segs: DiffSegment[] = [];
  let visible: DiffLine[] = [];
  let gapId = 0;
  const flush = () => {
    if (visible.length) segs.push({ kind: "lines", lines: visible });
    visible = [];
  };
  let i = 0;
  while (i < lines.length) {
    if (lines[i].kind !== "same") {
      visible.push(lines[i++]);
      continue;
    }
    let j = i;
    while (j < lines.length && lines[j].kind === "same") j++;
    const run = lines.slice(i, j);
    const keepBefore = i === 0 ? 0 : context; // context after the previous change
    const keepAfter = j === lines.length ? 0 : context; // context before the next change
    if (run.length - keepBefore - keepAfter >= minHidden) {
      visible.push(...run.slice(0, keepBefore));
      flush();
      segs.push({ kind: "gap", id: gapId++, lines: run.slice(keepBefore, run.length - keepAfter) });
      visible.push(...run.slice(run.length - keepAfter));
    } else {
      visible.push(...run);
    }
    i = j;
  }
  flush();
  return segs;
}
