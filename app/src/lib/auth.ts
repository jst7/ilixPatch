// Authorization (D39) helpers: type catalogue, form field descriptors, defaults, normalisation
// and inheritance. Pure module (type-only imports).
import type { Auth, AuthType, Collection, Folder } from "./api";

/** Type groups shown in the type dropdown (order = Postman's). */
export const AUTH_GROUPS: { id: "inherit" | "common" | "credentials" | "signatures" | "oauth"; types: AuthType[] }[] = [
  { id: "inherit", types: ["inherit", "no_auth"] },
  { id: "common", types: ["api_key", "bearer", "jwt", "basic", "digest"] },
  { id: "oauth", types: ["oauth1", "oauth2"] },
  { id: "signatures", types: ["hawk", "awsv4", "ntlm", "akamai", "asap"] },
];

export type FieldKind = "text" | "secret" | "pem" | "json" | "select" | "bool" | "number";

export interface AuthField {
  key: string;
  kind: FieldKind;
  /** Select options (values); "" = not set (null). */
  options?: string[];
  /** Whether a "" option (backend default) is offered for selects. */
  optional?: boolean;
  advanced?: boolean;
  /** Placeholder (non-translated example such as "Bearer"). */
  placeholder?: string;
  show?: (a: Record<string, unknown>) => boolean;
}

const HS = (a: Record<string, unknown>) => String(a.algorithm ?? "").startsWith("HS");
const RSA1 = (a: Record<string, unknown>) => String(a.signature_method ?? "").startsWith("RSA");

export const JWT_ALGORITHMS = ["HS256", "HS384", "HS512", "RS256", "RS384", "RS512", "PS256", "PS384", "PS512", "ES256", "ES384", "ES512"];
const ASAP_ALGORITHMS = JWT_ALGORITHMS.filter((x) => !x.startsWith("HS"));

/** Form fields per type (oauth2 has its own panel). */
export const AUTH_FIELDS: Partial<Record<AuthType, AuthField[]>> = {
  api_key: [
    { key: "key", kind: "text", placeholder: "X-API-Key" },
    { key: "value", kind: "secret" },
    { key: "in", kind: "select", options: ["header", "query"] },
  ],
  bearer: [{ key: "token", kind: "secret" }],
  jwt: [
    { key: "add_to", kind: "select", options: ["header", "query"] },
    { key: "algorithm", kind: "select", options: JWT_ALGORITHMS },
    { key: "secret", kind: "secret", show: HS },
    { key: "secret_base64", kind: "bool", show: HS },
    { key: "private_key", kind: "pem", show: (a) => !HS(a) },
    { key: "payload", kind: "json" },
    { key: "header_prefix", kind: "text", advanced: true, placeholder: "Bearer" },
    { key: "query_param", kind: "text", advanced: true, placeholder: "token" },
    { key: "header", kind: "json", advanced: true },
  ],
  basic: [
    { key: "username", kind: "text" },
    { key: "password", kind: "secret" },
  ],
  digest: [
    { key: "username", kind: "text" },
    { key: "password", kind: "secret" },
    { key: "disable_retry", kind: "bool" },
    { key: "realm", kind: "text", advanced: true },
    { key: "nonce", kind: "text", advanced: true },
    {
      key: "algorithm",
      kind: "select",
      optional: true,
      advanced: true,
      options: ["MD5", "MD5-sess", "SHA-256", "SHA-256-sess", "SHA-512-256", "SHA-512-256-sess"],
    },
    { key: "qop", kind: "text", advanced: true, placeholder: "auth" },
    { key: "nonce_count", kind: "text", advanced: true, placeholder: "00000001" },
    { key: "client_nonce", kind: "text", advanced: true },
    { key: "opaque", kind: "text", advanced: true },
  ],
  oauth1: [
    { key: "add_to", kind: "select", options: ["header", "query", "body"] },
    {
      key: "signature_method",
      kind: "select",
      options: ["HMAC-SHA1", "HMAC-SHA256", "HMAC-SHA512", "RSA-SHA1", "RSA-SHA256", "RSA-SHA512", "PLAINTEXT"],
    },
    { key: "consumer_key", kind: "text" },
    { key: "consumer_secret", kind: "secret", show: (a) => !RSA1(a) },
    { key: "private_key", kind: "pem", show: RSA1 },
    { key: "token", kind: "secret" },
    { key: "token_secret", kind: "secret", show: (a) => !RSA1(a) },
    { key: "callback_url", kind: "text", advanced: true },
    { key: "verifier", kind: "text", advanced: true },
    { key: "timestamp", kind: "text", advanced: true },
    { key: "nonce", kind: "text", advanced: true },
    { key: "version", kind: "text", advanced: true, placeholder: "1.0" },
    { key: "realm", kind: "text", advanced: true },
    { key: "include_body_hash", kind: "bool", advanced: true },
    { key: "add_empty_params", kind: "bool", advanced: true },
  ],
  hawk: [
    { key: "id", kind: "text" },
    { key: "key", kind: "secret" },
    { key: "algorithm", kind: "select", optional: true, options: ["sha256", "sha1"] },
    { key: "user", kind: "text", advanced: true },
    { key: "nonce", kind: "text", advanced: true },
    { key: "ext", kind: "text", advanced: true },
    { key: "app", kind: "text", advanced: true },
    { key: "dlg", kind: "text", advanced: true },
    { key: "timestamp", kind: "text", advanced: true },
    { key: "include_payload_hash", kind: "bool", advanced: true },
  ],
  awsv4: [
    { key: "access_key", kind: "text" },
    { key: "secret_key", kind: "secret" },
    { key: "region", kind: "text", placeholder: "us-east-1" },
    { key: "service", kind: "text", placeholder: "execute-api" },
    { key: "session_token", kind: "secret", advanced: true },
    { key: "add_to", kind: "select", options: ["header", "query"], advanced: true },
  ],
  ntlm: [
    { key: "username", kind: "text" },
    { key: "password", kind: "secret" },
    { key: "domain", kind: "text", advanced: true },
    { key: "workstation", kind: "text", advanced: true },
    { key: "disable_retry", kind: "bool", advanced: true },
  ],
  akamai: [
    { key: "access_token", kind: "secret" },
    { key: "client_token", kind: "secret" },
    { key: "client_secret", kind: "secret" },
    { key: "base_url", kind: "text", advanced: true },
    { key: "nonce", kind: "text", advanced: true },
    { key: "timestamp", kind: "text", advanced: true },
    { key: "headers_to_sign", kind: "text", advanced: true },
    { key: "max_body_size", kind: "number", advanced: true },
  ],
  asap: [
    { key: "algorithm", kind: "select", options: ASAP_ALGORITHMS },
    { key: "issuer", kind: "text" },
    { key: "audience", kind: "text" },
    { key: "key_id", kind: "text" },
    { key: "private_key", kind: "pem" },
    { key: "subject", kind: "text", advanced: true },
    { key: "additional_claims", kind: "json", advanced: true },
    { key: "expires_in", kind: "number", advanced: true, placeholder: "3600" },
  ],
};

