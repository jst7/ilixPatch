// cURL import/export (D38). Pure module (type-only imports) so Node can run the self-test
// directly: `node src/lib/curl.selftest.mjs`.
//
// parseCurl understands what browsers ("Copy as cURL" for bash / cmd / PowerShell) and Postman
// produce: single/double/$'…' quoting, line continuations (\ ^ `), and the common flags.

import type { Auth, AuthType, Body, HttpMethod, Request, RequestSettings } from "./api";

const METHODS: HttpMethod[] = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

export interface CurlImport {
  request: Request;
  /** Human-readable notes about flags that were ignored or approximated (English, short). */
  warnings: string[];
}

/** True when the text looks like a curl command (used to intercept pastes in the URL bar). */
export function looksLikeCurl(text: string): boolean {
  return /^\s*curl(\.exe)?\s/i.test(text);
}

// ---------- tokenizer ----------

/** Split a shell command line into arguments (bash, Windows cmd and PowerShell styles). */
export function tokenize(input: string): string[] {
  // Windows cmd "Copy as cURL" escapes with ^ (e.g. ^"…^", ^&, ^%^). Detect and normalise it.
  let s = input.replace(/\r\n?/g, "\n");
  const cmdStyle = /\^"/.test(s) || /\^\n/.test(s);
  if (cmdStyle) s = s.replace(/\^\n/g, " ").replace(/\^(.)/g, "$1");
  // Line continuations: bash `\⏎`, PowerShell `` `⏎ ``.
  s = s.replace(/\\\n/g, " ").replace(/`\n/g, " ");

  const out: string[] = [];
  let cur = "";
  let has = false;
  let i = 0;
  const push = () => {
    if (has) out.push(cur);
    cur = "";
    has = false;
  };
  while (i < s.length) {
    const c = s[i];
    if (c === " " || c === "\t" || c === "\n") {
      push();
      i++;
    } else if (c === "'" ) {
      const end = s.indexOf("'", i + 1);
      if (end < 0) throw new Error("Unterminated ' quote");
      cur += s.slice(i + 1, end);
      has = true;
      i = end + 1;
    } else if (c === "$" && s[i + 1] === "'") {
      // ANSI-C quoting ($'…'), used by Chrome for bodies with special characters.
      i += 2;
      while (i < s.length && s[i] !== "'") {
        if (s[i] === "\\" && i + 1 < s.length) {
          const n = s[i + 1];
          const map: Record<string, string> = { n: "\n", t: "\t", r: "\r", "\\": "\\", "'": "'", '"': '"', "0": "\0", e: "\x1b" };
          if (n === "x") {
            cur += String.fromCharCode(parseInt(s.slice(i + 2, i + 4), 16));
            i += 4;
          } else if (n === "u") {
            cur += String.fromCharCode(parseInt(s.slice(i + 2, i + 6), 16));
            i += 6;
          } else {
            cur += map[n] ?? n;
            i += 2;
          }
        } else {
          cur += s[i++];
        }
      }
      if (i >= s.length) throw new Error("Unterminated $' quote");
      has = true;
      i++;
    } else if (c === '"') {
      i++;
      while (i < s.length && s[i] !== '"') {
        if (s[i] === "\\" && i + 1 < s.length && '"\\$`'.includes(s[i + 1])) {
          cur += s[i + 1];
          i += 2;
        } else if (s[i] === "`" && i + 1 < s.length && '"`'.includes(s[i + 1])) {
          // PowerShell escape inside double quotes
          cur += s[i + 1];
          i += 2;
        } else {
          cur += s[i++];
        }
      }
      if (i >= s.length) throw new Error('Unterminated " quote');
      has = true;
      i++;
    } else if (c === "\\" && i + 1 < s.length) {
      cur += s[i + 1];
      has = true;
      i += 2;
    } else {
      cur += c;
      has = true;
      i++;
    }
  }
  push();
  return out;
}

// ---------- parser ----------

const NO_VALUE_IGNORED = new Set([
  "-s", "--silent", "-S", "--show-error", "-v", "--verbose", "-i", "--include", "-f", "--fail",
  "--fail-with-body", "-#", "--progress-bar", "-N", "--no-buffer", "--compressed", "--tr-encoding",
  "-n", "--netrc", "--globoff", "-g", "--path-as-is", "-q", "--disable", "-4", "--ipv4", "-6", "--ipv6",
]);
const VALUE_IGNORED = new Set([
  "-o", "--output", "--connect-timeout", "--retry", "--retry-delay", "--retry-max-time", "-w",
  "--write-out", "--limit-rate", "-r", "--range", "-c", "--cookie-jar", "-D", "--dump-header",
  "--resolve", "--interface", "--dns-servers", "--trace", "--trace-ascii", "--stderr",
]);

