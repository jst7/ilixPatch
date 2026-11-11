// Normalise whatever `api.sendRequest` rejects with into a RequestError (D36).
// Older backends / stored history entries use plain strings.
import type { RequestError, RequestErrorKind } from "$lib/api";
import { t, type MessageKey } from "$lib/i18n/index.svelte";

const KINDS: RequestErrorKind[] = [
  "invalid_url", "undefined_variables", "dns", "connection_refused", "connection_reset", "timeout",
  "tls_unknown_issuer", "tls_expired", "tls_hostname_mismatch", "tls_handshake", "tls_client_certificate",
  "proxy", "too_many_redirects", "body", "skipped_by_script", "cancelled", "other",
];

export function isKnownKind(k: unknown): k is RequestErrorKind {
  return typeof k === "string" && (KINDS as string[]).includes(k);
}

export function toRequestError(e: unknown): RequestError {
  if (e && typeof e === "object" && "message" in e) {
    const o = e as Partial<RequestError> & { message: unknown };
    const message = String(o.message ?? "");
    return {
      ...o,
      kind: isKnownKind(o.kind) ? o.kind : "other",
      message,
      detail: typeof o.detail === "string" && o.detail ? o.detail : message,
    } as RequestError;
  }
  const message = String(e);
  return { kind: message === "cancelled" ? "cancelled" : "other", message, detail: message };
}

export interface ErrorDescription {
  title: string;
  cause: string;
  /** Suggestions (may be empty). */
  tips: string[];
}

/** Localized title / likely cause / tips for an error; falls back to `message` without a translation. */
export function describeRequestError(error: RequestError, env?: string): ErrorDescription {
  const key = (part: "title" | "cause" | "tips") => `reqerr.${error.kind}.${part}` as MessageKey;
  const has = (k: MessageKey) => t(k) !== k;
  const host = error.host || t("reqerr.theServer");
  const target = error.host ? (error.port ? `${error.host}:${error.port}` : error.host) : (error.url ?? t("reqerr.theServer"));
  const title = has(key("title")) ? t(key("title")) : error.message;
  const cause = has(key("cause"))
    ? t(key("cause"), {
        host,
        port: error.port ?? "",
        target,
        url: error.url ?? "",
        timeout: error.timeout_ms ? `${error.timeout_ms} ms` : t("reqerr.theLimit"),
        proxy: error.proxy ?? "",
        variables: (error.variables ?? []).join(", "),
        env: env ?? t("reqerr.noEnvironment"),
        app: t("app.name"),
        message: error.message,
      })
    : error.message;
  // Tips have no placeholders (some contain literal {{variables}}), so no params are passed.
  const tips = has(key("tips")) ? t(key("tips")).split("\n").filter(Boolean) : [];
  return { title, cause, tips };
}
