#!/usr/bin/env node
// Generates THIRD_PARTY_LICENSES: the licence notices of every third-party component
// that ends up inside the ilixPatch app and CLI binaries.
//
//   node scripts/third-party-licenses.mjs           write THIRD_PARTY_LICENSES + app/static copy
//   node scripts/third-party-licenses.mjs --check   exit 1 if the committed file is out of date
//
// Included:
//   - Rust crates reachable through normal (runtime) dependencies of the workspace members,
//     for all target platforms (dev-dependencies, build scripts and proc-macros are compile-time
//     only and are not shipped).
//   - npm packages reachable from app/package.json "dependencies", plus the Svelte runtime
//     (a devDependency whose runtime is compiled into the bundle).
//   - Code vendored or embedded in the binaries: chai, QuickJS, SQLite.
// Licence texts are taken from each package's own LICENSE/COPYING/NOTICE files. Identical texts
// (e.g. the Apache-2.0 text shipped by hundreds of crates) are printed once with the list of users.

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const OUT = path.join(ROOT, "THIRD_PARTY_LICENSES");
const OUT_STATIC = path.join(ROOT, "app", "static", "THIRD_PARTY_LICENSES.txt");
const CHECK = process.argv.includes("--check");

const LICENSE_FILE = /^(licen[cs]e|copying|notice|unlicense|copyright)([-_.].*)?$/i;

/** @type {{ name: string, version: string, license: string, source: string, authors: string[], dir: string | null, extraFiles?: string[] }[]} */
const components = [];

// ---------- Rust ----------
/** cargo from PATH, or rustup's default location (GUI shells often lack ~/.cargo/bin in PATH). */
function cargoBin() {
  const home = process.env.CARGO_HOME ?? path.join(process.env.HOME ?? process.env.USERPROFILE ?? "", ".cargo");
  const candidates = ["cargo", path.join(home, "bin", process.platform === "win32" ? "cargo.exe" : "cargo")];
  for (const c of candidates) {
    try {
      execFileSync(c, ["--version"], { stdio: "ignore" });
      return c;
    } catch {}
  }
  return null;
}

function rustComponents() {
  const cargo = cargoBin();
  if (!cargo) throw new CargoMissing();
  let meta;
  try {
    meta = JSON.parse(
      execFileSync(cargo, ["metadata", "--format-version", "1", "--locked"], {
        cwd: ROOT,
        maxBuffer: 256 * 1024 * 1024,
        stdio: ["ignore", "pipe", "inherit"],
      }).toString(),
    );
  } catch (e) {
    throw new Error(`cargo metadata failed (is Rust installed?): ${e.message}`);
  }
  const pkgs = new Map(meta.packages.map((p) => [p.id, p]));
  const nodes = new Map(meta.resolve.nodes.map((n) => [n.id, n]));
  const members = new Set(meta.workspace_members);
  const isProcMacro = (p) => p.targets.some((t) => t.kind.includes("proc-macro"));

  const seen = new Set();
  const stack = [...members];
  while (stack.length) {
    const id = stack.pop();
    if (seen.has(id)) continue;
    seen.add(id);
    for (const dep of nodes.get(id)?.deps ?? []) {
      // kind null = normal dependency (build/dev are not shipped)
      if (!dep.dep_kinds.some((k) => k.kind === null)) continue;
      const p = pkgs.get(dep.pkg);
      if (p && !isProcMacro(p)) stack.push(dep.pkg);
    }
  }
  for (const id of seen) {
    if (members.has(id)) continue;
    const p = pkgs.get(id);
    components.push({
      name: p.name,
      version: p.version,
      license: p.license ?? (p.license_file ? "see license file" : "UNKNOWN"),
      source: p.repository ?? `https://crates.io/crates/${p.name}`,
      authors: p.authors ?? [],
      dir: path.dirname(p.manifest_path),
      extraFiles: p.license_file ? [path.join(path.dirname(p.manifest_path), p.license_file)] : [],
    });
  }
}

class CargoMissing extends Error {}

