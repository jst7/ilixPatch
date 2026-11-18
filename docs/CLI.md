# `ilixpatch` CLI

*English · [Castellano](CLI.es.md)*

`ilixpatch` is ilixPatch's command-line tool. It runs the same collections as the desktop app, without a GUI, and is
built for the terminal and CI. Its flags are **Newman-compatible**, so a pipeline that uses `newman run` can be moved
over by changing little more than the command name.

```text
ilixpatch <COMMAND> [OPTIONS]

Commands:
  run           Run a project, collection, folder or request (Newman-compatible flags)
  load          Load test with virtual users
  import        Import from other tools (Postman, OpenAPI / Swagger)
  export        Export to other tools (Postman)
  mock          Serve the saved example responses (mock server)
  ls            List the requests and environments of a collection or project
  check-update  Check GitHub for a newer published release
```

`ilixpatch --help`, `ilixpatch <command> --help` and `ilixpatch --version` show the help and the version.

## Contents
- [Installation](#installation)
- [What can be run](#what-can-be-run)
- [`ilixpatch run`](#ilixpatch-run)
- [`ilixpatch load`](#ilixpatch-load)
- [`ilixpatch import postman`](#ilixpatch-import-postman)
- [`ilixpatch import openapi`](#ilixpatch-import-openapi)
- [`ilixpatch export postman`](#ilixpatch-export-postman)
- [`ilixpatch mock`](#ilixpatch-mock)
- [`ilixpatch ls`](#ilixpatch-ls)
- [`ilixpatch check-update`](#ilixpatch-check-update)
- [Network options (TLS, mTLS, proxy)](#network-options-tls-mtls-proxy)
- [Environments and variables](#environments-and-variables)
- [Scripts (`pm.*`)](#scripts-pm)
- [Authorization](#authorization)
- [Error diagnosis](#error-diagnosis)
- [Exit codes](#exit-codes)
- [Environment variables](#environment-variables)
- [CI example](#ci-example)

## Installation
- **Binaries:** the [Releases](https://github.com/jst7/ilixPatch/releases) page has a CLI binary for each platform.
- **From source:**
  ```sh
  cargo build --release -p ilixpatch-cli      # → target/release/ilixpatch
  cargo run -p ilixpatch-cli -- run examples/demo-project --env dev
  ```
- `./init.sh --cli` runs the CLI against the example project.

## What can be run
The `<path>` argument of `run`, `load` and `ls` can be:

| Path | What runs |
|---|---|
| A **project** (directory with `.ilixpatch/config.toml`) | All its collections |
| A **collection** (directory with `collection.toml`) | All its requests |
| A **folder** inside a collection | The requests of that folder and its subfolders |
| A **request** `.toml` file | Just that request |

Requests run in tree order, as in the app's Runner.

## `ilixpatch run`
Runs the requests with their pre-request and test scripts, and writes reports.

```sh
ilixpatch run examples/demo-project --env dev

ilixpatch run examples/demo-project/jsonplaceholder/users \
  -e dev -d examples/demo-project/data/users.csv \
  -r cli,junit,html \
  --reporter-junit-export results/junit.xml \
  --reporter-html-export results/report.html
```

### Selection
| Flag | Description |
|---|---|
| `--folder <name>` | Run only the requests of that folder (or the request with that name). Repeatable |
| `-f, --filter <text>` | Run only the requests whose path contains that text (in a project the path includes the collection: `collection/folder/request`) |

If no filter matches, the CLI ends with `no requests matched` (exit code 2).

### Environment and variables
| Flag | Description |
|---|---|
| `-e, --env <name\|file>` | Project environment (`.ilixpatch/environments/`); if there is none, a *legacy* collection environment (`<collection>/environments/`); otherwise, a path to a `.toml` or a Postman environment `.json` |
| `--global-env <name>` | Use a **global** environment of the desktop app. Not combinable with `--env` |
| `--env-var key=value` | Set or override an environment variable. Repeatable |
| `--global-var key=value` | Set a global variable (`pm.globals`). Repeatable |
| `--no-globals` | Don't load the app's `globals.toml` as `pm.globals` |
| `--export-environment <path>` | At the end, write the final environment (with the scripts' changes) as Postman JSON |

### Iterations and data
| Flag | Description |
|---|---|
| `-n, --iteration-count <n>` | Number of iterations. Defaults to the number of rows of the data file, or 1 |
| `-d, --iteration-data <file>` | CSV (with a header) or JSON (an array of objects). One iteration per row; each column is available as `{{column}}` and in `pm.iterationData` |

With more iterations than rows, rows are reused as in Newman.

### Run control
| Flag | Description |
|---|---|
| `--delay-request <ms>` | Pause between requests (default 0) |
| `--timeout-request <ms>` | Per-request timeout; `0` = no limit (default) |
| `--bail` | Stop at the first failing request or test |
| `--suppress-exit-code` | Always exit with code 0, even with failures |
| `-v, --verbose` | Show the headers and body of every response |

`Ctrl-C` cancels the run gracefully (the summary and reports are still written); a second `Ctrl-C` aborts at once.

### Reports
| Flag | Description |
|---|---|
| `-r, --reporters cli,json,junit,html` | Reporters to use, comma-separated. Default `cli` |
| `--reporter-json-export <path>` | JSON report file |
| `--reporter-junit-export <path>` | JUnit XML report file (for GitHub Actions, GitLab, Jenkins…) |
| `--reporter-html-export <path>` | HTML report file (includes the response bodies) |

- Giving a `--reporter-X-export` path turns that reporter on even if it isn't in `-r`.
- A reporter turned on without a path writes to `ilixpatch/run-<timestamp>.<ext>`.
- Output directories are created if missing.

The `cli` reporter shows each request live (method, URL, status, size, time), the scripts' `console.log` output and
each test result. At the end it prints a Newman-style summary:

```text
┌────────────────────┬──────────┬────────┐
│                    │ executed │ failed │
├────────────────────┼──────────┼────────┤
│ iterations         │        3 │      0 │
│ requests           │       12 │      1 │
│ test-scripts       │       12 │      0 │
│ prerequest-scripts │        4 │      0 │
│ assertions         │       30 │      2 │
├────────────────────┴──────────┴────────┤
│ total run duration: 1.8s               │
│ total data received: 24.3kB            │
│ average response time: 142ms [...]     │
└────────────────────────────────────────┘
```

followed by a table with each failure numbered (type, request/test and detail).

Colors are off when the output isn't a terminal or `NO_COLOR` is set.

It also takes all the [network options](#network-options-tls-mtls-proxy).

## `ilixpatch load`
Load test: N virtual users (VUs) loop over the target's requests for a while.

```sh
ilixpatch load examples/demo-project/jsonplaceholder --vus 5 --duration 30s \
  --profile ramp-up --ramp-up 10s --think-time 100ms \
  --max-p95 500 --max-error-rate 0.01
```

| Flag | Description |
|---|---|
| `--vus <n>` | Number of virtual users (default 1) |
| `--duration <t>` | Test duration (default `10s`) |
| `--profile <profile>` | `fixed` (default), `ramp-up`, `spike` or `peak` |
| `--ramp-up <t>` | Time to reach all VUs with `ramp-up` / `peak` (default: 25 % of the duration) |
| `--think-time <t>` | Pause of each VU between requests (default `0ms`) |
| `--timeout <t>` | Per-request timeout (default `30s`; `0` = no limit) |
| `--no-scripts` | Don't run pre-request or test scripts (maximum throughput) |
| `--max-p95 <ms>` | CI threshold: fail (exit 1) if the overall p95 latency is above it |
| `--max-error-rate <0..1>` | CI threshold: fail (exit 1) if the error rate is above it |
| `-e, --env`, `--global-env`, `--env-var`, `--global-var`, `--no-globals`, `-k` | As in `run` |

Durations take `500ms`, `30s`, `1.5s`, `2m`, `1h` or a number of seconds.

**Load profiles:**
- `fixed`: all VUs from the start.
- `ramp-up`: grows linearly to `--vus` and stays there.
- `spike`: 20 % of the VUs, and 100 % during the middle 20 % of the test.
- `peak`: ramps up, holds and ramps down.

**What counts as an error:** a network error, a status outside 2xx/3xx or a failing test.

**Output:** one line per second with the time, active VUs, requests, req/s, average latency, p95 and errors; at the
end, a table per request (count, errors, avg, p95) and the overall p50/p90/p95/p99/max latency, throughput and error
rate. With thresholds, each one gets a ✓/✗. If no request completed, the test fails.

`load` also takes the [network options](#network-options-tls-mtls-proxy). Please be gentle with public APIs.

## `ilixpatch import postman`
Imports Postman work into an ilixPatch project.

```sh
ilixpatch import postman my-collection.postman_collection.json dev.postman_environment.json \
  --project ./my-project [--into ./my-project/api] [--env-scope project|global] [--on-conflict rename|overwrite|skip]
```

| Argument / flag | Description |
|---|---|
| `<paths…>` | One or more `.json` files (collections, environments, globals), *Export data* `.zip` files or folders |
| `--project <dir>` | Destination project (required). Initialized if it isn't a project yet |
| `--into <dir>` | Import each collection as a folder inside this collection or folder, instead of as a new collection |
| `--env-scope project\|global` | Where environments go: the project (default) or the app's global environments |
| `--on-conflict rename\|overwrite\|skip` | What to do when a file or folder already exists (default `rename`) |
| `--no-scripts` | Don't import pre-request or test scripts |

Capabilities:
- Collections v2.0 and v2.1, environments, globals and the *Export data* `.zip`.
- Authorization (including inherited) is converted; GraphQL bodies are imported as GraphQL.
- Collection and folder scripts can be inlined into the requests.
- Secret-looking variables go to git-ignored, encrypted files (`*.secret.toml`).

At the end it prints how many collections, folders, requests and environments were imported, the files created and
the conversion warnings. It exits with code 1 if an input file could not be read.

## `ilixpatch import openapi`
Imports **OpenAPI 3.0 / 3.1** and **Swagger 2.0** specs, in JSON or YAML, from files, folders or URLs. `swagger` is
an alias of the subcommand.

```sh
ilixpatch import openapi https://petstore3.swagger.io/api/v3/openapi.json --project ./my-project
ilixpatch import openapi api.yaml --project ./my-project --into ./my-project/api --group-by path
```

| Argument / flag | Description |
|---|---|
| `<FILE\|URL>…` | One or more files (`.json`, `.yaml`, `.yml`), folders containing them or `http(s)://` URLs |
| `--project <dir>` | Destination project (required). Initialized if it isn't a project yet |
| `--into <dir>` | Import each spec as a folder inside this collection or folder |
| `--group-by tags\|path` | Folders: one per tag (default) or one per URL path segment |
| `--on-conflict rename\|overwrite\|skip` | What to do when a file or folder already exists (default `rename`) |
| `-k`, `--proxy`, `--ssl-extra-ca-certs`, … | For downloading URLs, the same [network options](#network-options-tls-mtls-proxy) as `run` |

What you get:
- **One collection per spec**, with a `baseUrl` collection variable (the first server, with its variables filled
  in). A relative server is resolved against the URL the spec was downloaded from; for a file, a warning asks you to
  set `baseUrl`.
- **One request per operation**, named after its `summary` (or `operationId`):
  - Path parameters: their example, or `{{name}}`.
  - Query, headers and cookies: required ones always (with their example, or `{{name}}`); optional ones only if the
    spec gives them a value.
  - Body: the spec's example or, if there is none, one **generated from the schema** (`$ref`, `allOf`, `oneOf`,
    formats…).
  - `Accept` from the 2xx response.
- **Auth** from `security`: Bearer, Basic, Digest, API key and OAuth 2.0 (preferring *client credentials*, which the
  CLI can fetch by itself). Credentials are left as `{{bearerToken}}`, `{{clientSecret}}`… for you to set in an
  environment: they are not written to `collection.toml`, which goes to git.
- **Warnings** for what couldn't be converted: `$ref`s to other files, extra servers, TRACE operations, unsupported
  security schemes, file uploads…

Downloads follow redirects, fail on non-2xx statuses and accept up to 200 MB. GitHub's spec (11 MB of YAML, 1,280
operations) imports in under a second.

## `ilixpatch export postman`
Exports a project or a collection to **Postman**: the inverse of `import postman`.

```sh
ilixpatch export postman ./my-project --out ./postman-export
ilixpatch export postman ./my-project/shop-api --out ./postman-export --include-secrets
```

| Argument / flag | Description |
|---|---|
| `<dir>` | A project (all its collections and environments) or a collection |
| `-o`, `--out <dir>` | Output folder. Created if missing; files with the same name are overwritten |
| `--include-secrets` | Write the values of secret variables. By default they are exported empty (with type `secret`) |

What you get:
- **One `<collection>.postman_collection.json` (v2.1) per collection**, with folders, collection variables,
  collection/folder/request auth (every type), headers, query, bodies (JSON, text, form and GraphQL) and scripts.
- **One `<environment>.postman_environment.json` per project environment**, with disabled variables still disabled.
- **Response variables** (`[[extract]]`) become an equivalent test script (`pm.environment.set`…).
- Request settings go to `protocolProfileBehavior` (redirects and SSL verification). The timeout and HTTP version
  have no Postman equivalent: there is a warning.

What is exported imports back with `ilixpatch import postman` without losing anything.

## `ilixpatch mock`
Serves the saved **example responses** of a project or collection, so a frontend can be developed before the backend
exists (D62).

```sh
ilixpatch mock ./my-project --port 4000
ilixpatch mock ./my-project/shop-api --port 4000 --delay 300
```

| Argument / flag | Description |
|---|---|
| `<dir>` | A project (all its collections) or a collection |
| `-p, --port <n>` | Port (default `4000`) |
| `--host <addr>` | Address to listen on (default `127.0.0.1`; `0.0.0.0` accepts other machines) |
| `--delay <ms>` | Latency added to every response (an example's own `delay_ms` wins) |

- **Examples** are `[[examples]]` in a request file (name, status, headers, body, optional `delay_ms`). They are
  created in the app (Examples tab, or “Save as example” on a real response) and when importing: Postman saved
  responses, and OpenAPI responses (the spec's example, or one generated from the schema).
- **Matching:** by method and path, ignoring the base (`{{base_url}}` or `https://host`). `:id`, `{id}` and
  `{{id}}` segments match any value, and the most specific route wins (`/users/me` before `/users/:id`).
- **Which example:** the first 2xx one; another with the `X-Mock-Example: <name|status>` header or
  `?__example=<name|status>`.
- **For browsers:** CORS is open and preflights are answered. No match is a JSON 404 listing the routes.
- Edits to the collections are served at once (they are re-read at most once per second).
- It prints the routes and one line per request; `Ctrl-C` stops it.

```toml
[[examples]]
name = "OK"
status = 200
body = """{ "id": 1, "name": "Ann" }"""

[[examples]]
name = "Not found"
status = 404
headers = { "Content-Type" = "application/json" }
body = """{ "error": "not found" }"""
```

## `ilixpatch ls`
Lists the contents of a project or collection without running anything:

```sh
$ ilixpatch ls examples/demo-project
project: Demo project
project environments: dev
JSONPlaceholder (format v1)
  GET     posts/List posts
  POST    posts/Create post
  GET     posts/Get post
  GET     users/List users
  GET     users/Get user
  environments: -
Todos (format v1)
  GET     Get todo
  GET     open/List open todos
  environments: -
```

Handy to check which paths to use with `--folder` / `--filter` and which environments exist.

## `ilixpatch check-update`
Asks GitHub for the latest published release and compares it with the installed version:

```sh
$ ilixpatch check-update
ilixpatch 0.13.0 is available (you have 0.12.0): https://github.com/jst7/ilixPatch/releases/tag/v0.13.0
```

- It downloads and installs nothing: it shows the link to the release.
- Only published releases count; drafts and pre-releases don't.
- The CLI **never** checks on its own (no unexpected connections in CI).
- It uses the system proxy. Unauthenticated, GitHub allows 60 queries per hour per IP; the error says so when exceeded.

## Network options (TLS, mTLS, proxy)
Shared by `run` and `load`:

| Flag | Description |
|---|---|
| `-k, --insecure` | Accept invalid or self-signed TLS certificates |
| `--ssl-extra-ca-certs <pem>` | Extra trusted CAs (PEM bundle) |
| `--ssl-client-cert <pem\|p12>` | Client certificate (mTLS) for every host. PEM (with `--ssl-client-key`) or PKCS#12 (`.p12` / `.pfx`) |
| `--ssl-client-key <pem>` | Private key of the PEM certificate. Requires `--ssl-client-cert` |
| `--ssl-client-passphrase <p>` | Password of the key or of the PKCS#12 |
| `--ssl-client-cert-list <json>` | Per-host certificate list, Newman format (see below). Takes precedence over `--ssl-client-cert` |
| `--ignore-redirects` | Don't follow redirects |
| `--proxy <url>` | Send everything through this proxy (`http`, `https`, `socks5`, `socks5h`) |
| `--proxy-bypass <hosts>` | Hosts that skip the proxy (`NO_PROXY` syntax). Requires `--proxy`; repeatable |
| `--no-proxy` | Ignore system and environment-variable proxies |
| `--use-app-settings` | Start from the app's network settings (`network.toml`); the flags above override them |

By default the CLI uses the system proxy / `HTTP(S)_PROXY`, verifies TLS and follows redirects.

`--ssl-client-cert-list` format (relative paths are relative to the list file; `cert`/`key`/`pfx` can be a string
or `{"src": "..."}`):

```json
[
  {
    "name": "api",
    "matches": ["https://api.example.com/*", "https://*.corp.com/*"],
    "cert": "./client.crt",
    "key": "./client.key",
    "passphrase": "secret"
  },
  { "name": "p12", "matches": "https://p12.example.com/*", "pfx": "bundle.p12" }
]
```

## Environments and variables
**Precedence** when resolving `{{variable}}` (highest first):

```text
local (pm.variables) > iteration data > environment > collection > globals
```

- Changes made by scripts (`pm.environment.set`, `pm.globals.set`, …) last for the whole run.
- `--env-var` overrides values of the chosen environment; `--global-var` applies on top of `globals.toml`.
- In a project with several collections, `--env` applies to every collection that defines that environment.
- `*.secret.toml` files (git-ignored) override the matching environment.
- **Encrypted secrets (D60).** Secret values (the `*.secret.toml` files, secret variables of global environments and
  globals, and OAuth 2.0 tokens) are stored encrypted with the app's master password. The CLI takes it from
  `ILIXPATCH_PASSWORD`; without it, it uses the default password (`pass`). If the app's password was changed and none
  is given, the secrets can't be read and the command fails with an error that says so. A wrong `ILIXPATCH_PASSWORD`
  is an immediate error (exit code 2).

**Shared with the desktop app.** Global environments and globals live in the app's config directory:

| OS | Directory |
|---|---|
| macOS | `~/Library/Application Support/dev.ilixpatch.app` |
| Linux | `~/.config/dev.ilixpatch.app` |
| Windows | `%APPDATA%\dev.ilixpatch.app` |

with `environments/<name>.toml` (used with `--global-env`), `globals.toml` and `network.toml` (used with
`--use-app-settings`). In CI, point it elsewhere with `ILIXPATCH_CONFIG_DIR`.

## Scripts (`pm.*`)
`run` and `load` run the **pre-request** and **test** scripts with a Postman/Newman-compatible API, so existing
scripts work unchanged:

- **Tests and assertions:** `pm.test`, `pm.test.skip`, `pm.expect` (Chai), `pm.response.to.have.status(...)`,
  `.header(...)`, `.jsonBody(...)`, etc.
- **Response:** `pm.response.code`, `.status`, `.headers`, `.json()`, `.text()`, `.responseTime`, `.responseSize`.
- **Request:** `pm.request.url`, `.method`, `.headers`, `.body` (editable in pre-request scripts).
- **Variables:** `pm.variables`, `pm.environment`, `pm.collectionVariables`, `pm.globals`, `pm.iterationData`
  (`get`, `set`, `has`, `unset`, `toObject`, `replaceIn`).
- **Info:** `pm.info.requestName`, `.iteration`, `.iterationCount`, `.eventName`; `pm.environment.name`.
- **Flow:** `postman.setNextRequest(name | null)` / `pm.execution.setNextRequest`, and `pm.execution.skipRequest()`.
- **Requests from scripts:** `pm.sendRequest(url | request, (err, res) => …)` or `await pm.sendRequest(…)`, with the
  same network options as the request (proxy, certificates, `-k`…).
- **Modules:** `require()` of `chai`, `crypto-js`, `lodash`, `moment`, `uuid`, `xml2js`, `atob` and `btoa`, and the
  Postman globals `_`, `CryptoJS` and `xml2Json`.
- **Utilities:** `console.log`/`warn`/`error` (shown by the `cli` reporter), `atob`/`btoa`, `crypto.getRandomValues`,
  `setTimeout`.

Current limitations: other modules (cheerio, tv4, ajv…) are not available, and `pm.sendRequest` sends form-data
bodies as `x-www-form-urlencoded`.

## Authorization
`run` and `load` apply each request's authorization automatically, **after** the pre-request script and variable
resolution. Supported types (every Postman one):

`api_key`, `bearer`, `jwt`, `basic`, `digest`, `oauth1`, `oauth2`, `hawk`, `awsv4`, `ntlm`, `akamai`, `asap`,
plus `inherit` and `no_auth`.

- Inherited from the folder (`folder.toml`) and the collection (`collection.toml`); `no_auth` stops inheritance.
- Every field takes `{{variables}}`.
- An explicit auth replaces a manual `Authorization` header (with a warning, as in Postman).
- **Digest** answers the server's `401` challenge (or sends the header directly when `realm` and `nonce` are given).
- **NTLM** does the handshake over a single HTTP/1.1 keep-alive connection, so it doesn't work through proxies that
  close connections.
- **OAuth 2.0:**
  - If the request has an `access_token` (e.g. `access_token = "{{token}}"` with `--env-var token=…`), it is used as is.
  - Otherwise the CLI uses the app's token store (`oauth2-tokens.json` in the app's data directory, or
    `ILIXPATCH_DATA_DIR`): it refreshes expired tokens and fetches `client_credentials` and `password` ones by itself.
  - Interactive flows (authorization code, PKCE, implicit) need a token obtained earlier in the app.
  - Token requests use the same network options as the run (proxy, CAs, client certificate).

```toml
# collection.toml (or a request .toml, or <folder>/folder.toml)
[auth]
type = "bearer"
token = "{{token}}"
```

## Error diagnosis
When a request can't be sent, the `cli` reporter classifies the failure and shows the likely cause and a hint:

| Error | Hint |
|---|---|
| Invalid URL | Use an absolute URL with a scheme |
| Undefined variables | Pick an environment (`-e`), define them in the collection or use `--env-var` |
| DNS | Check the host name, the network/VPN or the environment's base URL |
| Connection refused / reset | Is the server running? http vs https on that port? |
| Timeout | Raise `--timeout-request` |
| Untrusted / expired / mismatched certificate | `--ssl-extra-ca-certs`, or `-k/--insecure` |
| TLS handshake | Is that port really HTTPS? |
| Client certificate rejected | `--ssl-client-cert` / `--ssl-client-key` / `--ssl-client-cert-list` |
| Proxy error | Check `--proxy` or `HTTP(S)_PROXY`, or use `--no-proxy` |
| Too many redirects | Look for a loop, or use `--ignore-redirects` |

## Exit codes
| Code | Meaning |
|---|---|
| `0` | All good (or `--suppress-exit-code`) |
| `1` | `run`: a request errored, a test failed or the run was cancelled. `load`: a threshold was exceeded or no request completed. `import`: a file could not be read |
| `2` | Usage error: invalid flags, missing path, environment not found, no filter matched, wrong `ILIXPATCH_PASSWORD`… |
| `130` | Aborted with a second `Ctrl-C` |

As in Newman, a non-2xx status **without tests** doesn't count as a failure.

## Environment variables
| Variable | Effect |
|---|---|
| `ILIXPATCH_CONFIG_DIR` | Config directory (global environments, `globals.toml`, `network.toml`) instead of the app's |
| `ILIXPATCH_DATA_DIR` | Data directory (OAuth 2.0 token store) instead of the app's |
| `ILIXPATCH_PASSWORD` | Master password to decrypt the secrets (D60); without it the default, `pass`, is used |
| `HTTP_PROXY`, `HTTPS_PROXY`, `NO_PROXY` | System proxy (ignored with `--no-proxy` or replaced by `--proxy`) |
| `NO_COLOR` | Turn colors off |

## CI example
GitHub Actions:

```yaml
- name: API tests
  run: |
    ilixpatch run . -e staging \
      --env-var api_key=${{ secrets.API_KEY }} \
      -r cli,junit --reporter-junit-export results/junit.xml
  env:
    ILIXPATCH_CONFIG_DIR: ${{ runner.temp }}/ilixpatch

- name: Smoke load test
  run: ilixpatch load ./api/health --vus 10 --duration 1m --max-p95 300 --max-error-rate 0.01
```
