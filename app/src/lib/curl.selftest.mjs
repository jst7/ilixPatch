// Self-test for lib/curl.ts — run from app/: node src/lib/curl.selftest.mjs
// (Node ≥ 22.18 / 23.6 strips TypeScript types natively.)
import assert from "node:assert/strict";
import { looksLikeCurl, parseCurl, toCurl, tokenize, resolveRequest, unexportedAuth } from "./curl.ts";

let n = 0;
const test = (name, fn) => {
  fn();
  n++;
  console.log("ok", name);
};

test("tokenizer: quotes, escapes, continuations", () => {
  assert.deepEqual(tokenize(`curl 'a b' "c \\"d\\"" e\\ f \\\n  -H x`), ["curl", "a b", 'c "d"', "e f", "-H", "x"]);
  assert.deepEqual(tokenize(`curl $'line1\\nline2\\u00e9'`), ["curl", "line1\nline2é"]);
});

test("simple GET", () => {
  const { request, warnings } = parseCurl("curl https://api.example.com/users/42");
  assert.equal(request.method, "GET");
  assert.equal(request.url, "https://api.example.com/users/42");
  assert.equal(request.name, "42");
  assert.equal(warnings.length, 0);
  assert.ok(looksLikeCurl("  curl https://x"));
  assert.ok(!looksLikeCurl("https://curl.se"));
});

test("Chrome 'Copy as cURL (bash)' POST with JSON", () => {
  const cmd = `curl 'https://api.example.com/v1/items?x=1' \\
  -H 'accept: application/json' \\
  -H 'content-type: application/json' \\
  -H 'cookie: a=1' \\
  --data-raw '{"name":"Duck","boots":2}' \\
  --compressed`;
  const { request, warnings } = parseCurl(cmd);
  assert.equal(request.method, "POST");
  assert.equal(request.url, "https://api.example.com/v1/items?x=1");
  assert.equal(request.headers["accept"], "application/json");
  assert.equal(request.headers["cookie"], "a=1");
  assert.equal(request.body.type, "json");
  assert.deepEqual(JSON.parse(request.body.content), { name: "Duck", boots: 2 });
  assert.equal(warnings.length, 0);
});

test("Windows cmd style (^ escapes and ^ continuation)", () => {
  const cmd = 'curl ^"https://api.example.com/a?b=1^&c=2^" ^\n  -H ^"accept: */*^" ^\n  --data-raw ^"^{^\\^"k^\\^":1^}^"';
  const { request } = parseCurl(cmd);
  assert.equal(request.url, "https://api.example.com/a?b=1&c=2");
  assert.equal(request.headers["accept"], "*/*");
  assert.equal(request.method, "POST");
});

test("flags: -X -u -k -L -m --http1.1 -A -G", () => {
  const { request } = parseCurl(
    "curl -XPUT -u admin:s3cr3t -k -L -m 2.5 --http1.1 -A 'Duck/1.0' -G -d q=boots https://h/search",
  );
  assert.equal(request.method, "PUT");
  assert.deepEqual(request.auth, { type: "basic", username: "admin", password: "s3cr3t" });
  assert.equal(request.headers.Authorization, undefined);
  assert.deepEqual(request.settings, { verify_ssl: false, follow_redirects: true, timeout_ms: 2500, http_version: "http1" });
  assert.equal(request.headers["User-Agent"], "Duck/1.0");
  assert.equal(request.url, "https://h/search?q=boots");
  assert.equal(request.body, null);
});

test("form bodies and --json", () => {
  const a = parseCurl("curl https://h -d 'a=1&b=hello%20world'").request;
  assert.deepEqual(a.body, { type: "form", fields: { a: "1", b: "hello world" } });
  const b = parseCurl(`curl https://h --data-urlencode 'msg=a b&c'`).request;
  assert.deepEqual(b.body, { type: "form", fields: { msg: "a b&c" } });
  const c = parseCurl(`curl --json '{"x":1}' https://h`).request;
  assert.equal(c.body.type, "json");
  assert.equal(c.headers["Content-Type"], "application/json");
  const d = parseCurl("curl -F name=duck -F 'pic=@photo.png' https://h");
  assert.deepEqual(d.request.body, { type: "form", fields: { name: "duck" } });
  assert.equal(d.warnings.length, 2);
});

test("variables survive, missing scheme defaults to http", () => {
  const { request } = parseCurl(`curl '{{base_url}}/users' -H 'Authorization: Bearer {{token}}'`);
  assert.equal(request.url, "{{base_url}}/users");
  assert.equal(parseCurl("curl localhost:3000/ping").request.url, "http://localhost:3000/ping");
});

test("errors", () => {
  assert.throws(() => parseCurl("wget https://x"), /Not a curl command/);
  assert.throws(() => parseCurl("curl -H 'a: b'"), /No URL/);
  assert.throws(() => parseCurl("curl 'https://x"), /Unterminated/);
});

