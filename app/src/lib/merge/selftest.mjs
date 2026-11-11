// Self-check of the merge algorithms (not part of the build). Run from app/:
//   node src/lib/merge/selftest.mjs
// Needs Node >= 22.18 / 23.6 (loads the .ts file with built-in type stripping).
import assert from "node:assert/strict";
import {
  splitLines,
  diffSequences,
  diff3Chunks,
  buildResult,
  mergeClean,
  twoWayChunks,
  wordDiff,
  mapLine,
} from "./diff3.ts";

let passed = 0;
function test(name, fn) {
  try {
    fn();
    passed++;
    console.log(`ok   ${name}`);
  } catch (e) {
    console.error(`FAIL ${name}\n${e.stack}`);
    process.exitCode = 1;
  }
}

const kinds = (b, o, t) => diff3Chunks(splitLines(b), splitLines(o), splitLines(t)).map((c) => c.kind);

/** Chunks must cover each text completely, contiguously and in order. */
function checkCoverage(b, o, t) {
  const [B, O, T] = [b, o, t].map(splitLines);
  const chunks = diff3Chunks(B, O, T);
  for (const [key, lines] of [["base", B], ["ours", O], ["theirs", T]]) {
    let pos = 0;
    for (const c of chunks) {
      assert.equal(c[key][0], pos, `${key} contiguous`);
      assert.ok(c[key][1] >= c[key][0]);
      pos = c[key][1];
    }
    assert.equal(pos, lines.length, `${key} covered`);
  }
  for (const c of chunks) {
    if (c.kind === "equal") {
      assert.deepEqual(O.slice(...c.ours), B.slice(...c.base));
      assert.deepEqual(T.slice(...c.theirs), B.slice(...c.base));
    }
  }
  return chunks;
}

test("splitLines keeps terminators", () => {
  assert.deepEqual(splitLines("a\nb\n"), ["a\n", "b\n"]);
  assert.deepEqual(splitLines("a\r\nb"), ["a\r\n", "b"]);
  assert.deepEqual(splitLines(""), []);
  assert.deepEqual(splitLines(null), []);
});

test("diffSequences reconstructs b (random)", () => {
  let seed = 42;
  const rnd = (n) => ((seed = (seed * 1103515245 + 12345) % 2147483648), seed % n);
  for (let iter = 0; iter < 300; iter++) {
    const a = Array.from({ length: rnd(30) }, () => "x" + rnd(6));
    const b = Array.from({ length: rnd(30) }, () => "x" + rnd(6));
    const hunks = diffSequences(a, b);
    const out = [];
    let pa = 0;
    for (const h of hunks) {
      out.push(...a.slice(pa, h.aStart), ...b.slice(h.bStart, h.bEnd));
      pa = h.aEnd;
    }
    out.push(...a.slice(pa));
    assert.deepEqual(out, b);
    // Coverage of three-way chunks on random texts too.
    const c = Array.from({ length: rnd(30) }, () => "x" + rnd(6));
    checkCoverage(a.join("\n"), b.join("\n"), c.join("\n"));
  }
});

const BASE = "a\nb\nc\nd\ne\n";

test("clean merge", () => {
  const ours = "a\nB\nc\nd\ne\n";
  const theirs = "a\nb\nc\nD\ne\n";
  assert.deepEqual(kinds(BASE, ours, theirs), ["equal", "ours", "equal", "theirs", "equal"]);
  checkCoverage(BASE, ours, theirs);
  assert.equal(mergeClean(BASE, ours, theirs), "a\nB\nc\nD\ne\n");
});

test("conflict keeps base in result, unresolved", () => {
  const ours = "a\nb\nOURS\nd\ne\n";
  const theirs = "a\nb\nTHEIRS\nd\ne\n";
  const chunks = checkCoverage(BASE, ours, theirs);
  assert.deepEqual(chunks.map((c) => c.kind), ["equal", "conflict", "equal"]);
  assert.deepEqual(chunks[1], { kind: "conflict", base: [2, 3], ours: [2, 3], theirs: [2, 3] });
  assert.equal(mergeClean(BASE, ours, theirs), null);
  const r = buildResult(...[BASE, ours, theirs].map(splitLines), chunks);
  assert.equal(r.text, BASE);
  assert.deepEqual(r.ranges[1], [2, 3]);
  assert.deepEqual(r.applied, [true, false, true]);
});

test("adjacent changes conflict (like git)", () => {
  assert.deepEqual(kinds(BASE, "a\nb\nC\nd\ne\n", "a\nb\nc\nD\ne\n"), ["equal", "conflict", "equal"]);
});

test("both sides make the same change", () => {
  const same = "a\nb\nC\nd\ne\n";
  assert.deepEqual(kinds(BASE, same, same), ["equal", "both", "equal"]);
  assert.equal(mergeClean(BASE, same, same), same);
});

