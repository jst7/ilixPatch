// Self-test for lib/netlog — run from app/: node src/lib/netlog/selftest.mjs
import assert from "node:assert/strict";
import { entryToCurl, maskValue, statusClass } from "./format.ts";

let n = 0;
const test = (name, fn) => {
  fn();
  n++;
  console.log("ok", name);
};
const entry = (over = {}) => ({
  id: 1, trace: 1, label: "x", phase: null, timestamp_ms: 0, method: "POST", url: "https://api.example.com/a?b=1",
  request_headers: [["host", "api.example.com"], ["content-length", "9"], ["accept-encoding", "gzip, br"],
    ["authorization", "Bearer secret"], ["x-note", "it's"]],
  request_body: { text: '{"a":"b"}', size: 9, truncated: false, binary: false },
  status: 200, status_text: "OK", response_headers: [], response_body: null, http_version: "HTTP/1.1",
  remote_addr: null, proxy: null, elapsed_ms: 1, error: null, ...over,
});

test("secrets are masked, keeping the scheme", () => {
  assert.equal(maskValue("Authorization", "Bearer abc"), "Bearer ••••••");
  assert.equal(maskValue("authorization", "abc"), "••••••");
  assert.equal(maskValue("Cookie", "a=1"), "••••••");
  assert.equal(maskValue("X-Api-Key", "k"), "••••••");
  assert.equal(maskValue("Content-Type", "text/plain"), "text/plain");
});

test("cURL skips client headers and quotes values", () => {
  const c = entryToCurl(entry());
  assert.equal(c.split("\n")[0], "curl -X POST 'https://api.example.com/a?b=1' \\");
  assert.ok(!c.includes("host:") && !c.includes("content-length"));
  assert.ok(c.includes("--compressed"));
  assert.ok(c.includes("-H 'authorization: Bearer secret'"));
  assert.ok(c.includes(`'x-note: it'\\''s'`));
  assert.ok(c.includes(`--data-raw '{"a":"b"}'`));
  assert.ok(entryToCurl(entry({ method: "GET", request_body: null })).startsWith("curl 'https"));
});

test("status classes", () => {
  assert.equal(statusClass(entry({ status: 204 })), "ok");
  assert.equal(statusClass(entry({ status: 302 })), "redirect");
  assert.equal(statusClass(entry({ status: 401 })), "warn");
  assert.equal(statusClass(entry({ status: 503 })), "err");
  assert.equal(statusClass(entry({ status: null, error: "refused" })), "err");
});

console.log(`${n} tests passed`);
