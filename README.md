<p align="center">
  <img src="app/src-tauri/icons/128x128@2x.png" width="128" height="128" alt="ilixPatch logo: a duck wearing rain boots">
</p>

<h1 align="center">ilixPatch</h1>

<p align="center">
  A fast, open-source, <b>local-first API client</b> — collections, environments, scripts, tests,
  load testing and a built-in Git client. No account, no cloud: your requests are plain files in your repo.
</p>

<p align="center">
  <a href="https://github.com/jst7/ilixPatch/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/jst7/ilixPatch/actions/workflows/ci.yml/badge.svg"></a>
  <a href="https://github.com/jst7/ilixPatch/releases"><img alt="Release" src="https://img.shields.io/github/v/release/jst7/ilixPatch?include_prereleases&sort=semver"></a>
  <a href="#license"><img alt="License" src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue"></a>
</p>

<p align="center"><a href="README.es.md">Leer en castellano</a></p>

> **Status: early development (0.x).** Things move fast and the on-disk format may still change

## Why ilixPatch
- **Fast and light**: Rust core + [Tauri](https://tauri.app) (the OS webview, not a bundled Chromium).
  Starts quickly, uses little memory, and keeps big responses in Rust.
- **Local-first**: no sign-up, no sync. Everything lives in your project folder.
- **Git-native**: one TOML file per request → clean diffs, easy reviews, and an IntelliJ-style Git client inside the app.
- **Postman-compatible where it matters**: `pm.*` scripts and Chai assertions run unchanged, a Newman-compatible CLI,
  and an importer for Postman collections, environments and data dumps.

## Features
**Requests & editor**
- HTTP methods with colour-coded selector, multi-line URL, params, headers, JSON/text/form bodies.
- CodeMirror editors: JSON highlighting, folding, regex + multi-line search & replace, `pm` autocompletion.
- Cancel a hung request instantly (`⌘.` / `Ctrl+.`); **descriptive errors** that explain the likely cause
  (DNS, connection refused, timeout, TLS issuer/expiry/hostname, mTLS, proxy…) with one-click fixes.
- Response viewer: status, time, size, headers, tests, console, HTTP version, redirect chain and TLS certificate.

**Organisation**
- **Project › Collection › Folder › Request**, each project usually a Git repository.
- **Project environments** (shared via Git; secrets kept in a git-ignored `.secret.toml`) and **global environments** (per user),
  copyable both ways. Collection & environment variables with Postman precedence.
- Open requests as tabs in a collapsible right sidebar; search the tree, the tabs and the history.
- **History** of every call with its response (local SQLite, retention by days and size), restorable into a new tab.

**Scripts, tests and automation**
- Pre-request and test scripts with the Postman `pm.*` API on an embedded QuickJS engine (Chai `expect`, `pm.response`,
  variables, `setNextRequest`, legacy `tests[...]`).
- **Collection runner** (iterations, CSV/JSON data, delays, bail) and **load testing** (virtual users, fixed/ramp/spike/peak
  profiles, p50–p99 latency, live charts).
- **CLI** `ilixpatch` — a Newman-compatible runner for CI with JUnit/JSON/HTML reports and load-test thresholds.

**Git**
- Any project, collection or folder can be its own repository with its own branch (badge in the tree).
- Changes with semantic request diffs, commit, branches, fetch/pull/push, history with commit details,
  revert / reset / cherry-pick, **3-way merge conflict resolver**, **Annotate (blame)** in the editors, clone from the UI.

**Network**
- SSL verification, extra CA certificates, **client certificates (mTLS)** per host (PEM or PKCS#12), proxy (system/custom,
  http/https/socks5), redirects, timeouts, TLS version, HTTP/1.1 vs HTTP/2 — globally and per request.
  CORS doesn't apply: requests are sent by the Rust engine, not the browser.

**Make it yours**
- Light/dark/system themes and fully customizable colours (export/import), English and Castellano UI (more languages welcome).

## Download
Pre-built installers and CLI binaries are published on the **[Releases](https://github.com/jst7/ilixPatch/releases)** page:

| Platform | App | CLI |
|---|---|---|
| macOS (Apple Silicon / Intel) | `.dmg` | `ilixpatch-<version>-aarch64-apple-darwin.tar.gz` / `x86_64-apple-darwin` |
| Windows | `.msi` / `.exe` | `ilixpatch-<version>-x86_64-pc-windows-msvc.zip` |
| Linux | `.deb` / `.rpm` / `.AppImage` | `ilixpatch-<version>-x86_64-unknown-linux-gnu.tar.gz` / `aarch64` |

Early builds may be **unsigned**: on macOS, right-click the app → *Open* the first time (or
`xattr -dr com.apple.quarantine /Applications/ilixPatch.app`); on Windows, SmartScreen → *More info* → *Run anyway*.

## Build from source
### The quick way
```sh
git clone https://github.com/jst7/ilixPatch.git
cd ilixPatch
./init.sh
```
`init.sh` detects your OS (macOS, Debian/Ubuntu, Fedora, Arch, openSUSE, WSL, Git Bash), checks the prerequisites,
offers to install what's missing (it asks first), installs the frontend dependencies and starts the app.

| Command | What it does |
|---|---|
| `./init.sh` | Check + install missing prerequisites + run the desktop app in dev mode |
| `./init.sh --check` | Only check prerequisites; changes nothing |
| `./init.sh --yes` | Install missing prerequisites without asking |
| `./init.sh --cli` | Run the CLI against the demo project |
| `./init.sh --build` | Build release installers into `target/release/bundle/` |

### Manual setup
Requirements: [Rust](https://rustup.rs) ≥ 1.85, Node.js ≥ 24 (LTS), Git, and the
[Tauri prerequisites](https://tauri.app/start/prerequisites/) for your OS (Xcode Command Line Tools on macOS;
WebKitGTK 4.1 & friends on Linux; MSVC Build Tools + WebView2 on Windows).

```sh
cd app
npm ci
npm run tauri dev        # desktop app with hot reload
npm run tauri build      # installers in ../target/release/bundle/

cd ..
cargo run -p ilixpatch-cli -- run examples/demo-project --env dev   # CLI
cargo build --release -p ilixpatch-cli                             # → target/release/ilixpatch
```

### Try it
Open the bundled demo with **+ › Open folder…** → `examples/demo-project`, pick the `dev` environment and press `⌘/Ctrl+Enter`.
Or import your own work with **+ › Import from Postman…**.

## CLI (Newman-compatible runner and load testing)
`ilixpatch run` runs a project, collection, folder or single request `.toml`,
including pre-request and test scripts (`pm.*` API, like Postman/Newman):

```sh
ilixpatch run examples/demo-project --env dev
ilixpatch run examples/demo-project/jsonplaceholder/users \
  -e dev -d examples/demo-project/data/users.csv \
  -r cli,junit,html --reporter-junit-export results/junit.xml \
  --reporter-html-export results/report.html
```

| Flag | Meaning |
|---|---|
| `-e, --env <name\|file>` | Project environment (`.ilixpatch/environments/`), else a legacy collection environment (`<collection>/environments/`), else a path to an environment `.toml` / Postman environment `.json` |
| `--global-env <name>` | Use one of the desktop app's global (user-level) environments instead |
| `--no-globals` | Don't load the desktop app's `globals.toml` as `pm.globals` |
| `-n, --iteration-count <n>` | Iterations (default: rows of the data file, or 1) |
| `-d, --iteration-data <file>` | CSV (header row) or JSON (array of objects); one iteration per row (`pm.iterationData`, `{{column}}`) |
| `--delay-request <ms>` / `--timeout-request <ms>` | Delay between requests / per-request timeout |
| `--bail` | Stop on the first failed request or test |
| `--folder <name>` | Only run this folder (repeatable) |
| `-r, --reporters cli,json,junit,html` | Reporters; `--reporter-{json,junit,html}-export <path>` set the output files |
| `--export-environment <path>` | Write the final environment (Postman JSON) |
| `--global-var k=v`, `--env-var k=v` | Set globals / override environment variables (repeatable) |
| `-k, --insecure` | Accept invalid TLS certificates |
| `--ssl-client-cert <pem\|p12>`, `--ssl-client-key <pem>`, `--ssl-client-passphrase <p>` | Client certificate (mTLS) for every host |
| `--ssl-client-cert-list <json>` | Newman list: `[{name, matches: ["https://api.example.com/*"], cert, key, pfx, passphrase}]` |
| `--ssl-extra-ca-certs <pem>` | Extra trusted CA certificates |
| `--ignore-redirects` | Don't follow redirects |
| `--proxy <url>` (+ `--proxy-bypass <hosts>`), `--no-proxy` | Custom proxy (http, https, socks5, socks5h) / ignore system and env proxies |
| `--use-app-settings` | Start from the desktop app's network settings (`network.toml`); the flags above override them |
| `--suppress-exit-code`, `--verbose` | Always exit 0 / print response headers and bodies |

Variables resolve with precedence local > iteration data > environment >
collection > globals, and script changes persist across requests of the run.

The CLI shares global environments and globals with the desktop app: they live
in the app config dir (`<OS config dir>/dev.ilixpatch.app/`, e.g.
`~/Library/Application Support/dev.ilixpatch.app` on macOS,
`~/.config/dev.ilixpatch.app` on Linux, `%APPDATA%\dev.ilixpatch.app` on
Windows), as `environments/<name>.toml` and `globals.toml`. Set
`ILIXPATCH_CONFIG_DIR` to use another directory (e.g. in CI).
`postman.setNextRequest(name | null)` and `pm.execution.skipRequest()` work.
Exit code: `1` if any request errored or any test failed (a non-2xx status
without tests is not a failure, as in Newman), `2` on usage errors.

`ilixpatch load` runs virtual users that loop over the target's requests:

```sh
ilixpatch load examples/demo-project/jsonplaceholder --vus 5 --duration 30s \
  --profile ramp-up --ramp-up 10s --think-time 100ms \
  --max-p95 500 --max-error-rate 0.01      # CI thresholds → exit 1 if exceeded
```

Profiles: `fixed`, `ramp-up` (linear up to `--vus`), `spike` (20% of the VUs,
100% during the middle 20% of the test) and `peak` (ramp up, hold, ramp down).
A request counts as an error on a network error, a status outside 2xx/3xx or a
failed test. `--no-scripts` skips scripts for maximum throughput. Please be
gentle with public APIs.

### Importing from Postman
In the app: **+ › Import from Postman…** — pick what to import and drag it wherever you want in your project.
From the command line:
```sh
ilixpatch import postman my-collection.postman_collection.json dev.postman_environment.json \
  --project ./my-project [--into ./my-project/api] [--env-scope project|global] [--on-conflict rename|overwrite|skip]
```
Collections v2.0/v2.1, environments, globals and the *Export data* `.zip` are supported. Auth (incl. inherited) is converted
to headers/scripts, collection/folder scripts can be inlined, and secret-looking variables go to git-ignored files.

## Project format
Everything is plain TOML in your repo: **Project > Collection > Folder > Request**.

```text
my-project/                  # usually a git repo
├── .ilixpatch/
│   └── config.toml          # marks the project root
├── users-api/               # a collection = any subdirectory with collection.toml
│   ├── collection.toml      # name, format version, collection variables
│   ├── environments/
│   │   ├── dev.toml         # [variables]
│   │   └── dev.secret.toml  # gitignored; overrides dev.toml (tokens, passwords)
│   └── users/               # folders nest freely
│       └── list-users.toml  # one request per file
└── billing-api/
    └── collection.toml
```

`.ilixpatch/config.toml`:

```toml
version = 1
name = "My API"
# optional, relative to the project root; default "."
collections_dir = "."
```

Collections are the direct subdirectories of `collections_dir` that contain a
`collection.toml` (hidden directories, `node_modules` and `target` are skipped).
Adding a folder in the app creates `.ilixpatch/config.toml` for you if it is missing.
The CLI accepts either a project or a single collection directory; in a project,
`--env` applies to every collection that defines that environment.

A request file:

```toml
name = "Create post"
method = "POST"
url = "{{base_url}}/posts"

[headers]
Accept = "application/json"

[body]
type = "json"
content = '''{ "userId": {{user_id}} }'''
```

Variables resolve with increasing priority: collection → environment → secrets.

## Keyboard shortcuts
| Shortcut (macOS / Windows·Linux) | Action |
|---|---|
| `⌘ Enter` / `Ctrl Enter` | Send the request (Enter in the URL field too) |
| `⌘ .` / `Ctrl .` | Cancel the request in progress |
| `⌘ S` / `Ctrl S` | Save |
| `⌘ W` / `Ctrl W` | Close tab |
| `Ctrl Tab` / `Ctrl Shift Tab` | Next / previous tab |
| `⌘ \` / `Ctrl \` | Toggle the tabs sidebar |
| `⌘ P` / `Ctrl P` | Search the project tree (or the history) |
| `⌘ E` / `Ctrl E` | Search open tabs |
| `⌘ F` / `Ctrl F` | Find in the editor (regex, multi-line) |
| `⌘ ⌥ F` / `Ctrl Alt F` (or `Ctrl H`) | Find & replace |
| `F7` / `Shift F7` | Next / previous conflict in the merge resolver |

## Architecture
A Cargo workspace; the UI and the CLI are thin shells over UI-agnostic crates.

| Crate | Purpose |
|---|---|
| `crates/core` | Data model, `{{variables}}`, on-disk format, projects, environments |
| `crates/engine` | HTTP engine (reqwest + rustls): TLS/mTLS, proxy, redirects, timing, error classification |
| `crates/scripting` | Postman-compatible `pm.*` sandbox on QuickJS (+ Chai) |
| `crates/runner` | Request pipeline, Newman-compatible collection runner, load testing, reporters |
| `crates/history` | Request history in SQLite with retention |
| `crates/git` | Multi-repository Git client (gix for reads, system `git` for writes and network) |
| `crates/importer` | Postman importer |
| `crates/cli` | The `ilixpatch` binary |
| `app/` | Tauri v2 desktop app: Svelte 5 frontend in `app/src`, Rust commands in `app/src-tauri` |

Design decisions (and the reasons behind them) are recorded in **[docs/DECISIONS.md](docs/DECISIONS.md)** (Spanish).

## Development
```sh
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
cd app && npm run check && npm run build
node src/lib/merge/selftest.mjs && node src/lib/import/selftest.mjs && node src/lib/search/selftest.mjs
```
- **CI** (`.github/workflows/ci.yml`) runs all of the above on Linux, macOS and Windows, plus an end-to-end CLI run of the demo project.
- **Third-party licenses**: `THIRD_PARTY_LICENSES` is generated by `node scripts/third-party-licenses.mjs` (also on every
  `npm run build`) and shown in the app under *About › Third-party licenses*. CI fails if it's out of date.
- **App icon**: edit `app/src-tauri/icons/icon.svg`, then `cd app && npx tauri icon src-tauri/icons/icon.svg`.

### Releasing
1. Bump the version in `Cargo.toml` (`[workspace.package]`) and `app/src-tauri/tauri.conf.json`.
2. Commit, then tag and push: `git tag v0.2.0 && git push origin v0.2.0`.
3. The **Release** workflow builds the installers (macOS arm64 + x64, Windows, Linux) and the CLI for 5 targets,
   and creates a **draft** GitHub Release. Review it and publish.
4. Optional signing: add the `APPLE_*` secrets listed at the top of `.github/workflows/release.yml` for signed and
   notarized macOS builds; Windows signing is configured via `bundle.windows.signCommand`.

## Contributing
Issues and pull requests are welcome! Please skim [docs/DECISIONS.md](docs/DECISIONS.md) first and run the
checks from [Development](#development) before opening a PR. Good first areas: importers, CLI reporters, translations
(`app/src/lib/i18n/locales/`), and themes.

## License
Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
Third-party components are listed in [THIRD_PARTY_LICENSES](THIRD_PARTY_LICENSES).

## Author
Created by **Jorge Segovia Tormo** — [@jst7](https://github.com/jst7).