test("export → import round trip", () => {
  const req = {
    name: "x",
    method: "POST",
    url: "https://api.example.com/items",
    headers: { "X-Duck": "it's quacking" },
    query: { page: "2", q: "a b" },
    body: { type: "json", content: '{\n  "a": 1\n}' },
    settings: { verify_ssl: false, timeout_ms: 1500 },
  };
  const curl = toCurl(req);
  assert.match(curl, /^curl -X POST \\\n {2}'https:\/\/api\.example\.com\/items\?page=2&q=a%20b'/);
  const back = parseCurl(curl).request;
  assert.equal(back.method, "POST");
  assert.equal(back.url, "https://api.example.com/items?page=2&q=a%20b");
  assert.equal(back.headers["X-Duck"], "it's quacking");
  assert.deepEqual(JSON.parse(back.body.content), { a: 1 });
  assert.equal(back.settings.verify_ssl, false);
  assert.equal(back.settings.timeout_ms, 1500);
  assert.equal(toCurl({ ...req, body: null, method: "GET", query: {}, headers: {}, settings: { follow_redirects: false } }, { multiline: false }),
    "curl https://api.example.com/items");
});

test("resolveRequest replaces known variables only", () => {
  const r = resolveRequest(
    { name: "x", method: "GET", url: "{{base}}/u/{{id}}?t={{$timestamp}}", headers: { A: "{{tok}}" } },
    { base: "https://h", id: "7", tok: "abc" },
  );
  assert.equal(r.url, "https://h/u/7?t={{$timestamp}}");
  assert.equal(r.headers.A, "abc");
});

test("auth import: digest, ntlm, aws-sigv4, oauth2-bearer", () => {
  assert.deepEqual(parseCurl("curl --digest -u bob:pw https://h").request.auth, { type: "digest", username: "bob", password: "pw" });
  assert.deepEqual(parseCurl("curl --ntlm -u 'CORP\\bob:pw' https://h").request.auth, {
    type: "ntlm",
    username: "bob",
    password: "pw",
    domain: "CORP",
  });
  assert.deepEqual(parseCurl("curl --ntlm -u bob:pw https://h").request.auth, { type: "ntlm", username: "bob", password: "pw" });
  assert.deepEqual(
    parseCurl(`curl --aws-sigv4 "aws:amz:eu-west-1:execute-api" -u AKIA:sec https://h`).request.auth,
    { type: "awsv4", access_key: "AKIA", secret_key: "sec", region: "eu-west-1", service: "execute-api" },
  );
  assert.deepEqual(parseCurl("curl --oauth2-bearer abc123 https://h").request.auth, { type: "bearer", token: "abc123" });
  assert.equal(parseCurl("curl https://h").request.auth, undefined);
});

test("auth export and round trip", () => {
  const base = { name: "x", method: "GET", url: "https://h/a", headers: {}, query: {}, body: null, settings: { follow_redirects: false } };
  const one = (auth) => toCurl({ ...base, auth }, { multiline: false });
  assert.equal(one({ type: "bearer", token: "t0k" }), "curl https://h/a -H 'Authorization: Bearer t0k'");
  assert.equal(one({ type: "basic", username: "u", password: "p w" }), "curl https://h/a -u 'u:p w'");
  assert.equal(one({ type: "digest", username: "u", password: "p" }), "curl https://h/a --digest -u u:p");
  assert.equal(one({ type: "ntlm", username: "u", password: "p", domain: "D" }), "curl https://h/a --ntlm -u 'D\\u:p'");
  assert.equal(one({ type: "api_key", key: "X-Key", value: "v", in: "header" }), "curl https://h/a -H 'X-Key: v'");
  assert.equal(one({ type: "api_key", key: "k", value: "v 1", in: "query" }), "curl 'https://h/a?k=v%201'");
  assert.equal(
    one({ type: "awsv4", access_key: "A", secret_key: "S", region: "r", service: "s" }),
    "curl https://h/a --aws-sigv4 aws:amz:r:s -u A:S",
  );
  assert.equal(
    one({ type: "oauth2", grant_type: "client_credentials", token_name: "t", client_id: "c", access_token: "lit" }),
    "curl https://h/a -H 'Authorization: Bearer lit'",
  );
  // Not expressible in curl: comment line in multiline mode only.
  const jwt = { type: "jwt", algorithm: "HS256", secret: "s", payload: "{}" };
  assert.equal(one(jwt), "curl https://h/a");
  assert.match(toCurl({ ...base, auth: jwt }), /^# auth jwt not exported\ncurl /);
  assert.equal(unexportedAuth(jwt), "jwt");
  assert.equal(unexportedAuth({ type: "oauth2", grant_type: "implicit", token_name: "t", client_id: "c" }), "oauth2");
  assert.equal(unexportedAuth({ type: "bearer", token: "x" }), null);
  // round trips
  for (const auth of [
    { type: "basic", username: "admin", password: "s3 cr'et" },
    { type: "digest", username: "u", password: "p" },
    { type: "ntlm", username: "u", password: "p", domain: "D" },
    { type: "awsv4", access_key: "A", secret_key: "S", region: "r", service: "s" },
  ]) {
    assert.deepEqual(parseCurl(toCurl({ ...base, auth })).request.auth, auth);
  }
});

test("resolveRequest interpolates auth fields", () => {
  const r = resolveRequest({ name: "x", method: "GET", url: "u", auth: { type: "bearer", token: "{{tok}}" } }, { tok: "abc" });
  assert.deepEqual(r.auth, { type: "bearer", token: "abc" });
});

console.log(`\n${n} curl tests passed`);
