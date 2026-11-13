// Typed contract between the Svelte UI and the Rust backend (app/src-tauri/src/lib.rs).
// Keep this file in sync with the Rust structs: field names are serde's JSON output.
// Note: Tauri converts Rust snake_case command args to camelCase on the JS side.

import { invoke, Channel } from "@tauri-apps/api/core";

export type HttpMethod = "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS";
export const HTTP_METHODS: HttpMethod[] = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

export type Body =
  | { type: "json"; content: string }
  | { type: "text"; content: string }
  | { type: "form"; fields: Record<string, string> };

export interface Request {
  name: string;
  method: HttpMethod;
  url: string;
  /** Omitted by the backend when empty. */
  headers?: Record<string, string>;
  /** Omitted by the backend when empty. */
  query?: Record<string, string>;
  /** Omitted by the backend when there is no body. */
  body?: Body | null;
  /** Postman-compatible JavaScript (pm.* API). Omitted when empty. */
  scripts?: Scripts;
  /** Per-request overrides of the global network settings (D29). Omitted when empty. */
  settings?: RequestSettings;
  /** Authorization (D39). Omitted = inherit from folder/collection. */
  auth?: Auth;
  /** Variables taken from the response after each send (D48). Omitted when empty. */
  extract?: Extract[];
}

/** D48: save a value of the response in a variable (`[[extract]]` in the request file). */
export interface Extract {
  var: string;
  from: "json" | "header" | "status" | "body" | "regex";
  /** JSON path, header name or regex, depending on `from` ("" for status/body). */
  path: string;
  scope: "environment" | "collection" | "global";
  enabled: boolean;
}

/** null/undefined = use the global NetworkSettings value. */
export interface RequestSettings {
  verify_ssl?: boolean | null;
  follow_redirects?: boolean | null;
  max_redirects?: number | null;
  timeout_ms?: number | null;
  http_version?: HttpVersionPref | null;
}

export interface Scripts {
  pre_request?: string | null;
  tests?: string | null;
}

/** A request plus the absolute path of the .toml file it lives in. */
export interface RequestFile {
  path: string;
  request: Request;
}

export interface Folder {
  name: string;
  /** Absolute directory path. */
  path: string;
  /** Folder auth from folder.toml (D39); { type: "inherit" } when absent. */
  auth: Auth;
  requests: RequestFile[];
  folders: Folder[];
}

export interface Environment {
  name: string;
  variables: Record<string, string>;
  color?: string | null;
  icon?: string | null;
}

export interface Collection {
  name: string;
  version: number;
  /** Absolute directory path (contains collection.toml). */
  path: string;
  variables: Record<string, string>;
  /** Collection auth from collection.toml (D39); "inherit" here means no auth. */
  auth: Auth;
  environments: Environment[];
  /** Top-level folder of the collection; root.path === collection.path. */
  root: Folder;
}

export interface Project {
  name: string;
  version: number;
  /** Absolute path of the project root (contains .ilixpatch/config.toml). */
  path: string;
  collections: Collection[];
  /** Project environments (D28), secrets already merged in; names sorted. */
  environments: Environment[];
}

/** One registered project. If it failed to load, `project` is null and `error` is set. */
export interface ProjectEntry {
  path: string;
  project: Project | null;
  error: string | null;
}

export interface TestResult {
  name: string;
  passed: boolean;
  error?: string;
  skipped: boolean;
}

export type VarScope = "global" | "collection" | "environment" | "local";
export interface VarUpdate {
  scope: VarScope;
  key: string;
  /** null = unset */
  value: string | null;
}

export interface SendResult {
  status: number;
  status_text: string;
  headers: [string, string][];
  elapsed_ms: number;
  size_bytes: number;
  /** The whole body as text when it is small (≤ NetworkSettings.inline_body_kb), otherwise a
   *  preview of its first bytes (see `body_handle.preview_truncated`). Never the full large body. */
  body: string;
  /** D41: handle to the full body kept in Rust (memory or temp file). Read it in chunks. */
  body_handle: BodyHandle | null;
  /** Results of pm.test(...) in the tests script. */
  tests: TestResult[];
  /** console.* output of both scripts, in order. */
  logs: string[];
  /** Script error (pre-request or tests), if any. The request may still have been sent. */
  script_error: string | null;
  /** Variable writes from scripts (pm.environment.set …). The UI keeps them in a runtime layer. */
  variable_updates: VarUpdate[];
  /** D29 network details of the final response. */
  network: ResponseNetworkInfo | null;
}

/** Variables passed to a single send, by scope (Postman semantics). */
export interface SendVariables {
  globals: Record<string, string>;
  collection: Record<string, string>;
  environment: Record<string, string>;
  environment_name: string | null;
}

// ---------- Collection runner (Newman-compatible) ----------

export interface RunOptions {
  /** Collection or folder directory (or a single request .toml). */
  target: string;
  /** Environment name: a project environment (default) or, with environment_scope "global", a global one.
   *  Falls back to a legacy collection environment of that name. */
  environment?: string | null;
  environment_scope?: EnvScope | null;
  iterations?: number; // default 1 (or number of data rows)
  /** CSV or JSON data file; one iteration per row. */
  data_file?: string | null;
  delay_ms?: number; // between requests
  timeout_ms?: number; // per request, 0 = none
  bail?: boolean; // stop on first failure
  /** Keep variable changes between requests (default true, like Newman). */
  persist_variables?: boolean;
}

export interface RequestRunResult {
  iteration: number;
  /** Request file path. */
  path: string;
  name: string;
  method: HttpMethod;
  url: string;
  status: number | null;
  status_text: string | null;
  elapsed_ms: number;
  size_bytes: number;
  tests: TestResult[];
  logs: string[];
  /** Network / variable / script error. */
  error: string | null;
  /** D36: category of `error` when it's a send failure (null for script/test failures). */
  error_kind: RequestErrorKind | null;
}

