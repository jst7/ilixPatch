// Self-test for match.ts. Run: node app/src/lib/search/selftest.mjs (Node >= 23.6 strips TS types).
import assert from "node:assert/strict";
import { allTermsIn, highlight, normalize, parseQuery, prepare, ranges } from "./match.ts";

// normalize: case + accents
assert.equal(normalize("Petición ÁÉÍÓÚ Ñandú"), "peticion aeiou nandu");

// parseQuery: AND terms + filters
assert.deepEqual(parseQuery("  Usuarios  method:POST in:Admin  crear "), {
  terms: ["usuarios", "crear"],
  method: "post",
  within: ["admin"],
});
assert.deepEqual(parseQuery("method:"), { terms: ["method:"], method: null, within: [] });

// allTermsIn: AND across haystacks
assert.ok(allTermsIn(["users", "get"], ["list users", "get"]));
assert.ok(!allTermsIn(["users", "post"], ["list users", "get"]));

// ranges map back to the original text (accented chars)
const p = prepare("Crear petición");
assert.deepEqual(ranges(p, ["peticion"]), [[6, 14]]);
// Decomposed input (e + combining acute) still highlights the whole grapheme
const d = prepare("café bar");
assert.deepEqual(ranges(d, ["cafe"]), [[0, 5]]); // includes the combining mark
// overlapping / repeated terms merge
assert.deepEqual(ranges(prepare("aaaa"), ["aa"]), [[0, 4]]);
assert.deepEqual(ranges(prepare("Get users, get"), ["get"]), [[0, 3], [11, 14]]);

// highlight parts
assert.deepEqual(highlight("Hola Mundo", ["mun"]), [
  { text: "Hola ", hit: false },
  { text: "Mun", hit: true },
  { text: "do", hit: false },
]);
assert.deepEqual(highlight("abc", []), [{ text: "abc", hit: false }]);

// performance: 20k prepared names, 3-term AND scan well under a frame budget per query
const names = Array.from({ length: 20000 }, (_, i) => normalize(`Request ${i} usuarios método ${i % 7}`));
const t0 = performance.now();
let n = 0;
for (const h of names) if (allTermsIn(["usuarios", "metodo", "3"], [h])) n++;
const ms = performance.now() - t0;
assert.ok(n > 0);
assert.ok(ms < 50, `scan took ${ms} ms`);

console.log(`search selftest: ok (20k scan ${ms.toFixed(1)} ms)`);
