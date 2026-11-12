// Self-test for lib/url — run from app/: node src/lib/url/selftest.mjs
import assert from "node:assert/strict";
import { tokenizeUrl } from "./tokens.ts";

let n = 0;
const test = (name, fn) => {
  fn();
  n++;
  console.log("ok", name);
};
const kinds = (url) => tokenizeUrl(url).map((t) => [t.kind, t.text]);

test("full URL with port, path param, query and fragment", () => {
  assert.deepEqual(kinds("https://api.example.com:8443/users/:id?page=2&q=a#top"), [
    ["scheme", "https://"],
    ["host", "api.example.com"],
    ["punct", ":"],
    ["port", "8443"],
    ["punct", "/"],
    ["path", "users"],
    ["punct", "/"],
    ["param", ":id"],
    ["punct", "?"],
    ["qkey", "page"],
    ["punct", "="],
    ["qval", "2"],
    ["punct", "&"],
    ["qkey", "q"],
    ["punct", "="],
    ["qval", "a"],
    ["fragment", "#top"],
  ]);
});

test("variables keep the section they are in", () => {
  const toks = tokenizeUrl("{{baseUrl}}/users/{{ id }}?token={{tok}}");
  assert.deepEqual(
    toks.map((t) => [t.kind, t.text, t.name]),
    [
      ["var", "{{baseUrl}}", "baseUrl"],
      ["punct", "/", undefined],
      ["path", "users", undefined],
      ["punct", "/", undefined],
      ["var", "{{ id }}", "id"],
      ["punct", "?", undefined],
      ["qkey", "token", undefined],
      ["punct", "=", undefined],
      ["var", "{{tok}}", "tok"],
    ],
  );
  assert.equal(toks[4].start, 18);
});

test("tokens cover the whole input exactly", () => {
  for (const url of ["", "localhost:3000", "/rel/path?x", "http://h/{{unclosed", "a{{b}}c{{$guid}}", "http://h?a=1=2&&b#x?y"]) {
    const toks = tokenizeUrl(url);
    assert.equal(toks.map((t) => t.text).join(""), url);
    let pos = 0;
    for (const t of toks) {
      assert.equal(t.start, pos);
      pos += t.text.length;
    }
  }
});

test("unclosed variable is plain text", () => {
  assert.ok(tokenizeUrl("http://h/{{x").every((t) => t.kind !== "var"));
});

console.log(`${n} tests passed`);