export interface RunSummary {
  iterations: number;
  requests: number;
  failed_requests: number;
  tests: number;
  failed_tests: number;
  skipped_tests: number;
  total_ms: number;
  avg_response_ms: number;
  cancelled: boolean;
}

export type RunEvent =
  | { type: "started"; total_requests: number; iterations: number }
  | { type: "iteration"; iteration: number }
  | { type: "request"; result: RequestRunResult }
  | { type: "finished"; summary: RunSummary };

// ---------- Load testing ----------

export type LoadProfile = "fixed" | "ramp_up" | "spike" | "peak";

export interface LoadOptions {
  target: string;
  environment?: string | null;
  environment_scope?: EnvScope | null;
  virtual_users: number;
  duration_secs: number;
  /** For ramp_up/peak: seconds to reach virtual_users. */
  ramp_up_secs?: number;
  profile: LoadProfile;
  /** Pause per virtual user between requests. */
  think_time_ms?: number;
  /** Run pre-request/test scripts (slower). Default true. */
  run_scripts?: boolean;
}

export interface LoadRequestStats {
  path: string;
  name: string;
  method: HttpMethod;
  count: number;
  errors: number;
  avg_ms: number;
  p95_ms: number;
}

/** Emitted ~every second, and once more at the end with final=true. */
export interface LoadSnapshot {
  elapsed_secs: number;
  active_vus: number;
  requests: number;
  errors: number;
  /** Requests/s over the last interval. */
  rps: number;
  avg_ms: number;
  p50_ms: number;
  p90_ms: number;
  p95_ms: number;
  p99_ms: number;
  max_ms: number;
  error_rate: number; // 0..1
  per_request: LoadRequestStats[];
  final: boolean;
}

// ---------- Git (D16): any project/collection/folder dir can be its own repo ----------

export interface GitRepoInfo {
  /** Absolute repo root (the dir containing .git). */
  root: string;
  /** Current branch; null when HEAD is detached. */
  branch: string | null;
  /** Short HEAD commit id; null in an empty repo. */
  head: string | null;
  upstream: string | null;
  ahead: number;
  behind: number;
  /** Any staged/unstaged/untracked change. */
  dirty: boolean;
  /** Merge/rebase in progress or conflicted files. */
  conflicted: boolean;
}

export type GitFileState = "modified" | "added" | "deleted" | "renamed" | "untracked" | "conflicted";

export interface GitFileStatus {
  /** Absolute path. */
  path: string;
  state: GitFileState;
  staged: boolean;
  /** For renames. */
  old_path?: string | null;
}

export interface GitBranch {
  name: string;
  current: boolean;
  /** true for remote-tracking branches like origin/main. */
  remote: boolean;
  upstream: string | null;
}

export interface GitCommit {
  id: string;
  short_id: string;
  summary: string;
  author: string;
  /** Unix seconds. */
  time: number;
}

/** Old (HEAD) and new (working tree) contents of one file, for diff views. */
export interface GitFileDiff {
  path: string;
  old: string | null;
  new: string | null;
}

// ---------- Request history (D22): local SQLite, never in the repo ----------

export interface HistorySettings {
  enabled: boolean;
  /** Delete entries older than N days; null = keep forever. */
  max_days: number | null;
  /** Max total storage in MB; oldest entries are pruned first. null = unlimited. */
  max_size_mb: number | null;
  save_response_body: boolean;
  /** Bodies larger than this are truncated (body_truncated = true). */
  max_body_kb: number;
}

/** Where a history entry came from (optional; ad-hoc requests have none). */
export interface HistoryContext {
  project_path: string | null;
  request_path: string | null;
  environment: string | null;
}

export interface HistoryEntrySummary {
  id: number;
  /** Unix milliseconds. */
  timestamp: number;
  name: string;
  method: HttpMethod;
  /** Resolved URL that was actually sent. */
  url: string;
  status: number | null;
  elapsed_ms: number;
  size_bytes: number;
  /** Network error, if the request failed. */
  error: string | null;
  project_path: string | null;
  request_path: string | null;
  environment: string | null;
}

export interface HistoryResponse {
  status: number;
  status_text: string;
  headers: [string, string][];
  /** Empty when save_response_body is false. */
  body: string;
  body_truncated: boolean;
}

export interface HistoryEntry extends HistoryEntrySummary {
  /** The request as edited (unresolved {{vars}}) — this is what gets restored into a new tab. */
  request: Request;
  /** The request after variable resolution and pre-request script. */
  resolved_request: Request | null;
  response: HistoryResponse | null;
  tests: TestResult[];
  logs: string[];
}

export interface HistoryStats {
  entries: number;
  size_bytes: number;
  /** Unix ms of the oldest entry. */
  oldest: number | null;
}

// ---------- Git merge & conflict resolution (D24) ----------

export type GitOperation = "merge" | "rebase" | "cherry_pick" | "revert";

export interface GitMergeState {
  /** Operation in progress, or null. */
  operation: GitOperation | null;
  /** Absolute paths of files with unresolved conflicts. */
  conflicted: string[];
  /** Prepared commit message (MERGE_MSG), if any. */
  message: string | null;
  /** e.g. "main" / "feature-x" or short commit ids, for pane titles. */
  ours_label: string;
  theirs_label: string;
}

/** The three stages of a conflicted file, plus the working-tree version (with conflict markers). */
export interface GitConflictVersions {
  path: string;
  /** Common ancestor (stage 1); null if the file didn't exist there. */
  base: string | null;
  /** Current branch / HEAD (stage 2); null if deleted on our side. */
  ours: string | null;
  /** Incoming (stage 3); null if deleted on their side. */
  theirs: string | null;
  /** Working tree content (usually with <<<<<<< markers). */
  merged: string | null;
  /** True when a side is binary: the UI only offers take-ours / take-theirs. */
  binary: boolean;
}

export type GitPullMode = "ff_only" | "merge" | "rebase";

// ---------- Git annotate / blame (D25) ----------

