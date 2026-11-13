#!/usr/bin/env node
// Publish a new version: bump it everywhere, commit, tag and push (D52).
//
//   node scripts/release.mjs              # asks major / medium / minor
//   node scripts/release.mjs minor        # or: major | medium | minor | 1.2.3
//   node scripts/release.mjs minor --yes  # no questions (CI-like)
//   node scripts/release.mjs --dry-run    # show what would happen, change nothing
//
// "major" → X.0.0, "medium" → x.Y.0, "minor" → x.y.Z.
// Pushing the tag starts the Release workflow: CI first, then the builds, and the
// release is published automatically when everything passes.

import { execFileSync, spawnSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { createInterface } from "node:readline/promises";
import { stdin, stdout, exit, argv, platform } from "node:process";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const BRANCH = "main";
const FILES = {
  cargo: join(ROOT, "Cargo.toml"),
  tauri: join(ROOT, "app/src-tauri/tauri.conf.json"),
  pkg: join(ROOT, "app/package.json"),
};

const args = argv.slice(2);
const yes = args.includes("--yes") || args.includes("-y");
const dryRun = args.includes("--dry-run");
const wanted = args.find((a) => !a.startsWith("-"));

const c = (code) => (s) => (stdout.isTTY ? `\x1b[${code}m${s}\x1b[0m` : s);
const bold = c(1);
const green = c(32);
const yellow = c(33);
const red = c(31);
const dim = c(2);

function fail(msg) {
  console.error(red(`✖ ${msg}`));
  exit(1);
}

/** Run a command; returns trimmed stdout (throws on failure). */
function run(cmd, cmdArgs, opts = {}) {
  return execFileSync(cmd, cmdArgs, { cwd: ROOT, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"], ...opts }).trim();
}

/** Run a command showing its output; fails the script if it fails. */
function step(label, cmd, cmdArgs, opts = {}) {
  console.log(dim(`→ ${label}`));
  if (dryRun) return;
  const r = spawnSync(cmd, cmdArgs, { cwd: ROOT, stdio: "inherit", shell: platform === "win32", ...opts });
  if (r.status !== 0) fail(`${label} failed`);
}

// Lines are queued as they arrive: with piped input (printf 'y\nn\n' | node …) readline
// emits them while commands run, and rl.question() would miss them.
const rl = createInterface({ input: stdin, output: stdout, terminal: stdin.isTTY });
const pending = [];
const waiting = [];
let closed = false;
rl.on("line", (line) => (waiting.length ? waiting.shift()(line) : pending.push(line)));
rl.on("close", () => {
  closed = true;
  while (waiting.length) waiting.shift()("");
});
async function ask(q) {
  stdout.write(q);
  if (pending.length) return pending.shift().trim();
  if (closed) return "";
  return (await new Promise((resolve) => waiting.push(resolve))).trim();
}
async function confirm(q, def = false) {
  if (yes) return true;
  const a = (await ask(`${q} ${def ? "[Y/n]" : "[y/N]"} `)).toLowerCase();
  return a === "" ? def : ["s", "si", "sí", "y", "yes"].includes(a);
}

// ---------- current version ----------

function currentVersion() {
  const toml = readFileSync(FILES.cargo, "utf8");
  const m = /^\[workspace\.package\][\s\S]*?^version\s*=\s*"([^"]+)"/m.exec(toml);
  if (!m) fail("no version in [workspace.package] of Cargo.toml");
  return m[1];
}

function parse(v) {
  const m = /^(\d+)\.(\d+)\.(\d+)$/.exec(v);
  if (!m) fail(`version "${v}" is not X.Y.Z`);
  return m.slice(1).map(Number);
}

function bump(v, kind) {
  const [a, b, p] = parse(v);
  if (kind === "major") return `${a + 1}.0.0`;
  if (kind === "medium") return `${a}.${b + 1}.0`;
  if (kind === "minor") return `${a}.${b}.${p + 1}`;
  return null;
}

// ---------- checks ----------

function preflight() {
  const branch = run("git", ["rev-parse", "--abbrev-ref", "HEAD"]);
  if (branch !== BRANCH) fail(`you are on "${branch}"; releases are made from "${BRANCH}"`);
  const dirty = run("git", ["status", "--porcelain"]);
  if (dirty) fail(`there are uncommitted changes; commit or stash them first:\n${dirty}`);
  try {
    run("git", ["fetch", "--quiet", "--tags", "origin", BRANCH]);
  } catch (e) {
    fail(`git fetch failed: ${e.message}`);
  }
  const [behind, ahead] = run("git", ["rev-list", "--left-right", "--count", `origin/${BRANCH}...HEAD`]).split(/\s+/).map(Number);
  if (behind > 0) fail(`your ${BRANCH} is ${behind} commit(s) behind origin; pull first`);
  return ahead;
}

// ---------- bump ----------

function writeVersion(next) {
  // Cargo.toml: only the [workspace.package] version.
  const toml = readFileSync(FILES.cargo, "utf8");
  const out = toml.replace(/(^\[workspace\.package\][\s\S]*?^version\s*=\s*")[^"]+(")/m, `$1${next}$2`);
  if (out === toml) fail("could not update Cargo.toml");
  writeFileSync(FILES.cargo, out);
  // JSON files: keep their formatting (indent) and trailing newline.
  for (const f of [FILES.tauri, FILES.pkg]) {
    const text = readFileSync(f, "utf8");
    const updated = text.replace(/("version"\s*:\s*")[^"]+(")/, `$1${next}$2`);
    if (updated === text) fail(`could not update ${f}`);
    writeFileSync(f, updated);
  }
}

// ---------- main ----------

async function main() {
  const current = currentVersion();
  console.log(bold(`ilixPatch release — current version ${current}`));

  const ahead = preflight();
  if (ahead > 0) console.log(yellow(`! ${ahead} local commit(s) not pushed yet; they go out with the release.`));

  let next;
  if (wanted && /^\d+\.\d+\.\d+$/.test(wanted)) next = wanted;
  else if (wanted) next = bump(current, wanted) ?? fail(`unknown "${wanted}" (use major, medium, minor or X.Y.Z)`);
  else {
    if (yes) fail("say which part to bump: major | medium | minor");
    console.log(`  1) major   ${current} → ${bold(bump(current, "major"))}   (incompatible changes)`);
    console.log(`  2) medium  ${current} → ${bold(bump(current, "medium"))}   (new features)`);
    console.log(`  3) minor   ${current} → ${bold(bump(current, "minor"))}   (fixes)`);
    const a = (await ask("Which one? [1/2/3] ")).toLowerCase();
    const kind = { 1: "major", 2: "medium", 3: "minor", major: "major", medium: "medium", minor: "minor" }[a];
    if (!kind) fail("cancelled");
    next = bump(current, kind);
  }

  const tag = `v${next}`;
  if (run("git", ["tag", "--list", tag])) fail(`tag ${tag} already exists`);
  const cmp = (x, y) => parse(x).reduce((acc, n, i) => acc || n - parse(y)[i], 0);
  if (cmp(next, current) <= 0) fail(`${next} is not greater than ${current}`);

  console.log(`\nNew version: ${bold(next)}  (tag ${tag})${dryRun ? yellow("  [dry run]") : ""}`);
  if (!(await confirm("Continue?", true))) fail("cancelled");

  if (!dryRun) writeVersion(next);
  console.log(green(`✔ version ${next} in Cargo.toml, tauri.conf.json and package.json`));
  step("update package-lock.json", "npm", ["install", "--package-lock-only", "--no-audit", "--no-fund"], { cwd: join(ROOT, "app") });
  step("update Cargo.lock", "cargo", ["update", "--workspace"]);
  step("regenerate THIRD_PARTY_LICENSES", "node", ["scripts/third-party-licenses.mjs"]);

  if (await confirm("Run the local checks first (clippy, tests, app check)? CI runs them anyway.", false)) {
    step("cargo clippy", "cargo", ["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"]);
    step("cargo test", "cargo", ["test", "--workspace"]);
    step("npm run check", "npm", ["run", "check"], { cwd: join(ROOT, "app") });
  }

  step("commit", "git", ["commit", "-am", `Release ${next}`]);
  step("tag", "git", ["tag", "-a", tag, "-m", `ilixPatch ${next}`]);

  if (!(await confirm(`Push ${BRANCH} and ${tag}? This starts the Release workflow.`, true))) {
    console.log(yellow(`Not pushed. When ready: git push origin ${BRANCH} && git push origin ${tag}`));
    return;
  }
  step(`push ${BRANCH}`, "git", ["push", "origin", BRANCH]);
  step(`push ${tag}`, "git", ["push", "origin", tag]);

  let repo = "";
  try {
    repo = run("git", ["remote", "get-url", "origin"]).replace(/\.git$/, "").replace(/^git@github\.com:/, "https://github.com/");
  } catch {
    // ignore
  }
  console.log(green(`\n✔ ${tag} pushed.`));
  console.log(`The Release workflow runs CI, builds and publishes it if everything passes.`);
  if (repo.startsWith("https://github.com/")) console.log(`Follow it: ${repo}/actions`);
}

main()
  .catch((e) => fail(e?.message ?? String(e)))
  .finally(() => rl.close());
