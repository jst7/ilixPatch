// Self-test for lib/largebody — run from app/: node src/lib/largebody/selftest.mjs
import assert from "node:assert/strict";
import { LineWindowCache } from "./cache.ts";
import { tokenizeJsonLine, segments } from "./tokens.ts";
import { LongLine, splitText, utf8Length } from "./longline.ts";
import { formatBytes, hexDump, clipLine, matchesByLine, firstLineAt, scrollTopFor, base64ToBytes } from "./format.ts";

let n = 0;
const test = (name, fn) => {
  fn();
  n++;
  console.log("ok", name);
};

test("window cache: get/set/LRU/missing", () => {
  const c = new LineWindowCache(10, 2);
  assert.equal(c.get(5), undefined);
  assert.deepEqual(c.missing(0, 25, 100), [0, 1, 2]);
  c.set(0, Array.from({ length: 10 }, (_, i) => `l${i}`));
  c.set(1, Array.from({ length: 10 }, (_, i) => `l${10 + i}`));
  assert.equal(c.get(13), "l13");
  c.get(2); // touch window 0 → window 1 becomes LRU
  c.set(2, ["l20"]);
  assert.ok(c.has(0) && c.has(2) && !c.has(1));
  assert.deepEqual(c.missing(0, 25, 21), [1]); // clamped to total-1 = 20
  assert.deepEqual(c.missing(0, 5, 0), []);
  c.clear();
  assert.equal(c.size, 0);
});

test("json tokenizer", () => {
  const toks = tokenizeJsonLine(`  "name": "Duck \\"Boots\\"", "n": -1.5e3, "ok": true, "x": null},`);
  const kinds = toks.filter((t) => t.kind !== "plain").map((t) => `${t.kind}:${t.text}`);
  assert.deepEqual(kinds, [
    'key:"name"', "punct::", 'string:"Duck \\"Boots\\""', "punct:,", 'key:"n"', "punct::", "number:-1.5e3", "punct:,",
    'key:"ok"', "punct::", "bool:true", "punct:,", 'key:"x"', "punct::", "null:null", "punct:}", "punct:,",
  ]);
  assert.equal(toks.map((t) => t.text).join(""), `  "name": "Duck \\"Boots\\"", "n": -1.5e3, "ok": true, "x": null},`);
  // unterminated string (line cut) still tokenizes
  assert.equal(tokenizeJsonLine(`"abc`)[0].kind, "string");
  assert.deepEqual(tokenizeJsonLine(""), []);
});

test("segments merge tokens with matches", () => {
  const line = `"key": "value"`;
  const segs = segments(line, true, [{ column: 3, length: 6 }]);
  assert.equal(segs.map((s) => s.text).join(""), line);
  const hit = segs.filter((s) => s.match === 0).map((s) => s.text).join("");
  assert.equal(hit, `y": "v`);
  assert.deepEqual(segments("hello", false, []), [{ text: "hello", kind: "plain", match: -1 }]);
  const two = segments("abcabc", false, [{ column: 0, length: 1 }, { column: 3, length: 1 }]);
  assert.deepEqual(two.map((s) => [s.text, s.match]), [["a", 0], ["bc", -1], ["a", 1], ["bc", -1]]);
});

test("formatting helpers", () => {
  assert.equal(formatBytes(512), "512 B");
  assert.equal(formatBytes(1536), "1.50 KB");
  assert.equal(formatBytes(250 * 1024 * 1024), "250 MB");
  const dump = hexDump(new TextEncoder().encode("Hello, world!\n\x00\x01ABC"));
  assert.equal(dump.length, 2);
  assert.ok(dump[0].startsWith("00000000  48 65 6c 6c 6f 2c 20 77  6f 72 6c 64 21 0a 00 01  |Hello, world!...|"));
  assert.ok(dump[1].startsWith("00000010  41 42 43"));
  assert.deepEqual(clipLine("abcdef", 4), { text: "abcd", hidden: 2 });
  assert.deepEqual(clipLine("ab", 4), { text: "ab", hidden: 0 });
  const byLine = matchesByLine([{ line: 1, c: 0 }, { line: 1, c: 5 }, { line: 7, c: 2 }]);
  assert.equal(byLine.get(1).length, 2);
  assert.equal(byLine.get(7).length, 1);
  assert.deepEqual([...base64ToBytes("AAH/")], [0, 1, 255]);
});