/** Who last changed a line. `commit` is null for uncommitted (working tree) changes. */
export interface BlameInfo {
  commit: string | null;
  short_id: string | null;
  author: string | null;
  author_email: string | null;
  /** Unix seconds; null when uncommitted. */
  time: number | null;
  summary: string | null;
}

/** Raw blame of any file: one entry per line (1-based line = index + 1). */
export interface FileBlame {
  path: string;
  lines: BlameInfo[];
}

/** Blame mapped onto the fields of a request .toml, so the UI can annotate its editors. */
export interface RequestBlame {
  path: string;
  name: BlameInfo | null;
  method: BlameInfo | null;
  url: BlameInfo | null;
  headers: Record<string, BlameInfo>;
  query: Record<string, BlameInfo>;
  /** One per line of the body content as shown in the body editor (json/text); form bodies use `form`. */
  body_lines: BlameInfo[];
  form: Record<string, BlameInfo>;
  pre_request_lines: BlameInfo[];
  tests_lines: BlameInfo[];
}

// ---------- Git clone (D26) ----------

export interface GitCloneOptions {
  url: string;
  /** Existing directory chosen by the user; the repo is cloned into parent_dir/dir_name. */
  parent_dir: string;
  /** Defaults to the repo name derived from the URL. Must not exist (or be an empty dir). */
  dir_name?: string | null;
  branch?: string | null;
  /** Shallow clone depth; null = full history. */
  depth?: number | null;
  /** HTTPS credentials for private repos without a credential helper. Never written to disk or argv. */
  username?: string | null;
  token?: string | null;
  /** Register the clone as a project (creating .ilixpatch/config.toml if missing). */
  add_as_project: boolean;
}

export type GitCloneEvent =
  /** phase e.g. "Receiving objects", "Resolving deltas"; percent null when unknown. */
  | { type: "progress"; phase: string; percent: number | null; message: string }
  | { type: "done"; path: string };

export interface GitCloneResult {
  /** Absolute path of the cloned repo. */
  path: string;
  /** Present when add_as_project was true. */
  project: Project | null;
}

// ---------- Git commit details & history actions (D27) ----------

export interface GitCommitFile {
  /** Absolute path (as it is in that commit). */
  path: string;
  /** For renames/copies. */
  old_path: string | null;
  state: "added" | "modified" | "deleted" | "renamed";
  /** null for binary files. */
  additions: number | null;
  deletions: number | null;
  binary: boolean;
}

export interface GitCommitDetails extends GitCommit {
  /** Full message (summary + body). */
  message: string;
  author_email: string;
  committer: string;
  committer_email: string;
  /** Unix seconds. */
  committer_time: number;
  /** Full ids; empty for the root commit, 2+ for merges. */
  parents: string[];
  /** Changes against the FIRST parent (or the empty tree for the root commit). */
  files: GitCommitFile[];
  /** Branch/tag names pointing at this commit. */
  refs: string[];
}

export type GitResetMode = "soft" | "mixed" | "hard";

// ---------- Environments: project & global (D28) ----------

export interface EnvVariable {
  key: string;
  value: string;
  /** Secret values are never written to the repo (project: <name>.secret.toml, gitignored). */
  secret: boolean;
  /** Disabled variables are kept but not used. */
  enabled: boolean;
}

/** Editable environment document (project or global). */
export interface EnvironmentDoc {
  name: string;
  variables: EnvVariable[];
  /** D49: theme colour token ("ok", "warn"…) or "#rrggbb". */
  color?: string | null;
  /** D49: icon id (lib/env/style.ts). */
  icon?: string | null;
}

export type EnvScope = "project" | "global";

export type EnvRef = { scope: "project"; project_path: string; name: string } | { scope: "global"; name: string };

/** What a request is sent with: an environment of the project or a global one. */
export interface EnvSelection {
  scope: EnvScope;
  name: string;
}

// ---------- Network, HTTPS & proxy (D29) ----------
// Requests are sent from Rust (reqwest), never from the webview, so browser CORS does not apply.

export type HttpVersionPref = "auto" | "http1" | "http2";

export interface ClientCertificate {
  /** Host pattern: "api.example.com", "*.example.com", optionally with ":port". */
  host: string;
  /** PEM certificate + PEM key, or a PFX/P12 bundle. Absolute paths. */
  cert_path: string | null;
  key_path: string | null;
  pfx_path: string | null;
  /** Stored only in the user's settings file (0600), never in a repo. */
  passphrase: string | null;
  enabled: boolean;
}

export interface ProxySettings {
  mode: "system" | "none" | "custom";
  /** e.g. http://proxy:8080 or socks5://127.0.0.1:1080 */
  url: string | null;
  username: string | null;
  password: string | null;
  /** Hosts that bypass the proxy (NO_PROXY syntax: "localhost,.internal,10.0.0.0/8"). */
  bypass: string[];
}

export interface NetworkSettings {
  verify_ssl: boolean;
  /** Trust the operating system's root certificates (in addition to extra CAs). */
  use_system_ca: boolean;
  /** Extra trusted CA certificates (PEM files). */
  ca_certificates: string[];
  client_certificates: ClientCertificate[];
  proxy: ProxySettings;
  follow_redirects: boolean;
  max_redirects: number;
  /** 0 = no timeout. */
  timeout_ms: number;
  min_tls_version: "1.2" | "1.3" | null;
  http_version: HttpVersionPref;
  /** D41: bodies up to this size stay in memory and are sent whole to the UI (default 1024 KB). */
  inline_body_kb: number;
  /** D41: stop downloading after this many MB (body marked incomplete); 0 = unlimited (default 1024). */
  max_body_mb: number;
}

export interface CertificateInfo {
  subject: string;
  issuer: string;
  /** ISO 8601. */
  not_before: string;
  not_after: string;
  /** SHA-256 fingerprint, hex with colons. */
  sha256: string;
  /** Subject alternative names (DNS/IP). */
  san: string[];
}

