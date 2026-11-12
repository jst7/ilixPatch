// Self-test for lib/preview — run from app/: node src/lib/preview/selftest.mjs
import assert from "node:assert/strict";
import { previewFromMime, sniffPreview, isGenericMime } from "./kind.ts";

let n = 0;
const test = (name, fn) => {
  fn();
  n++;
  console.log("ok", name);
};
const bytes = (...parts) =>
  new Uint8Array(parts.flatMap((p) => (typeof p === "string" ? [...p].map((c) => c.charCodeAt(0)) : p)));
const kindOf = (b) => {
  const r = sniffPreview(b);
  return r && `${r.kind} ${r.mime}`;
};

test("Content-Type with parameters", () => {
  assert.deepEqual(previewFromMime("image/PNG; charset=binary"), { kind: "image", mime: "image/png", ext: "png" });
  assert.equal(previewFromMime("application/pdf").kind, "pdf");
  assert.equal(previewFromMime("text/html; charset=utf-8").kind, "html");
  assert.equal(previewFromMime("video/mp4").kind, "video");
  assert.equal(previewFromMime("audio/mpeg").kind, "audio");
  assert.equal(previewFromMime("application/json"), null);
  assert.equal(previewFromMime(null), null);
});

test("generic types need sniffing", () => {
  assert.ok(isGenericMime(null));
  assert.ok(isGenericMime("application/octet-stream"));
  assert.ok(!isGenericMime("image/png"));
});

test("magic numbers", () => {
  assert.equal(kindOf(bytes([0x89], "PNG\r\n\x1a\n")), "image image/png");
  assert.equal(kindOf(bytes([0xff, 0xd8, 0xff, 0xe0])), "image image/jpeg");
  assert.equal(kindOf(bytes("GIF89a")), "image image/gif");
  assert.equal(kindOf(bytes("RIFF", [0, 0, 0, 0], "WEBPVP8 ")), "image image/webp");
  assert.equal(kindOf(bytes("RIFF", [0, 0, 0, 0], "WAVEfmt ")), "audio audio/wav");
  assert.equal(kindOf(bytes("%PDF-1.7\n")), "pdf application/pdf");
  assert.equal(kindOf(bytes("OggS", [0])), "audio audio/ogg");
  assert.equal(kindOf(bytes("ID3", [4, 0])), "audio audio/mpeg");
  assert.equal(kindOf(bytes([0x1a, 0x45, 0xdf, 0xa3])), "video video/webm");
  assert.equal(kindOf(bytes([0, 0, 0, 0x20], "ftypisom")), "video video/mp4");
  assert.equal(kindOf(bytes([0, 0, 0, 0x20], "ftypavif")), "image image/avif");
  assert.equal(kindOf(bytes('<?xml version="1.0"?>\n<svg xmlns="http://www.w3.org/2000/svg">')), "image image/svg+xml");
  assert.equal(kindOf(bytes('{"a":1}')), null);
  assert.equal(kindOf(bytes([])), null);
});

console.log(`${n} tests passed`);