function headerName(h: string): string {
  return h.slice(0, Math.max(0, h.indexOf(":"))).trim();
}

/** Parse a curl command into a Request. Throws with a readable message when it isn't valid curl. */
export function parseCurl(command: string): CurlImport {
  const args = tokenize(command.trim());
  if (!args.length || !/^curl(\.exe)?$/i.test(args[0])) throw new Error("Not a curl command (it must start with `curl`)");

  const warnings: string[] = [];
  let method: string | null = null;
  let url: string | null = null;
  const headers: [string, string][] = [];
  const data: { kind: "raw" | "urlencode" | "binary"; value: string }[] = [];
  const form: [string, string][] = [];
  let getMode = false;
  let head = false;
  let json = false;
  const settings: RequestSettings = {};
  let user: string | null = null;
  let authScheme: "basic" | "digest" | "ntlm" = "basic";
  let awsSigv4: string | null = null;
  let oauth2Bearer: string | null = null;

  const value = (i: number, flag: string): string => {
    if (i >= args.length) throw new Error(`Missing value for ${flag}`);
    return args[i];
  };

  for (let i = 1; i < args.length; i++) {
    let a = args[i];
    // --flag=value form
    let inline: string | null = null;
    if (a.startsWith("--") && a.includes("=")) {
      inline = a.slice(a.indexOf("=") + 1);
      a = a.slice(0, a.indexOf("="));
    }
    // Short flag with attached value: -XPOST, -H'…'
    if (/^-[XHdAeubmFx]./.test(a) && !a.startsWith("--")) {
      inline = a.slice(2);
      a = a.slice(0, 2);
    }
    const next = () => (inline !== null ? inline : value(++i, a));

    switch (a) {
      case "-X":
      case "--request":
        method = next().toUpperCase();
        break;
      case "--url":
        url = next();
        break;
      case "-H":
      case "--header": {
        const h = next();
        const idx = h.indexOf(":");
        if (idx <= 0) {
          warnings.push(`Ignored malformed header "${h}"`);
          break;
        }
        // `-H 'X-Foo;'` means an empty header in curl
        headers.push([h.slice(0, idx).trim(), h.slice(idx + 1).trim()]);
        break;
      }
      case "-d":
      case "--data":
      case "--data-ascii":
      case "--data-raw":
        data.push({ kind: "raw", value: next() });
        break;
      case "--data-binary": {
        const v = next();
        if (v.startsWith("@")) warnings.push(`File body "${v.slice(1)}" is not supported; paste its content instead`);
        else data.push({ kind: "binary", value: v });
        break;
      }
      case "--data-urlencode":
        data.push({ kind: "urlencode", value: next() });
        break;
      case "--json":
        data.push({ kind: "raw", value: next() });
        json = true;
        break;
      case "-F":
      case "--form":
      case "--form-string": {
        const f = next();
        const eq = f.indexOf("=");
        if (eq <= 0) break;
        const k = f.slice(0, eq);
        const v = f.slice(eq + 1);
        if (v.startsWith("@") || v.startsWith("<")) warnings.push(`File field "${k}" is not supported (multipart files)`);
        else form.push([k, v]);
        break;
      }
      case "-u":
      case "--user":
        user = next();
        break;
      case "--basic":
      case "--anyauth":
        authScheme = "basic";
        break;
      case "--digest":
        authScheme = "digest";
        break;
      case "--ntlm":
        authScheme = "ntlm";
        break;
      case "--aws-sigv4":
        awsSigv4 = next();
        break;
      case "--oauth2-bearer":
        oauth2Bearer = next();
        break;
      case "-A":
      case "--user-agent":
        headers.push(["User-Agent", next()]);
        break;
      case "-e":
      case "--referer":
        headers.push(["Referer", next()]);
        break;
      case "-b":
      case "--cookie": {
        const c = next();
        if (c.includes("=")) headers.push(["Cookie", c]);
        else warnings.push(`Cookie file "${c}" ignored`);
        break;
      }
      case "-G":
      case "--get":
        getMode = true;
        break;
      case "-I":
      case "--head":
        head = true;
        break;
      case "-k":
      case "--insecure":
        settings.verify_ssl = false;
        break;
      case "-L":
      case "--location":
        settings.follow_redirects = true;
        break;
      case "--max-redirs":
        settings.max_redirects = Number(next()) || null;
        break;
      case "-m":
      case "--max-time": {
        const secs = Number(next());
        if (Number.isFinite(secs) && secs > 0) settings.timeout_ms = Math.round(secs * 1000);
        break;
      }
      case "--http1.0":
      case "--http1.1":
        settings.http_version = "http1";
        break;
      case "--http2":
      case "--http2-prior-knowledge":
        settings.http_version = "http2";
        break;
      case "-x":
      case "--proxy":
        warnings.push(`Proxy "${next()}" ignored — configure it in Settings › Proxy`);
        break;
      case "-E":
      case "--cert":
      case "--key":
      case "--cacert":
        warnings.push(`${a} ${next()} ignored — configure certificates in Settings › HTTPS`);
        break;
      default:
        if (NO_VALUE_IGNORED.has(a)) break;
        if (VALUE_IGNORED.has(a)) {
          if (inline === null) i++;
          break;
        }
        if (a.startsWith("-") && a.length > 1) {
          warnings.push(`Unsupported option ${a} ignored`);
          break;
        }
        if (url === null) url = a;
        else warnings.push(`Extra argument "${a}" ignored`);
    }
  }

  if (!url) throw new Error("No URL found in the curl command");
  if (!/^[a-z][a-z0-9+.-]*:\/\//i.test(url) && !url.startsWith("{{")) url = `http://${url}`;

  const hasHeader = (name: string) => headers.some(([k]) => k.toLowerCase() === name.toLowerCase());
  const contentType = headers.find(([k]) => k.toLowerCase() === "content-type")?.[1]?.toLowerCase() ?? "";

  // -G: data goes to the query string instead of the body.
  if (getMode && data.length) {
    const qs = data.map((d) => (d.kind === "urlencode" ? encodeUrlencodeArg(d.value) : d.value)).join("&");
    url += (url.includes("?") ? "&" : "?") + qs;
    data.length = 0;
  }

  let body: Body | null = null;
  if (form.length) {
    body = { type: "form", fields: Object.fromEntries(form) };
    warnings.push("multipart/form-data was imported as a URL-encoded form (multipart is not supported yet)");
  } else if (data.length) {
    const urlencodedOnly = data.every((d) => d.kind === "urlencode");
    const raw = data.map((d) => (d.kind === "urlencode" ? encodeUrlencodeArg(d.value) : d.value)).join("&");
    if (json || contentType.includes("json") || (!contentType && looksLikeJson(raw))) {
      body = { type: "json", content: prettyJson(raw) };
      if (json && !hasHeader("Content-Type")) headers.push(["Content-Type", "application/json"]);
      if (json && !hasHeader("Accept")) headers.push(["Accept", "application/json"]);
    } else if (urlencodedOnly || contentType.includes("x-www-form-urlencoded") || (!contentType && /^[^=&\s]+=[^&]*(&[^=&\s]+=[^&]*)*$/.test(raw))) {
      body = { type: "form", fields: Object.fromEntries(raw.split("&").map(splitPair)) };
    } else {
      body = { type: "text", content: raw };
    }
  }

  let finalMethod = (method ?? (head ? "HEAD" : body ? "POST" : "GET")).toUpperCase();
  if (!METHODS.includes(finalMethod as HttpMethod)) {
    warnings.push(`Method ${finalMethod} is not supported; using GET`);
    finalMethod = "GET";
  }

  // Duplicate header names: keep the last value (our model is a map), warn once.
  const headerMap: Record<string, string> = {};
  for (const [k, v] of headers) {
    const existing = Object.keys(headerMap).find((h) => h.toLowerCase() === k.toLowerCase());
    if (existing && existing.toLowerCase() !== "cookie") warnings.push(`Header ${k} appears more than once; kept the last value`);
    if (existing && existing.toLowerCase() === "cookie") headerMap[existing] += `; ${v}`;
    else headerMap[existing ?? k] = v;
  }

  const request: Request = {
    name: nameFromUrl(url),
    method: finalMethod as HttpMethod,
    url,
    headers: headerMap,
    query: {},
    body,
  };
  if (Object.keys(settings).length) request.settings = settings;
  const auth = authFromFlags(user, authScheme, awsSigv4, oauth2Bearer, warnings);
  if (auth) request.auth = auth;
  return { request, warnings };
}

/** Map curl's credential flags to an Authorization (D39) instead of a raw header. */
function authFromFlags(
  user: string | null,
  scheme: "basic" | "digest" | "ntlm",
  awsSigv4: string | null,
  bearer: string | null,
  warnings: string[],
): Auth | null {
  const split = (cred: string): [string, string] => {
    const i = cred.indexOf(":");
    return i < 0 ? [cred, ""] : [cred.slice(0, i), cred.slice(i + 1)];
  };
  if (awsSigv4 !== null) {
    // --aws-sigv4 "aws:amz:<region>:<service>"
    const parts = awsSigv4.split(":");
    const [access_key, secret_key] = user ? split(user) : ["", ""];
    if (!user) warnings.push("--aws-sigv4 without -u: fill in the access and secret keys");
    return { type: "awsv4", access_key, secret_key, region: parts[2] ?? "", service: parts[3] ?? "" };
  }
  if (user !== null) {
    const [username, password] = split(user);
    if (scheme === "digest") return { type: "digest", username, password };
    if (scheme === "ntlm") {
      const bs = username.indexOf("\\");
      return bs > 0
        ? { type: "ntlm", username: username.slice(bs + 1), password, domain: username.slice(0, bs) }
        : { type: "ntlm", username, password };
    }
    return { type: "basic", username, password };
  }
  if (bearer !== null) return { type: "bearer", token: bearer };
  return null;
}

function splitPair(p: string): [string, string] {
  const eq = p.indexOf("=");
  const dec = (x: string) => {
    try {
      return decodeURIComponent(x.replace(/\+/g, " "));
    } catch {
      return x;
    }
  };
  return eq < 0 ? [dec(p), ""] : [dec(p.slice(0, eq)), dec(p.slice(eq + 1))];
}

/** curl --data-urlencode semantics: "name=value" → name=encoded(value); "value" → encoded(value). */
function encodeUrlencodeArg(v: string): string {
  const eq = v.indexOf("=");
  if (eq < 0) return encodeURIComponent(v);
  return `${v.slice(0, eq)}=${encodeURIComponent(v.slice(eq + 1))}`;
}

function looksLikeJson(s: string): boolean {
  const t = s.trim();
  if (!(t.startsWith("{") || t.startsWith("["))) return false;
  try {
    JSON.parse(t);
    return true;
  } catch {
    return false;
  }
}

function prettyJson(s: string): string {
  try {
    return JSON.stringify(JSON.parse(s), null, 2);
  } catch {
    return s; // keep as-is (may contain {{variables}})
  }
}

function nameFromUrl(url: string): string {
  try {
    const u = new URL(url.replace(/{{[^}]*}}/g, "x"));
    const last = u.pathname.split("/").filter(Boolean).pop();
    return last ? decodeURIComponent(last) : u.host;
  } catch {
    return "Imported from cURL";
  }
}