export interface ResponseNetworkInfo {
  /** "HTTP/1.1", "HTTP/2" … */
  http_version: string;
  remote_addr: string | null;
  /** Each redirect followed, in order (status + Location). */
  redirects: { status: number; url: string }[];
  /** Final URL after redirects. */
  final_url: string;
  /** Present for HTTPS responses. */
  tls: { peer_certificate: CertificateInfo | null; verified: boolean } | null;
  /** Whether a proxy was used, and which (credentials stripped). */
  proxy: string | null;
}

// ---------- Importer: Postman (D31) and OpenAPI / Swagger (D53) ----------

export interface ImportWarning {
  /** Id of the source node/environment it refers to (null = general). */
  source_id: string | null;
  /** Stable code, e.g. "auth_oauth2", "body_file", "disabled_header", "parent_script", "path_variable". */
  code: string;
  /** Human-readable English detail (the UI may translate by `code`). */
  message: string;
}

export interface ImportNode {
  /** Unique, stable within one preview. */
  id: string;
  kind: "collection" | "folder" | "request";
  name: string;
  method?: HttpMethod;
  url?: string;
  /** Has pre-request/test scripts (own or inherited). */
  has_scripts: boolean;
  /** Auth type in Postman ("bearer", "basic", "apikey", "oauth2", "noauth", "inherit"…), if any. */
  auth: string | null;
  /** Collection-level variables (collections only). */
  variables?: EnvVariable[];
  children: ImportNode[];
}

export interface ImportEnvironment {
  id: string;
  name: string;
  variables: EnvVariable[];
}

/** How OpenAPI operations are grouped into folders: by (first) tag, or by URL path segment. */
export type GroupBy = "tags" | "path";

/** Options that change how sources are read (changing them needs a new preview). */
export interface ParseOptions {
  group_by: GroupBy;
}

export interface ImportPreview {
  /** Opaque id of this parsed set of sources, used by importApply. */
  session: string;
  collections: ImportNode[];
  environments: ImportEnvironment[];
  /** Globals found in a data dump / globals file. */
  globals: EnvVariable[] | null;
  warnings: ImportWarning[];
  /** Files (or URLs) that could not be read, with the reason. */
  errors: { file: string; message: string }[];
  /** At least one OpenAPI / Swagger spec was read (the wizard offers GroupBy). */
  openapi: boolean;
}

/** Put one source node (and its checked descendants) somewhere in the destination project. */
export interface ImportPlacement {
  source_id: string;
  /** Absolute destination dir: the project root (→ new collection) or a collection/folder dir. */
  target_parent: string;
  /** "collection" only when target_parent is the project root (or its collections_dir). */
  as: "collection" | "folder" | "request";
  /** Rename on import (null = keep the source name). */
  name: string | null;
}

export interface ImportPlan {
  session: string;
  /** Registered destination project. */
  project_path: string;
  placements: ImportPlacement[];
  /** Source ids unchecked by the user (their descendants are skipped too). */
  excluded: string[];
  environments: { source_id: string; scope: EnvScope; name: string | null }[];
  globals: "skip" | "merge";
  options: {
    on_conflict: "rename" | "overwrite" | "skip";
    /** Convert Postman auth (bearer/basic/apikey, incl. inherited) into headers or a pre-request script. */
    convert_auth: boolean;
    keep_scripts: boolean;
    /** Prepend inherited collection/folder scripts to each request's scripts. */
    inline_parent_scripts: boolean;
    /** Mark variables whose key matches this regex as secret (case-insensitive); null = none. */
    secret_pattern: string | null;
  };
}

export interface ImportReport {
  collections: number;
  folders: number;
  requests: number;
  environments: number;
  /** Absolute paths of files written. */
  files: string[];
  skipped: number;
  warnings: ImportWarning[];
}

// ---------- Update check (D54) ----------

/** The latest published release on GitHub (drafts and pre-releases never count). */
export interface LatestRelease {
  /** Without the leading "v", e.g. "0.4.2". */
  version: string;
  /** Release page with the installers, CLI binaries and notes. */
  url: string;
  published_at: string | null;
}

export interface UpdateCheck {
  current: string;
  latest: LatestRelease;
  /** latest is a higher version than current. */
  newer: boolean;
}

// ---------- Descriptive request errors (D36) ----------

/** Stable category of a failed send; the UI shows a localized title, likely cause and suggestions per kind. */
export type RequestErrorKind =
  | "invalid_url"
  | "undefined_variables"
  | "dns"
  | "connection_refused"
  | "connection_reset"
  | "timeout"
  | "tls_unknown_issuer"
  | "tls_expired"
  | "tls_hostname_mismatch"
  | "tls_handshake"
  | "tls_client_certificate"
  | "proxy"
  | "too_many_redirects"
  | "body"
  | "skipped_by_script"
  | "cancelled"
  | "other";

/** What `sendRequest` rejects with (instead of a plain string). */
export interface RequestError {
  kind: RequestErrorKind;
  /** Short English summary from the backend (fallback when the UI has no translation). */
  message: string;
  /** Full technical chain (error sources joined), for the "Details" disclosure. */
  detail: string;
  /** Facts the UI can interpolate into its hints. */
  host?: string | null;
  port?: number | null;
  url?: string | null;
  timeout_ms?: number | null;
  /** For undefined_variables. */
  variables?: string[] | null;
  /** Whether a proxy was in use (helps tell proxy vs. target failures apart). */
  proxy?: string | null;
}

// ---------- Authorization (D39) — mirrors crates/core/src/auth.rs ----------
// Every string field may contain {{variables}}.

export type AddTo = "header" | "query" | "body";