/** A fresh auth of `type` with the required fields filled with defaults. */
export function defaultAuth(type: AuthType): Auth {
  switch (type) {
    case "inherit":
    case "no_auth":
      return { type };
    case "api_key":
      return { type, key: "", value: "", in: "header" };
    case "bearer":
      return { type, token: "" };
    case "jwt":
      return { type, algorithm: "HS256", secret: "", payload: "{}", add_to: "header" };
    case "basic":
      return { type, username: "", password: "" };
    case "digest":
      return { type, username: "", password: "" };
    case "oauth1":
      return { type, signature_method: "HMAC-SHA1", consumer_key: "", consumer_secret: "", add_to: "header" };
    case "oauth2":
      return {
        type,
        grant_type: "authorization_code",
        token_name: "Token",
        client_id: "",
        client_secret: "",
        use_local_callback: true,
        client_authentication: "basic",
        add_to: "header",
        auto_refresh: true,
        auto_fetch: false,
      };
    case "hawk":
      return { type, id: "", key: "", algorithm: "sha256" };
    case "awsv4":
      return { type, access_key: "", secret_key: "", region: "", service: "" };
    case "ntlm":
      return { type, username: "", password: "" };
    case "akamai":
      return { type, client_token: "", client_secret: "", access_token: "" };
    case "asap":
      return { type, algorithm: "RS256", issuer: "", audience: "", key_id: "", private_key: "" };
  }
}

/** Deep copy without null/undefined fields (what we send/save). */
export function cleanAuth(a: Auth): Auth {
  const out: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(a)) {
    if (v === null || v === undefined) continue;
    out[k] = v && typeof v === "object" ? JSON.parse(JSON.stringify(v)) : v;
  }
  return out as unknown as Auth;
}

/** Order- and default-insensitive form for dirty checks (null/undefined/"" ignored). */
export function canonicalAuth(a: Auth | null | undefined): string | null {
  if (!a || a.type === "inherit") return null;
  const norm = (v: unknown): unknown => {
    if (Array.isArray(v)) return v.map(norm);
    if (v && typeof v === "object") {
      const o: Record<string, unknown> = {};
      for (const k of Object.keys(v).sort()) {
        const x = (v as Record<string, unknown>)[k];
        if (x === null || x === undefined || x === "") continue;
        o[k] = norm(x);
      }
      return o;
    }
    return v;
  };
  return JSON.stringify(norm(a));
}

export interface AuthSource {
  kind: "folder" | "collection";
  path: string;
  name: string;
}

export interface EffectiveAuth {
  auth: Auth;
  /** Where an inherited auth comes from (null = the item's own auth, or nothing anywhere). */
  source: AuthSource | null;
}

/**
 * What a parent chain provides: walk folders from the innermost outwards, then the collection.
 * `folders` are ordered from the collection root downwards (excluding root).
 */
export function inheritedAuth(collection: Collection, folders: Folder[]): EffectiveAuth {
  for (let i = folders.length - 1; i >= 0; i--) {
    const f = folders[i];
    if (f.auth && f.auth.type !== "inherit") return { auth: f.auth, source: { kind: "folder", path: f.path, name: f.name } };
  }
  const c = collection.auth;
  if (c && c.type !== "inherit" && c.type !== "no_auth") {
    return { auth: c, source: { kind: "collection", path: collection.path, name: collection.name } };
  }
  return { auth: { type: "no_auth" }, source: null };
}

/** The auth actually used when sending: own auth unless it inherits. */
export function effectiveAuth(own: Auth | null | undefined, collection: Collection | null, folders: Folder[]): EffectiveAuth {
  if (own && own.type !== "inherit") return { auth: own, source: null };
  if (!collection) return { auth: { type: "no_auth" }, source: null };
  return inheritedAuth(collection, folders);
}

/** Secret-looking field holding a literal value (not a {{variable}}). */
export function isLiteralSecret(value: unknown): boolean {
  return typeof value === "string" && value.trim() !== "" && !/{{\s*[^{}]+\s*}}/.test(value);
}