// ---------- export ----------

export interface CurlExportOptions {
  /** One flag per line with `\` continuations (like Postman). Default true. */
  multiline?: boolean;
}

/** POSIX shell single-quote quoting. */
export function shellQuote(s: string): string {
  if (/^[A-Za-z0-9_@%+=:,./-]+$/.test(s)) return s;
  return `'${s.replace(/'/g, `'\\''`)}'`;
}

/** Build a curl command for a request (query map appended to the URL, settings mapped to flags). */
export function toCurl(req: Request, opts: CurlExportOptions = {}): string {
  const multiline = opts.multiline ?? true;
  let url = req.url;
  const query = Object.entries(req.query ?? {});
  if (query.length) {
    const qs = query.map(([k, v]) => `${encodeURIComponent(k)}=${encodeURIComponent(v)}`).join("&");
    url += (url.includes("?") ? (url.endsWith("?") || url.endsWith("&") ? "" : "&") : "?") + qs;
  }
  const authParts: string[] = [];
  const auth = req.auth ?? null;
  const skipped = unexportedAuth(auth);
  if (auth && !skipped) {
    switch (auth.type) {
      case "bearer":
        authParts.push(`-H ${shellQuote(`Authorization: Bearer ${auth.token}`)}`);
        break;
      case "basic":
        authParts.push(`-u ${shellQuote(`${auth.username}:${auth.password ?? ""}`)}`);
        break;
      case "digest":
        authParts.push(`--digest -u ${shellQuote(`${auth.username}:${auth.password ?? ""}`)}`);
        break;
      case "ntlm": {
        const u = auth.domain ? `${auth.domain}\\${auth.username}` : auth.username;
        authParts.push(`--ntlm -u ${shellQuote(`${u}:${auth.password ?? ""}`)}`);
        break;
      }
      case "api_key":
        if ((auth.in ?? "header") === "query") {
          const pair = `${encodeURIComponent(auth.key)}=${encodeURIComponent(auth.value)}`;
          url += (url.includes("?") ? (url.endsWith("?") || url.endsWith("&") ? "" : "&") : "?") + pair;
        } else {
          authParts.push(`-H ${shellQuote(`${auth.key}: ${auth.value}`)}`);
        }
        break;
      case "awsv4":
        authParts.push(`--aws-sigv4 ${shellQuote(`aws:amz:${auth.region}:${auth.service}`)}`);
        authParts.push(`-u ${shellQuote(`${auth.access_key}:${auth.secret_key}`)}`);
        if (auth.session_token) authParts.push(`-H ${shellQuote(`x-amz-security-token: ${auth.session_token}`)}`);
        break;
      case "oauth2":
        authParts.push(`-H ${shellQuote(`Authorization: ${auth.header_prefix || "Bearer"} ${auth.access_token ?? ""}`)}`);
        break;
    }
  }
  const parts: string[] = [];
  if (req.method === "HEAD") parts.push("--head");
  else if (req.method !== "GET" || (req.body && req.method === "GET")) parts.push(`-X ${req.method}`);
  parts.push(shellQuote(url));
  const headers = Object.entries(req.headers ?? {});
  for (const [k, v] of headers) parts.push(`-H ${shellQuote(`${k}: ${v}`)}`);
  parts.push(...authParts);
  const body = req.body ?? null;
  if (body?.type === "form") {
    for (const [k, v] of Object.entries(body.fields)) parts.push(`--data-urlencode ${shellQuote(`${k}=${v}`)}`);
  } else if (body) {
    const hasType = headers.some(([k]) => k.toLowerCase() === "content-type");
    if (body.type === "json" && !hasType) parts.push(`-H ${shellQuote("Content-Type: application/json")}`);
    parts.push(`--data-raw ${shellQuote(body.content)}`);
  }
  const s = req.settings ?? {};
  if (s.verify_ssl === false) parts.push("--insecure");
  if (s.follow_redirects !== false) parts.push("--location");
  if (s.max_redirects != null) parts.push(`--max-redirs ${s.max_redirects}`);
  if (s.timeout_ms) parts.push(`--max-time ${+(s.timeout_ms / 1000).toFixed(3)}`);
  if (s.http_version === "http1") parts.push("--http1.1");
  if (s.http_version === "http2") parts.push("--http2");
  if (!multiline) return `curl ${parts.join(" ")}`;
  const note = skipped ? `# auth ${skipped} not exported\n` : "";
  return `${note}curl ${parts.join(" \\\n  ")}`;
}