export type Auth =
  | { type: "inherit" }
  | { type: "no_auth" }
  | { type: "api_key"; key: string; value: string; in?: AddTo }
  | { type: "bearer"; token: string }
  | {
      type: "jwt";
      /** HS256/384/512, RS256/384/512, PS256/384/512, ES256/384/512 */
      algorithm: string;
      secret?: string | null;
      secret_base64?: boolean;
      private_key?: string | null;
      /** JSON object */
      payload: string;
      /** JSON object with extra header fields */
      header?: string | null;
      add_to?: AddTo;
      header_prefix?: string | null;
      query_param?: string | null;
    }
  | { type: "basic"; username: string; password?: string }
  | {
      type: "digest";
      username: string;
      password?: string;
      realm?: string | null;
      nonce?: string | null;
      /** MD5, MD5-sess, SHA-256, SHA-256-sess, SHA-512-256, SHA-512-256-sess */
      algorithm?: string | null;
      qop?: string | null;
      nonce_count?: string | null;
      client_nonce?: string | null;
      opaque?: string | null;
      disable_retry?: boolean;
    }
  | {
      type: "oauth1";
      /** HMAC-SHA1, HMAC-SHA256, HMAC-SHA512, RSA-SHA1, RSA-SHA256, RSA-SHA512, PLAINTEXT */
      signature_method: string;
      consumer_key: string;
      consumer_secret?: string;
      token?: string | null;
      token_secret?: string | null;
      private_key?: string | null;
      callback_url?: string | null;
      verifier?: string | null;
      timestamp?: string | null;
      nonce?: string | null;
      version?: string | null;
      realm?: string | null;
      add_to?: AddTo;
      include_body_hash?: boolean;
      add_empty_params?: boolean;
    }
  | ({ type: "oauth2" } & OAuth2Config)
  | {
      type: "hawk";
      id: string;
      key: string;
      algorithm?: "sha256" | "sha1" | null;
      user?: string | null;
      nonce?: string | null;
      ext?: string | null;
      app?: string | null;
      dlg?: string | null;
      timestamp?: string | null;
      include_payload_hash?: boolean;
    }
  | {
      type: "awsv4";
      access_key: string;
      secret_key: string;
      session_token?: string | null;
      region: string;
      service: string;
      add_to?: AddTo;
    }
  | { type: "ntlm"; username: string; password?: string; domain?: string | null; workstation?: string | null; disable_retry?: boolean }
  | {
      type: "akamai";
      client_token: string;
      client_secret: string;
      access_token: string;
      base_url?: string | null;
      nonce?: string | null;
      timestamp?: string | null;
      headers_to_sign?: string | null;
      max_body_size?: number | null;
    }
  | {
      type: "asap";
      algorithm: string;
      issuer: string;
      subject?: string | null;
      audience: string;
      key_id: string;
      private_key: string;
      additional_claims?: string | null;
      expires_in?: number | null;
    };

export type AuthType = Auth["type"];

export type OAuth2Grant = "authorization_code" | "authorization_code_pkce" | "implicit" | "password" | "client_credentials";

export interface OAuth2Config {
  grant_type: OAuth2Grant;
  /** Tokens are stored per user under this name (never in the repo). */
  token_name: string;
  auth_url?: string | null;
  access_token_url?: string | null;
  client_id: string;
  client_secret?: string | null;
  scope?: string | null;
  state?: string | null;
  callback_url?: string | null;
  /** Listen on a loopback port for the redirect (default true). */
  use_local_callback?: boolean;
  code_challenge_method?: "S256" | "plain" | null;
  code_verifier?: string | null;
  username?: string | null;
  password?: string | null;
  client_authentication?: "basic" | "body";
  audience?: string | null;
  resource?: string | null;
  extra_params?: Record<string, string>;
  add_to?: AddTo;
  header_prefix?: string | null;
  auto_refresh?: boolean;
  auto_fetch?: boolean;
  /** Literal token to use instead of the token store. */
  access_token?: string | null;
}

/** A stored OAuth 2.0 token (user-level token store, never in a repo). */
export interface OAuth2StoredToken {
  id: string;
  token_name: string;
  access_token: string;
  token_type: string | null;
  refresh_token: string | null;
  id_token: string | null;
  scope: string | null;
  /** Unix seconds. */
  expires_at: number | null;
  /** Unix seconds. */
  created_at: number;
  /** Full token response JSON. */
  raw: unknown;
}

// ---------- Large responses: streaming & chunked reads (D41) ----------

export interface BodyHandle {
  /** Opaque id of the body in the Rust ResponseStore. */
  id: string;
  /** Bytes received (after decompression). */
  size: number;
  /** "memory" for small bodies, "file" when spilled to a temp file. */
  stored: "memory" | "file";
  /** false when the download stopped early (max_body_mb reached or cancelled). */
  complete: boolean;
  content_type: string | null;
  /** Looks like text (UTF-8 or declared text/json/xml charset). */
  is_text: boolean;
  /** Looks like JSON (content type or sniffed). */
  is_json: boolean;
  /** `SendResult.body` holds only the first bytes. */
  preview_truncated: boolean;
}

export interface BodyChunk {
  /** Text (UTF-8, lossy) when the body is text; base64 otherwise. */
  data: string;
  encoding: "utf8" | "base64";
  offset: number;
  /** Bytes covered (may be < requested at EOF; adjusted to UTF-8 boundaries for text). */
  length: number;
  eof: boolean;
}

export interface BodyLines {
  start: number;
  /** Line texts, each cut to at most `max_line_bytes` bytes (on a UTF-8 boundary). */
  lines: string[];
  /** Byte offset where each line starts in the selected (raw or pretty) view. */
  offsets: number[];
  /** Full byte length of each line (without the line break). `> bytes of lines[i]` ⇒ the line was cut. */
  byte_lengths: number[];
  /** Total lines of the (raw or pretty) view. */
  total_lines: number;
}

export interface BodySearchMatch {
  /** 0-based line in the selected view. */
  line: number;
  /** Character column and length within that line. */
  column: number;
  length: number;
}

export interface BodySearchResult {
  matches: BodySearchMatch[];
  /** Stopped at max_results. */
  truncated: boolean;
}

// ---------- Network console (D46) ----------