test("deletion vs modification", () => {
  const deleted = "a\nb\nd\ne\n";
  const modified = "a\nb\nC\nd\ne\n";
  const chunks = checkCoverage(BASE, deleted, modified);
  assert.deepEqual(chunks.map((c) => c.kind), ["equal", "conflict", "equal"]);
  assert.deepEqual(chunks[1].ours, [2, 2]);
  assert.deepEqual(chunks[1].theirs, [2, 3]);
  // Far apart: clean.
  assert.equal(mergeClean(BASE, "b\nc\nd\ne\n", "a\nb\nc\nd\nE\n"), "b\nc\nd\nE\n");
});

test("empty base (added on both sides)", () => {
  assert.deepEqual(kinds(null, "x\n", "y\n"), ["conflict"]);
  assert.deepEqual(kinds(null, "x\n", "x\n"), ["both"]);
  assert.equal(mergeClean(null, "x\n", null), "x\n");
  assert.equal(mergeClean(null, null, null), "");
  checkCoverage("", "x\ny\n", "z\n");
});

test("trailing newline differences", () => {
  // Only ours adds the final newline → clean, keeps it.
  assert.equal(mergeClean("a\nb", "a\nb\n", "a\nb"), "a\nb\n");
  // Theirs adds it, ours changes a non-adjacent line → clean.
  assert.equal(mergeClean("a\nx\nb", "A\nx\nb", "a\nx\nb\n"), "A\nx\nb\n");
  // Ours edits the last line, theirs adds the newline → same line changed differently.
  assert.deepEqual(kinds("a\nb", "a\nB", "a\nb\n"), ["equal", "conflict"]);
  // Both remove it the same way.
  assert.deepEqual(kinds("a\nb\n", "a\nb", "a\nb"), ["equal", "both"]);
});

test("insertions at the same point conflict, different points merge", () => {
  assert.deepEqual(kinds(BASE, "a\nX\nb\nc\nd\ne\n", "a\nY\nb\nc\nd\ne\n"), ["equal", "conflict", "equal"]);
  assert.equal(mergeClean(BASE, "X\n" + BASE, BASE + "Y\n"), "X\n" + BASE + "Y\n");
});

test("buildResult without auto-apply returns base and ranges", () => {
  const [B, O, T] = [BASE, "a\nB\nc\nd\ne\n", "a\nb\nc\nd\ne\nf\n"].map(splitLines);
  const chunks = diff3Chunks(B, O, T);
  const r = buildResult(B, O, T, chunks, false);
  assert.equal(r.text, BASE);
  assert.deepEqual(r.applied, chunks.map((c) => c.kind === "equal"));
  assert.deepEqual(r.ranges[r.ranges.length - 1], [5, 5]);
});

test("two-way chunks", () => {
  const c = twoWayChunks(splitLines("a\nb\nc\n"), splitLines("a\nB\nc\nd\n"));
  assert.deepEqual(c.map((x) => x.kind), ["equal", "changed", "equal", "added"]);
  assert.deepEqual(twoWayChunks(splitLines("a\n"), []).map((x) => x.kind), ["removed"]);
});

test("word diff", () => {
  const w = wordDiff('  "name": "foo",\n', '  "name": "bar",\n');
  assert.deepEqual(w.a, [[11, 14]]);
  assert.deepEqual(w.b, [[11, 14]]);
});

test("mapLine through segments", () => {
  const segs = [
    { from: [0, 2], to: [0, 2] },
    { from: [2, 3], to: [2, 6] },
    { from: [3, 3], to: [6, 8] },
    { from: [3, 5], to: [8, 10] },
  ];
  assert.equal(mapLine(segs, 1), 1);
  assert.equal(mapLine(segs, 2.5), 4);
  assert.equal(mapLine(segs, 3), 6);
  assert.equal(mapLine(segs, 4), 9);
  assert.equal(mapLine(segs, 6), 11);
});

test("performance: 5000-line files", () => {
  const base = Array.from({ length: 5000 }, (_, i) => `line ${i} ${"x".repeat(i % 40)}\n`);
  const ours = base.map((l, i) => (i % 97 === 0 ? `ours ${i}\n` : l));
  const theirs = base.map((l, i) => (i % 89 === 0 ? `theirs ${i}\n` : l));
  const t0 = performance.now();
  const chunks = diff3Chunks(base, ours, theirs);
  const ms = performance.now() - t0;
  assert.ok(chunks.length > 100);
  assert.ok(ms < 500, `took ${ms.toFixed(0)} ms`);
  // Completely different files hit the edit limit and degrade to one block, still fast.
  const other = base.map((_, i) => `other ${i}\n`);
  const t1 = performance.now();
  const c2 = twoWayChunks(base, other);
  const ms2 = performance.now() - t1;
  assert.equal(c2.length, 1);
  assert.ok(ms2 < 1500, `took ${ms2.toFixed(0)} ms`);
  console.log(`     diff3 ${ms.toFixed(1)} ms, fully different ${ms2.toFixed(1)} ms`);
});

console.log(`${passed} passed${process.exitCode ? ", some FAILED" : ""}`);
