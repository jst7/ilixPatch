// Self-test for lib/extract — run from app/: node src/lib/extract/selftest.mjs
// Same cases as crates/runner/src/extract.rs, so the preview matches what the runner does.
import assert from "node:assert/strict";
import { jsonPath, previewRule } from "./eval.ts";

let n = 0;
const test = (name, fn) => {
  fn();
  n++;
  console.log("ok", name);
};
const d = JSON.parse('{"data":{"access_token":"abc","items":[{"id":7},{"id":8}],"a.b":true},"n":null}');

test("json paths", () => {
  assert.equal(jsonPath(d, "data.access_token"), "abc");
  assert.equal(jsonPath(d, "$.data.items[1].id"), "8");
  assert.equal(jsonPath(d, "data.items.0.id"), "7");
  assert.equal(jsonPath(d, 'data["a.b"]'), "true");
  assert.equal(jsonPath(d, "data.items[0]"), '{"id":7}');
  assert.equal(jsonPath(d, "n"), "null");
  assert.equal(jsonPath(d, "data.missing"), undefined);
  assert.throws(() => jsonPath(d, "data[0"));
});

test("rule previews", () => {
  const res = { status: 201, headers: [["x-request-id", "r-1"]], body: '{"data":{"access_token":"abc","html":"csrf=XYZ;"}}' };
  const r = (from, path) => ({ var: "v", from, path, scope: "environment", enabled: true });
  assert.deepEqual(previewRule(r("json", "data.access_token"), res), { ok: true, value: "abc" });
  assert.deepEqual(previewRule(r("header", "X-Request-Id"), res), { ok: true, value: "r-1" });
  assert.deepEqual(previewRule(r("status", ""), res), { ok: true, value: "201" });
  assert.deepEqual(previewRule(r("regex", "csrf=(\\w+)"), res), { ok: true, value: "XYZ" });
  assert.equal(previewRule(r("json", "data.nope"), res).ok, false);
  assert.equal(previewRule(r("json", "a"), { ...res, body: "<html>" }).error, "not JSON");
});

console.log(`${n} tests passed`);