export interface NetBodyPreview {
  /** UTF-8 text (empty for binary bodies). */
  text: string;
  size: number;
  truncated: boolean;
  binary: boolean;
}

/** Phases of one exchange, in ms (D46 phase 2). TCP and TLS are measured together. */
export interface NetTimings {
  dns_ms: number | null;
  /** New connection: TCP + TLS (+ proxy CONNECT); null when a pooled connection was reused. */
  connect_ms: number | null;
  reused: boolean;
  /** Request sent → response headers (time to first byte). */
  wait_ms: number;
  /** Reading the body (final response only). */
  download_ms: number | null;
}

/** One HTTP exchange as sent on the wire (a request, a redirect hop, an auth round, an OAuth 2.0 token request). */
export interface NetEntry {
  id: number;
  /** Exchanges of the same send share it. */
  trace: number | null;
  /** Request name, "Runner · name", "OAuth 2.0 · token"… */
  label: string;
  /** "redirect" | "digest-response" | "ntlm-negotiate" | "ntlm-authenticate" | "oauth2-token" | null */
  phase: string | null;
  timestamp_ms: number;
  method: string;
  url: string;
  /** As sent, including the headers the HTTP client adds (Host, User-Agent, Accept…). */
  request_headers: [string, string][];
  request_body: NetBodyPreview | null;
  status: number | null;
  status_text: string | null;
  response_headers: [string, string][];
  response_body: NetBodyPreview | null;
  http_version: string | null;
  remote_addr: string | null;
  proxy: string | null;
  elapsed_ms: number;
  timings: NetTimings | null;
  error: string | null;
}