/**
 * Auth types curl can't express (exported without auth): returns the type, or null when the
 * auth is absent or exportable. OAuth 2.0 exports only with a literal access token.
 */
export function unexportedAuth(auth: Auth | null | undefined): AuthType | null {
  if (!auth) return null;
  switch (auth.type) {
    case "inherit":
    case "no_auth":
    case "bearer":
    case "basic":
    case "digest":
    case "ntlm":
    case "awsv4":
      return null;
    case "api_key":
      return auth.in === "body" ? auth.type : null;
    case "oauth2":
      return auth.access_token ? null : auth.type;
    default:
      return auth.type;
  }
}

/** Replace {{name}} with values from `vars` (unknown and dynamic `{{$…}}` variables are left as-is). */
export function interpolate(text: string, vars: Record<string, string>): string {
  return text.replace(/{{\s*([^{}]+?)\s*}}/g, (m, name: string) => (name in vars ? vars[name] : m));
}

/** Copy of `req` with variables resolved in URL, headers, query and body. */
export function resolveRequest(req: Request, vars: Record<string, string>): Request {
  const map = (o?: Record<string, string>) =>
    Object.fromEntries(Object.entries(o ?? {}).map(([k, v]) => [interpolate(k, vars), interpolate(v, vars)]));
  const body: Body | null | undefined = req.body
    ? req.body.type === "form"
      ? { type: "form", fields: map(req.body.fields) }
      : { type: req.body.type, content: interpolate(req.body.content, vars) }
    : req.body;
  const auth = req.auth
    ? (Object.fromEntries(
        Object.entries(req.auth).map(([k, v]) => [k, typeof v === "string" && k !== "type" ? interpolate(v, vars) : v]),
      ) as unknown as Auth)
    : req.auth;
  return { ...req, url: interpolate(req.url, vars), headers: map(req.headers), query: map(req.query), body, auth };
}