// ---------- npm ----------
function npmComponents() {
  const appDir = path.join(ROOT, "app");
  const pkgJson = JSON.parse(fs.readFileSync(path.join(appDir, "package.json"), "utf8"));
  const seen = new Set();
  const resolve = (name, fromDir) => {
    for (let d = fromDir; ; d = path.dirname(d)) {
      const f = path.join(d, "node_modules", name, "package.json");
      if (fs.existsSync(f)) return f;
      if (d === path.dirname(d) || d.length < appDir.length) return null;
    }
  };
  const walk = (name, fromDir) => {
    const file = resolve(name, fromDir);
    if (!file) throw new Error(`npm package ${name} not installed (run npm ci in app/)`);
    if (seen.has(file)) return;
    seen.add(file);
    const j = JSON.parse(fs.readFileSync(file, "utf8"));
    const lic = typeof j.license === "string" ? j.license : (j.license?.type ?? j.licenses?.map((l) => l.type).join(" OR ") ?? "UNKNOWN");
    const repo = typeof j.repository === "string" ? j.repository : j.repository?.url;
    const author = typeof j.author === "string" ? j.author : j.author?.name;
    components.push({
      name: j.name,
      version: j.version,
      license: lic,
      source: (repo ?? `https://www.npmjs.com/package/${j.name}`).replace(/^git\+/, ""),
      authors: author ? [author] : [],
      dir: path.dirname(file),
    });
    for (const dep of Object.keys(j.dependencies ?? {})) walk(dep, path.dirname(file));
  };
  for (const dep of Object.keys(pkgJson.dependencies ?? {})) walk(dep, appDir);
  walk("svelte", appDir); // runtime compiled into the bundle
}

// ---------- vendored / embedded ----------
function vendoredComponents() {
  const quickjs = components.find((c) => c.name === "rquickjs-sys");
  if (quickjs?.dir) {
    components.push({
      name: "QuickJS (embedded via rquickjs-sys)",
      version: quickjs.version,
      license: "MIT",
      source: "https://bellard.org/quickjs/",
      authors: ["Fabrice Bellard", "Charlie Gordon"],
      dir: null,
      extraFiles: [path.join(quickjs.dir, "quickjs", "LICENSE")].filter((f) => fs.existsSync(f)),
    });
  }
  components.push({
    name: "chai (vendored in crates/scripting)",
    version: "4.5.0",
    license: "MIT",
    source: "https://github.com/chaijs/chai",
    authors: ["Jake Luer"],
    dir: null,
    extraFiles: [path.join(ROOT, "crates", "scripting", "vendor", "LICENSE-chai")],
  });
  if (components.some((c) => c.name === "libsqlite3-sys")) {
    components.push({
      name: "SQLite (embedded via libsqlite3-sys)",
      version: "bundled",
      license: "blessing (public domain)",
      source: "https://sqlite.org/copyright.html",
      authors: [],
      dir: null,
      inlineText:
        "The author disclaims copyright to this source code. In place of a legal notice, here is a blessing:\n\n" +
        "    May you do good and not evil.\n    May you find forgiveness for yourself and forgive others.\n" +
        "    May you share freely, never taking more than you give.",
    });
  }
}

// ---------- licence texts ----------
const MIT_TEMPLATE = (holders) => `MIT License

Copyright (c) ${holders}

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.`;

function licenseFiles(c) {
  const files = new Set(c.extraFiles ?? []);
  if (c.dir && fs.existsSync(c.dir)) {
    for (const f of fs.readdirSync(c.dir)) {
      const full = path.join(c.dir, f);
      if (LICENSE_FILE.test(f) && fs.statSync(full).isFile()) files.add(full);
    }
  }
  return [...files].sort();
}

const normalize = (s) => s.replace(/\r\n?/g, "\n").replace(/[ \t]+$/gm, "").trim();
const hashOf = (s) => createHash("sha256").update(s.replace(/\s+/g, " ").toLowerCase()).digest("hex");

