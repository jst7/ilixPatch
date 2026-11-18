// Self-test for lib/json/model.ts — run from app/: node src/lib/json/selftest.mjs
import assert from "node:assert/strict";
import { ancestors, childPath, diff, normalize, search, stats, summary, tableColumns } from "./model.ts";

let n = 0;
const test = (name, fn) => {
  fn();
  n++;
  console.log("ok", name);
};

test("paths", () => {
  assert.equal(childPath("", "data"), "data");
  assert.equal(childPath("data", "items"), "data.items");
  assert.equal(childPath("data.items", 3), "data.items[3]");
  assert.equal(childPath("a", "x.y"), 'a["x.y"]');
  assert.equal(childPath("", "1st"), '["1st"]');
  assert.deepEqual(ancestors("a.b[0].c"), ["", "a", "a.b", "a.b[0]"]);
  assert.deepEqual(ancestors('a["x.y"].z'), ["", "a", 'a["x.y"]']);
});

test("summary, stats, table columns", () => {
  assert.equal(summary([1, 2, 3]), "[3]");
  assert.equal(summary({ a: 1, b: 2, c: 3, d: 4, e: 5 }), "{a, b, c, d, …}");
  assert.equal(summary("x".repeat(100), 10), '"xxxxxxxx…');
  assert.deepEqual(stats({ a: [1, { b: 2 }] }), { nodes: 5, depth: 3 });
  assert.deepEqual(tableColumns([{ id: 1, n: "a" }, { id: 2, extra: true }]), ["id", "n", "extra"]);
  assert.equal(tableColumns([1, 2]), null);
  assert.equal(tableColumns([]), null);
});

test("search finds keys and values, with paths", () => {
  const doc = { users: [{ name: "Ann", email: "ann@x.io" }, { name: "Bob" }], meta: { total: 2 } };
  assert.deepEqual(search(doc, "ann"), ["users[0].name", "users[0].email"]);
  assert.deepEqual(search(doc, "TOTAL"), ["meta.total"]);
  assert.deepEqual(search(doc, " "), []);
});

const off = { sortKeys: false, sortArrays: false, trimStrings: false, ignore: [] };

test("as-is diff sees order changes in arrays", () => {
  const a = { items: [1, 2], b: 1 };
  const b = { b: 1, items: [2, 1] };
  const changes = diff(normalize(a, off), normalize(b, off));
  assert.deepEqual(changes.map((c) => c.path), ["items[0]", "items[1]"]);
});

test("normalized diff ignores key and array order, whitespace and ignored paths", () => {
  const opts = { sortKeys: true, sortArrays: true, trimStrings: true, ignore: ["*.updatedAt", "meta.requestId", "items[*].tmp"] };
  const a = { items: [{ id: 2, tmp: 1 }, { id: 1 }], meta: { requestId: "a", n: " x  y " }, user: { updatedAt: "t1", v: 1 } };
  const b = { meta: { n: "x y", requestId: "b" }, user: { v: 1, updatedAt: "t2" }, items: [{ id: 1 }, { id: 2, tmp: 9 }] };
  assert.deepEqual(diff(normalize(a, opts), normalize(b, opts)), []);
  assert.equal(JSON.stringify(normalize({ b: 1, a: 2 }, opts)), '{"a":2,"b":1}');
});

test("data changes are reported with old and new values", () => {
  const changes = diff({ a: 1, gone: true, list: [1] }, { a: 2, added: "x", list: [1, 2] });
  assert.deepEqual(changes, [
    { path: "a", kind: "changed", old: 1, new: 2 },
    { path: "gone", kind: "removed", old: true },
    { path: "list[1]", kind: "added", new: 2 },
    { path: "added", kind: "added", new: "x" },
  ]);
  assert.deepEqual(diff(1, "1"), [{ path: "(root)", kind: "changed", old: 1, new: "1" }]);
});

console.log(`${n} json tests passed`);