test("scroll mapping (normal and scaled)", () => {
  assert.equal(firstLineAt(180, 400, 18, 1000, 1e7), 10);
  assert.equal(scrollTopFor(10, 400, 18, 1000, 1e7), 180);
  // 10M lines × 18px exceeds 15M px → scaled
  const total = 10_000_000;
  const max = 15_000_000;
  assert.equal(firstLineAt(0, 400, 18, total, max), 0);
  const end = firstLineAt(max - 400, 400, 18, total, max);
  assert.equal(end, total - Math.ceil(400 / 18));
  const mid = scrollTopFor(5_000_000, 400, 18, total, max);
  assert.ok(Math.abs(firstLineAt(mid, 400, 18, total, max) - 5_000_000) < 2);
});

// Fake byte reader over a UTF-8 buffer (adjusts reads to character boundaries like the backend).
function reader(text) {
  const bytes = new TextEncoder().encode(text);
  const dec = new TextDecoder();
  let calls = 0;
  const read = async (offset, length) => {
    calls++;
    let end = Math.min(bytes.length, offset + length);
    while (end < bytes.length && (bytes[end] & 0xc0) === 0x80) end--;
    return { data: dec.decode(bytes.subarray(offset, end)), length: end - offset };
  };
  return { read, bytes, calls: () => calls };
}

test("splitText keeps bytes and surrogate pairs", () => {
  const s = "a€😀".repeat(10);
  const parts = splitText(s, utf8Length(s), 7);
  assert.equal(parts.map((p) => p.text).join(""), s);
  assert.equal(parts.reduce((a, p) => a + p.bytes, 0), utf8Length(s));
  for (const p of parts) assert.ok(!/^[\uDC00-\uDFFF]/.test(p.text));
});

await (async () => {
  // A long single line with a prefix, as responseLines would return it cut.
  const line = Array.from({ length: 50_000 }, (_, i) => `{"i":${i},"name":"duck ñ ${i}"}`).join(",");
  const prefix = "HEADER\n";
  const r = reader(prefix + line);
  const lineOffset = utf8Length(prefix);
  const cut = new TextDecoder().decode(r.bytes.subarray(lineOffset, lineOffset + 1000));
  const ll = new LongLine(lineOffset, utf8Length(line), cut);
  assert.ok(ll.hasNext() && !ll.hasPrev());
  // Step through the whole line chunk by chunk and rebuild it.
  let i = 0;
  while (await ll.ensure(i, r.read, 65536)) i++;
  assert.equal(ll.chunks.map((c) => c.text).join(""), line);
  assert.equal(ll.remainingBytes, 0);
  assert.ok(ll.chunks.every((c) => c.charLength <= 65536));
  // Evict and reload by byte range.
  ll.pos = 0;
  ll.evict(1);
  assert.equal(ll.chunks[5].text, null);
  await ll.ensure(5, r.read, 65536);
  assert.equal(ll.chunks[5].text, line.slice(ll.chunks[5].charStart, ll.chunks[5].charStart + ll.chunks[5].charLength));
  // Seek to a character column on a fresh line.
  const fresh = new LongLine(lineOffset, utf8Length(line), cut);
  const col = line.indexOf('"i":40000');
  const idx = await fresh.seekChar(col, r.read, 1 << 20);
  const c = fresh.chunks[idx];
  await fresh.ensure(idx, r.read, 65536);
  assert.equal(c.text.slice(col - c.charStart, col - c.charStart + 9), '"i":40000');
  assert.equal(await fresh.seekChar(line.length + 5, r.read, 1 << 20), -1);
  n++;
  console.log("ok long line: progressive chunks, eviction, seek");
})();

console.log(`\n${n} largebody tests passed`);