function build() {
  rustComponents();
  npmComponents();
  vendoredComponents();

  // Dedupe identical name@version (e.g. the same crate reached twice).
  const byKey = new Map();
  for (const c of components) byKey.set(`${c.name}@${c.version}`, c);
  const list = [...byKey.values()].sort((a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version));

  /** text hash -> { text, users[] } */
  const texts = new Map();
  const missing = [];
  const addText = (text, user) => {
    const t = normalize(text);
    if (!t) return;
    const h = hashOf(t);
    if (!texts.has(h)) texts.set(h, { text: t, users: [] });
    texts.get(h).users.push(user);
  };

  for (const c of list) {
    const user = `${c.name} ${c.version}`;
    const files = licenseFiles(c);
    if (c.inlineText) addText(c.inlineText, user);
    for (const f of files) addText(fs.readFileSync(f, "utf8"), user);
    if (!files.length && !c.inlineText) {
      if (/\bMIT\b/.test(c.license)) {
        const holders = c.authors.length ? c.authors.map((a) => a.replace(/\s*<[^>]*>/, "")).join(", ") : `the ${c.name} authors`;
        addText(MIT_TEMPLATE(holders), user);
      } else {
        missing.push(c);
      }
    }
  }

  const lines = [];
  lines.push("THIRD-PARTY SOFTWARE NOTICES AND LICENSES");
  lines.push("=========================================");
  lines.push("");
  lines.push("ilixPatch (https://github.com/jst7/ilixPatch), © Jorge Segovia Tormo and contributors,");
  lines.push("includes the third-party components listed below. Each component is distributed under");
  lines.push("its own license; the full license texts follow the list. Where a component is offered");
  lines.push("under a choice of licenses (\"A OR B\"), ilixPatch uses it under any of those options.");
  lines.push("");
  lines.push("This file is generated by scripts/third-party-licenses.mjs — do not edit by hand.");
  lines.push("");
  lines.push(`COMPONENTS (${list.length})`);
  lines.push("-".repeat(40));
  const w = Math.min(48, Math.max(...list.map((c) => `${c.name} ${c.version}`.length)));
  for (const c of list) lines.push(`${`${c.name} ${c.version}`.padEnd(w)}  ${c.license}`);
  lines.push("");
  if (missing.length) {
    lines.push("Components without a bundled license file (standard SPDX text applies, see");
    lines.push("https://spdx.org/licenses/ ; the full text is also included below for most of them):");
    for (const c of missing) lines.push(`  - ${c.name} ${c.version}: ${c.license} (${c.source})`);
    lines.push("");
  }
  lines.push("");
  lines.push(`LICENSE TEXTS (${texts.size} distinct)`);
  lines.push("-".repeat(40));
  const sorted = [...texts.values()].sort((a, b) => b.users.length - a.users.length || a.users[0].localeCompare(b.users[0]));
  for (const { text, users } of sorted) {
    lines.push("");
    lines.push("=".repeat(80));
    lines.push(`Used by: ${users.join(", ")}`);
    lines.push("=".repeat(80));
    lines.push("");
    lines.push(text);
  }
  lines.push("");
  return { content: lines.join("\n"), count: list.length, distinct: texts.size, missing };
}

let result;
try {
  result = build();
} catch (e) {
  if (!(e instanceof CargoMissing) || CHECK) throw e;
  // Frontend-only environments: keep the committed file instead of failing the build.
  if (!fs.existsSync(OUT)) throw new Error("cargo not found and no THIRD_PARTY_LICENSES to fall back to");
  fs.mkdirSync(path.dirname(OUT_STATIC), { recursive: true });
  fs.copyFileSync(OUT, OUT_STATIC);
  console.warn("THIRD_PARTY_LICENSES: cargo not found — using the committed file (may be out of date).");
  process.exit(0);
}
const { content, count, distinct, missing } = result;
if (CHECK) {
  const current = fs.existsSync(OUT) ? fs.readFileSync(OUT, "utf8") : "";
  if (current !== content) {
    console.error("THIRD_PARTY_LICENSES is out of date: run `node scripts/third-party-licenses.mjs`");
    process.exit(1);
  }
  console.log(`THIRD_PARTY_LICENSES up to date (${count} components).`);
} else {
  fs.writeFileSync(OUT, content);
  fs.mkdirSync(path.dirname(OUT_STATIC), { recursive: true });
  fs.writeFileSync(OUT_STATIC, content);
  console.log(
    `THIRD_PARTY_LICENSES: ${count} components, ${distinct} distinct license texts, ` +
      `${(content.length / 1024).toFixed(0)} KB` +
      (missing.length ? `, ${missing.length} without a license file (listed)` : ""),
  );
}