export const api = {
  /** All projects registered in the app config, in insertion order. */
  listProjects: () => invoke<ProjectEntry[]>("list_projects"),
  /** Register a folder as a project. Creates .ilixpatch/config.toml (name = folder name) if missing. */
  addProject: (path: string) => invoke<Project>("add_project", { path }),
  /** Unregister (does NOT delete files). */
  removeProject: (path: string) => invoke<void>("remove_project", { path }),
  reloadProject: (path: string) => invoke<Project>("reload_project", { path }),
  /** Returns the new collection directory path. */
  createCollection: (projectPath: string, name: string) =>
    invoke<string>("create_collection", { projectPath, name }),
  /** parentPath is a collection or folder directory. Returns the new folder path. */
  createFolder: (parentPath: string, name: string) => invoke<string>("create_folder", { parentPath, name }),
  /** folderPath is a collection or folder directory. Creates a GET request file. */
  createRequest: (folderPath: string, name: string) =>
    invoke<RequestFile>("create_request", { folderPath, name }),
  saveRequest: (path: string, request: Request) => invoke<void>("save_request", { path, request }),
  /** Deletes a request file, folder or collection. Must be inside a registered project; never the project root. */
  deleteItem: (path: string) => invoke<void>("delete_item", { path }),
  /** Runs pre-request script, sends, runs tests. The backend interpolates {{vars}}. */
  /** `history` context is recorded with the history entry (backend records every send when history is enabled). */
  /** `requestId` lets the UI cancel it with cancelRequest. Rejects with a `RequestError`
   *  (kind "cancelled" when the user cancelled). */
  sendRequest: (
    request: Request,
    variables: SendVariables,
    history: HistoryContext | null = null,
    requestId: string | null = null,
  ) => invoke<SendResult>("send_request", { request, variables, history, requestId }),
  /** Abort a send in progress (closes the connection immediately). No-op if it already finished. */
  cancelRequest: (requestId: string) => invoke<void>("cancel_request", { requestId }),

  /** Run a collection/folder. Events stream through onEvent; resolves with the summary. One run at a time. */
  runCollection: (options: RunOptions, onEvent: (e: RunEvent) => void) => {
    const channel = new Channel<RunEvent>();
    channel.onmessage = onEvent;
    return invoke<RunSummary>("run_collection", { options, onEvent: channel });
  },
  cancelRun: () => invoke<void>("cancel_run"),

  /** Start a load test; snapshots stream ~1/s. Resolves with the final snapshot. One at a time. */
  startLoadTest: (options: LoadOptions, onSnapshot: (s: LoadSnapshot) => void) => {
    const channel = new Channel<LoadSnapshot>();
    channel.onmessage = onSnapshot;
    return invoke<LoadSnapshot>("start_load_test", { options, onSnapshot: channel });
  },
  stopLoadTest: () => invoke<void>("stop_load_test"),

  // Git — every call is scoped to a repo root returned by gitRepos (must be inside a registered project).
  /** All repos relevant to a project: the one containing the project root (if any) plus every nested repo root
   *  inside it (collections/folders with their own .git), sorted by path. Cheap enough to call on every reload. */
  gitRepos: (projectPath: string) => invoke<GitRepoInfo[]>("git_repos", { projectPath }),
  gitStatus: (repoRoot: string) => invoke<GitFileStatus[]>("git_status", { repoRoot }),
  gitBranches: (repoRoot: string) => invoke<GitBranch[]>("git_branches", { repoRoot }),
  /** Switch branch; create=true creates it from HEAD first. Refuses when it would overwrite local changes. */
  gitCheckout: (repoRoot: string, branch: string, create: boolean) =>
    invoke<GitRepoInfo>("git_checkout", { repoRoot, branch, create }),
  /** Make a directory (project, collection or folder) its own repo. */
  gitInit: (dir: string) => invoke<GitRepoInfo>("git_init", { dir }),
  gitDiffFile: (path: string) => invoke<GitFileDiff>("git_diff_file", { path }),
  /** Stage exactly `paths` (absolute) and commit. */
  gitCommit: (repoRoot: string, paths: string[], message: string) =>
    invoke<GitCommit>("git_commit", { repoRoot, paths, message }),
  /** Discard working-tree changes of these files (restore from HEAD; delete if untracked). */
  gitDiscard: (repoRoot: string, paths: string[]) => invoke<void>("git_discard", { repoRoot, paths }),
  gitFetch: (repoRoot: string) => invoke<GitRepoInfo>("git_fetch", { repoRoot }),
  gitPull: (repoRoot: string) => invoke<GitRepoInfo>("git_pull", { repoRoot }),
  gitPush: (repoRoot: string) => invoke<GitRepoInfo>("git_push", { repoRoot }),
  /** History of the repo, or of one file when `path` is given. */
  gitLog: (repoRoot: string, path: string | null, limit: number) =>
    invoke<GitCommit[]>("git_log", { repoRoot, path, limit }),

  // History
  /** Newest first. `query` filters by URL/name/method substring; paginate with `beforeId`. */
  historyList: (query: string | null, beforeId: number | null, limit: number) =>
    invoke<HistoryEntrySummary[]>("history_list", { query, beforeId, limit }),
  historyGet: (id: number) => invoke<HistoryEntry>("history_get", { id }),
  historyDelete: (ids: number[]) => invoke<void>("history_delete", { ids }),
  historyClear: () => invoke<void>("history_clear"),
  historyStats: () => invoke<HistoryStats>("history_stats"),
  historyGetSettings: () => invoke<HistorySettings>("history_get_settings"),
  /** Saves and prunes immediately according to the new limits. */
  historySetSettings: (settings: HistorySettings) => invoke<HistoryStats>("history_set_settings", { settings }),

  // Git merge & conflicts (D24)
  gitPullWith: (repoRoot: string, mode: GitPullMode) => invoke<GitRepoInfo>("git_pull_with", { repoRoot, mode }),
  /** Merge `branch` into the current branch. Conflicts are NOT an error: check gitMergeState afterwards. */
  gitMerge: (repoRoot: string, branch: string) => invoke<GitMergeState>("git_merge", { repoRoot, branch }),
  gitMergeState: (repoRoot: string) => invoke<GitMergeState>("git_merge_state", { repoRoot }),
  gitConflictVersions: (repoRoot: string, path: string) =>
    invoke<GitConflictVersions>("git_conflict_versions", { repoRoot, path }),
  /** Write the resolved content and mark the file resolved (git add). */
  gitResolveFile: (repoRoot: string, path: string, content: string) =>
    invoke<GitMergeState>("git_resolve_file", { repoRoot, path, content }),
  /** Resolve by taking one whole side (also handles deleted/binary files). */
  gitResolveWith: (repoRoot: string, path: string, side: "ours" | "theirs") =>
    invoke<GitMergeState>("git_resolve_with", { repoRoot, path, side }),
  /** Abort the operation in progress (merge --abort / rebase --abort / …). */
  gitAbortOperation: (repoRoot: string) => invoke<GitRepoInfo>("git_abort_operation", { repoRoot }),
  /** Finish the operation once nothing is conflicted (commit the merge / rebase --continue …). */
  gitContinueOperation: (repoRoot: string, message: string | null) =>
    invoke<GitMergeState>("git_continue_operation", { repoRoot, message }),

  // Git annotate (D25). Path must be inside a repo of a registered project.
  gitBlameFile: (path: string) => invoke<FileBlame>("git_blame_file", { path }),
  gitBlameRequest: (path: string) => invoke<RequestBlame>("git_blame_request", { path }),

  // Git clone (D26). One clone at a time.
  gitClone: (options: GitCloneOptions, onEvent: (e: GitCloneEvent) => void) => {
    const channel = new Channel<GitCloneEvent>();
    channel.onmessage = onEvent;
    return invoke<GitCloneResult>("git_clone", { options, onEvent: channel });
  },
  /** Cancels the running clone and removes the partially cloned directory. */
  gitCancelClone: () => invoke<void>("git_cancel_clone"),

  // Git commit details & history actions (D27)
  /** `skip` for paging the history (git log --skip). */
  gitLogPage: (repoRoot: string, path: string | null, skip: number, limit: number) =>
    invoke<GitCommit[]>("git_log_page", { repoRoot, path, skip, limit }),
  gitCommitDetails: (repoRoot: string, commit: string) =>
    invoke<GitCommitDetails>("git_commit_details", { repoRoot, commit }),
  /** old = version in the first parent (null if added), new = version in the commit (null if deleted). */
  gitCommitFileDiff: (repoRoot: string, commit: string, path: string) =>
    invoke<GitFileDiff>("git_commit_file_diff", { repoRoot, commit, path }),
  /** New commit undoing `commit` (noCommit = only stage the changes). Conflicts → state, not an error. */
  gitRevertCommit: (repoRoot: string, commit: string, noCommit: boolean) =>
    invoke<GitMergeState>("git_revert_commit", { repoRoot, commit, noCommit }),
  /** Move the current branch to `commit`. "hard" discards working-tree changes. */
  gitResetTo: (repoRoot: string, commit: string, mode: GitResetMode) =>
    invoke<GitRepoInfo>("git_reset_to", { repoRoot, commit, mode }),
  /** Overwrite one file in the working tree with its version at `commit` (staged). */
  gitRestoreFile: (repoRoot: string, path: string, commit: string) =>
    invoke<void>("git_restore_file", { repoRoot, path, commit }),
  gitCherryPick: (repoRoot: string, commit: string) =>
    invoke<GitMergeState>("git_cherry_pick", { repoRoot, commit }),
  /** Create a branch at `commit`; checkout=true switches to it. */
  gitCreateBranchAt: (repoRoot: string, name: string, commit: string, checkout: boolean) =>
    invoke<GitRepoInfo>("git_create_branch_at", { repoRoot, name, commit, checkout }),
  /** Detached checkout of a commit. Refuses when it would overwrite local changes. */
  gitCheckoutCommit: (repoRoot: string, commit: string) =>
    invoke<GitRepoInfo>("git_checkout_commit", { repoRoot, commit }),

  // Environments (D28). Project envs live in <project>/.ilixpatch/environments/, global ones in the app config dir.
  envList: (scope: EnvScope, projectPath: string | null) =>
    invoke<EnvironmentDoc[]>("env_list", { scope, projectPath }),
  /** Create or update. `originalName` renames an existing environment (null = new or same name). */
  envSave: (scope: EnvScope, projectPath: string | null, env: EnvironmentDoc, originalName: string | null) =>
    invoke<EnvironmentDoc>("env_save", { scope, projectPath, env, originalName }),
  envDelete: (scope: EnvScope, projectPath: string | null, name: string) =>
    invoke<void>("env_delete", { scope, projectPath, name }),
  /** Copy between scopes or projects (project ↔ global). Returns the created doc (name made unique: "dev (2)"). */
  envCopy: (from: EnvRef, to: EnvScope, toProjectPath: string | null, newName: string | null) =>
    invoke<EnvironmentDoc>("env_copy", { from, to, toProjectPath, newName }),
  /** Global variables (pm.globals), user-level. */
  globalsGet: () => invoke<EnvVariable[]>("globals_get"),
  globalsSave: (variables: EnvVariable[]) => invoke<void>("globals_save", { variables }),

  // Network, HTTPS & proxy (D29) — stored in the app config dir (user-level).
  networkGetSettings: () => invoke<NetworkSettings>("network_get_settings"),
  /** Validates (files exist, PEM/PFX parse, proxy URL) and saves; error message names the bad field. */
  networkSetSettings: (settings: NetworkSettings) => invoke<void>("network_set_settings", { settings }),

  // Importer: Postman (D31), OpenAPI / Swagger (D53)
  /**
   * Parse Postman exports (collection/environment/globals .json, a data-dump .zip) and OpenAPI 3.x / Swagger 2.0
   * specs (.json/.yaml), or folders of them; `urls` are downloaded with the app's network settings.
   */
  importPreview: (paths: string[], urls: string[], options: ParseOptions) =>
    invoke<ImportPreview>("import_preview", { paths, urls, options }),
  importApply: (plan: ImportPlan) => invoke<ImportReport>("import_apply", { plan }),

  // Update check (D54): asks GitHub with the app's network settings; nothing is downloaded or installed.
  updateCheck: () => invoke<UpdateCheck>("update_check"),

  // Authorization (D39)
  /** Set/clear a collection's auth (collection.toml [auth]); null = no auth. */
  saveCollectionAuth: (collectionPath: string, auth: Auth | null) =>
    invoke<void>("save_collection_auth", { collectionPath, auth }),
  /** Set/clear a folder's auth (folder.toml [auth]); null = inherit (folder.toml removed if empty). */
  saveFolderAuth: (folderPath: string, auth: Auth | null) => invoke<void>("save_folder_auth", { folderPath, auth }),
  /** Set a collection's variables (collection.toml [variables]). */
  saveCollectionVariables: (collectionPath: string, variables: Record<string, string>) =>
    invoke<void>("save_collection_variables", { collectionPath, variables }),
  /** Run the OAuth 2.0 flow for `config` (variables resolved with `variables`): opens the system browser
   *  for code/PKCE/implicit and waits for the loopback redirect (or `cancelOAuth2`). Stores and returns the token. */
  oauth2GetToken: (config: OAuth2Config, variables: SendVariables) =>
    invoke<OAuth2StoredToken>("oauth2_get_token", { config, variables }),
  oauth2CancelFlow: () => invoke<void>("oauth2_cancel_flow"),
  oauth2RefreshToken: (id: string, config: OAuth2Config, variables: SendVariables) =>
    invoke<OAuth2StoredToken>("oauth2_refresh_token", { id, config, variables }),
  /** All stored tokens, or only those for `tokenName`. Newest first. */
  oauth2ListTokens: (tokenName: string | null) => invoke<OAuth2StoredToken[]>("oauth2_list_tokens", { tokenName }),
  oauth2DeleteTokens: (ids: string[]) => invoke<void>("oauth2_delete_tokens", { ids }),
  /** Mark which stored token is used for `tokenName` (default: the newest valid one). */
  oauth2SelectToken: (tokenName: string, id: string) => invoke<void>("oauth2_select_token", { tokenName, id }),

  // Large responses (D41). `pretty` = JSON pretty-printed view (built in Rust, streaming; JSON bodies only).
  /** Bytes [offset, offset+length) of the raw or pretty view (use with BodyLines.offsets to page through a huge line). */
  responseRead: (id: string, offset: number, length: number, pretty = false) =>
    invoke<BodyChunk>("response_read", { id, offset, length, pretty }),
  /** Lines [start, start+count) of the raw or pretty view; builds a line index on first use.
   *  Lines longer than `maxLineBytes` (default 64 KB) are cut — a minified 50 MB JSON line never crosses the bridge whole. */
  responseLines: (id: string, start: number, count: number, pretty: boolean, maxLineBytes = 65536) =>
    invoke<BodyLines>("response_lines", { id, start, count, pretty, maxLineBytes }),
  responseSearch: (
    id: string,
    query: string,
    options: { regex: boolean; case_sensitive: boolean; pretty: boolean; max_results: number },
  ) => invoke<BodySearchResult>("response_search", { id, query, options }),
  /** Copy the body to `path` (streamed from the temp file; never through the UI). */
  responseSave: (id: string, path: string, pretty: boolean) =>
    invoke<void>("response_save", { id, path, pretty }),
  /** Free the body now (temp file deleted). Also done automatically on resend / tab close / LRU. */
  responseRelease: (id: string) => invoke<void>("response_release", { id }),
  /** D44: open the body with the system app (copied to a temp file with extension `ext`). */
  responseOpen: (id: string, ext: string) => invoke<void>("response_open", { id, ext }),

  // Network console (D46): the last 500 exchanges, in memory only. New ones arrive as `netlog://entry` events.
  netlogList: () => invoke<NetEntry[]>("netlog_list"),
  netlogClear: () => invoke<void>("netlog_clear"),
};
