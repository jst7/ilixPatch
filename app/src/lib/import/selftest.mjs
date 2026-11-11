// Self-check of the import planning rules (not part of the build). Run from app/:
//   node src/lib/import/selftest.mjs
// Needs Node >= 22.18 / 23.6 (loads the .ts file with built-in type stripping).
import assert from "node:assert/strict";
import {
  indexSource,
  checkState,
  setChecked,
  isExcluded,
  dropAs,
  place,
  unplace,
  rename,
  defaultPlacements,
  coveringPlacement,
  summarize,
  buildPlan,
  validPattern,
} from "./plan.ts";

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

const req = (id) => ({ id, kind: "request", name: id, method: "GET", has_scripts: false, auth: null, children: [] });
const folder = (id, children) => ({ id, kind: "folder", name: id, has_scripts: false, auth: null, children });
const coll = (id, children) => ({ id, kind: "collection", name: id, has_scripts: false, auth: null, children });

// c1: f1(r1, r2), f2(f3(r3)), r4      c2: r5
const tree = [coll("c1", [folder("f1", [req("r1"), req("r2")]), folder("f2", [folder("f3", [req("r3")])]), req("r4")]), coll("c2", [req("r5")])];
const index = indexSource(tree);
const P = "/proj";

test("index has every node with parents", () => {
  assert.equal(index.size, 10);
  assert.equal(index.get("r3").parent, "f3");
  assert.equal(index.get("c1").parent, null);
});

test("check states", () => {
  const none = new Set();
  assert.equal(checkState(index, none, "c1"), "all");
  const ex = setChecked(index, none, "r1", false);
  assert.equal(checkState(index, ex, "r1"), "none");
  assert.equal(checkState(index, ex, "f1"), "some");
  assert.equal(checkState(index, ex, "c1"), "some");
  const ex2 = setChecked(index, ex, "r2", false);
  assert.equal(checkState(index, ex2, "f1"), "none");
  assert.equal(checkState(index, ex2, "c1"), "some");
});

test("unchecking a container clears redundant descendant exclusions", () => {
  let ex = setChecked(index, new Set(), "r1", false);
  ex = setChecked(index, ex, "f1", false);
  assert.deepEqual([...ex], ["f1"]);
  assert.ok(isExcluded(index, ex, "r2"));
});

test("checking a node under an unchecked ancestor re-checks the path only", () => {
  let ex = setChecked(index, new Set(), "c1", false);
  ex = setChecked(index, ex, "r3", true);
  assert.equal(checkState(index, ex, "r3"), "all");
  assert.equal(checkState(index, ex, "f3"), "all");
  assert.equal(checkState(index, ex, "f2"), "all");
  assert.equal(checkState(index, ex, "f1"), "none");
  assert.equal(checkState(index, ex, "r4"), "none");
  assert.equal(checkState(index, ex, "c1"), "some");
  assert.ok(!ex.has("c1"));
});

test("re-checking a container checks everything below", () => {
  let ex = setChecked(index, new Set(), "r1", false);
  ex = setChecked(index, ex, "r3", false);
  ex = setChecked(index, ex, "c1", true);
  assert.equal(ex.size, 0);
});

test("drop rules", () => {
  assert.equal(dropAs("collection", "project"), "collection");
  assert.equal(dropAs("folder", "project"), "collection");
  assert.equal(dropAs("request", "project"), null);
  assert.equal(dropAs("collection", "folder"), "folder");
  assert.equal(dropAs("folder", "collection"), "folder");
  assert.equal(dropAs("request", "collection"), "request");
});

test("defaults: checked collections at the project root", () => {
  const ex = setChecked(index, new Set(), "c2", false);
  const d = defaultPlacements(tree, ex, P);
  assert.deepEqual(d, [{ source_id: "c1", target_parent: P, as: "collection", name: null }]);
});

test("place moves, keeps rename, refuses invalid drops", () => {
  let p = defaultPlacements(tree, new Set(), P);
  p = rename(p, "c1", "  Users API ");
  p = place(index, p, "c1", "/proj/existing", "collection");
  assert.equal(p.filter((x) => x.source_id === "c1").length, 1);
  assert.deepEqual(p.find((x) => x.source_id === "c1"), { source_id: "c1", target_parent: "/proj/existing", as: "folder", name: "Users API" });
  assert.equal(place(index, p, "r1", P, "project"), null);
  p = rename(p, "c1", "   ");
  assert.equal(p.find((x) => x.source_id === "c1").name, null);
});

test("placing a covered node moves it out of its ancestor", () => {
  let p = defaultPlacements(tree, new Set(), P);
  assert.equal(coveringPlacement(index, p, "r3").source_id, "c1");
  p = place(index, p, "f3", "/proj/other", "collection");
  const s = summarize(index, tree, new Set(), p);
  // c1 and c2 collections; folders: f1, f2 (in c1) + f3 (own placement); requests r1 r2 r3 r4 r5 counted once.
  assert.deepEqual(s, { collections: 2, folders: 3, requests: 5, unplaced: 0 });
  p = unplace(p, "f3");
  assert.deepEqual(summarize(index, tree, new Set(), p), { collections: 2, folders: 3, requests: 5, unplaced: 0 });
});

test("summary counts exclusions and unplaced nodes", () => {
  let ex = setChecked(index, new Set(), "f1", false);
  let p = defaultPlacements(tree, ex, P);
  assert.deepEqual(summarize(index, tree, ex, p), { collections: 2, folders: 2, requests: 3, unplaced: 0 });
  p = unplace(p, "c2");
  assert.deepEqual(summarize(index, tree, ex, p), { collections: 1, folders: 2, requests: 2, unplaced: 1 });
  // A request placed on its own counts as a request.
  p = place(index, p, "r5", "/proj/c", "collection");
  assert.deepEqual(summarize(index, tree, ex, p), { collections: 1, folders: 2, requests: 3, unplaced: 0 });
});

test("plan drops placements of unchecked sources", () => {
  const ex = setChecked(index, new Set(), "c2", false);
  const p = defaultPlacements(tree, new Set(), P);
  const plan = buildPlan({
    session: "s",
    projectPath: P,
    index,
    excluded: ex,
    placements: p,
    environments: [],
    globals: "skip",
    options: { on_conflict: "rename", convert_auth: true, keep_scripts: true, inline_parent_scripts: false, secret_pattern: null },
  });
  assert.deepEqual(plan.placements.map((x) => x.source_id), ["c1"]);
  assert.deepEqual(plan.excluded, ["c2"]);
});

test("secret pattern validation", () => {
  assert.ok(validPattern("token|secret|password|api[_-]?key|auth"));
  assert.ok(!validPattern("(unclosed"));
});

console.log(`\n${passed} passed${process.exitCode ? ", some FAILED" : ""}`);
