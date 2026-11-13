// Network console helpers (D46): secret masking and "Copy as cURL".
import type { NetEntry } from "$lib/api";

const SECRET_HEADERS = new Set([
  "authorization",
  "proxy-authorization",
  "cookie",
  "set-cookie",
  "x-api-key",
  "api-key",
  "x-auth-token",
  "x-access-token",
  "x-amz-security-token",
]);

export function isSecretHeader(name: string): boolean {
  return SECRET_HEADERS.has(name.toLowerCase());
}

/** "Bearer abc.def" → "Bearer ••••••"; other values → "••••••". */
export function maskValue(name: string, value: string): string {
  if (!isSecretHeader(name)) return value;
  const n = name.toLowerCase();
  if (n === "authorization" || n === "proxy-authorization") {
    const sp = value.indexOf(" ");
    if (sp > 0) return `${value.slice(0, sp)} ••••••`;
  }
  return "••••••";
}

/** Headers the HTTP client adds on its own; curl adds its own versions. */
const CLIENT_HEADERS = new Set(["host", "content-length", "accept-encoding"]);

const q = (s: string) => `'${s.replace(/'/g, `'\\''`)}'`;

/** The exchange as a curl command (real values, also secrets). */
export function entryToCurl(e: NetEntry): string {
  // One argument (flag + value) per line.
  const parts = [e.method !== "GET" || e.request_body ? `curl -X ${e.method} ${q(e.url)}` : `curl ${q(e.url)}`];
  for (const [k, v] of e.request_headers) {
    if (!CLIENT_HEADERS.has(k.toLowerCase())) parts.push(`-H ${q(`${k}: ${v}`)}`);
  }
  if (e.request_headers.some(([k]) => k.toLowerCase() === "accept-encoding")) parts.push("--compressed");
  if (e.request_body && !e.request_body.binary) parts.push(`--data-raw ${q(e.request_body.text)}`);
  return parts.join(" \\\n  ");
}

export function timeOf(ms: number): string {
  const d = new Date(ms);
  const p = (n: number, w = 2) => String(n).padStart(w, "0");
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}.${p(d.getMilliseconds(), 3)}`;
}

export function statusClass(e: NetEntry): "ok" | "redirect" | "warn" | "err" {
  if (e.error || e.status === null) return "err";
  if (e.status >= 500) return "err";
  if (e.status >= 400) return "warn";
  if (e.status >= 300) return "redirect";
  return "ok";
}
