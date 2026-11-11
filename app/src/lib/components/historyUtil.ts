// Helpers for the request history UI: date formatting, cURL export and shared actions.
import { ask } from "@tauri-apps/plugin-dialog";
import { api, type HistoryEntrySummary, type Request } from "$lib/api";
import { i18n, t } from "$lib/i18n/index.svelte";
import { history } from "$lib/stores/history.svelte";
import { ws } from "$lib/stores/workspace.svelte";

const DAY = 24 * 60 * 60 * 1000;

function startOfDay(ms: number): number {
  const d = new Date(ms);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

/** "Today", "Yesterday" or a localized date. */
export function dayLabel(ms: number, now = Date.now()): string {
  const diff = Math.round((startOfDay(now) - startOfDay(ms)) / DAY);
  if (diff === 0) return t("history.today");
  if (diff === 1) return t("history.yesterday");
  const sameYear = new Date(ms).getFullYear() === new Date(now).getFullYear();
  return new Intl.DateTimeFormat(i18n.locale, {
    weekday: "short",
    day: "numeric",
    month: "short",
    ...(sameYear ? {} : { year: "numeric" }),
  }).format(ms);
}

export function dayKey(ms: number): number {
  return startOfDay(ms);
}

export function timeOfDay(ms: number): string {
  return new Intl.DateTimeFormat(i18n.locale, { hour: "2-digit", minute: "2-digit" }).format(ms);
}

/** "just now", "5 min ago" within the last hour, else hh:mm. */
export function timeAgo(ms: number, now = Date.now()): string {
  const diff = now - ms;
  if (diff < 60_000) return t("history.justNow");
  if (diff < 3_600_000) return t("history.minutesAgo", { n: Math.floor(diff / 60_000) });
  return timeOfDay(ms);
}

export function dateTime(ms: number): string {
  return new Intl.DateTimeFormat(i18n.locale, { dateStyle: "medium", timeStyle: "short" }).format(ms);
}

export function formatBytes(n: number): string {
  const nf = (v: number, d: number) => new Intl.NumberFormat(i18n.locale, { maximumFractionDigits: d }).format(v);
  if (n < 1024) return `${nf(n, 0)} B`;
  if (n < 1024 * 1024) return `${nf(n / 1024, 1)} KB`;
  return `${nf(n / 1024 / 1024, 1)} MB`;
}

// ---------- cURL ----------

function shellQuote(s: string): string {
  return `'${s.replace(/'/g, `'\\''`)}'`;
}

/** Build a cURL command for a request (query params not already in the URL are appended). */
export function toCurl(req: Request): string {
  let url = req.url;
  const query = Object.entries(req.query ?? {});
  if (query.length) {
    const existing = url.includes("?") ? url.slice(url.indexOf("?") + 1).split("&") : [];
    const extra = query
      .filter(([k]) => !existing.some((p) => p === k || p.startsWith(`${encodeURIComponent(k)}=`) || p.startsWith(`${k}=`)))
      .map(([k, v]) => `${encodeURIComponent(k)}=${encodeURIComponent(v)}`);
    if (extra.length) url += (url.includes("?") ? (url.endsWith("?") || url.endsWith("&") ? "" : "&") : "?") + extra.join("&");
  }
  const parts = ["curl"];
  if (req.method !== "GET") parts.push("-X", req.method);
  parts.push(shellQuote(url));
  for (const [k, v] of Object.entries(req.headers ?? {})) parts.push("-H", shellQuote(`${k}: ${v}`));
  const body = req.body ?? null;
  if (body?.type === "form") {
    for (const [k, v] of Object.entries(body.fields)) parts.push("--data-urlencode", shellQuote(`${k}=${v}`));
  } else if (body) {
    const hasType = Object.keys(req.headers ?? {}).some((k) => k.toLowerCase() === "content-type");
    if (body.type === "json" && !hasType) parts.push("-H", shellQuote("Content-Type: application/json"));
    parts.push("--data-raw", shellQuote(body.content));
  }
  return parts.join(" ");
}

// ---------- actions ----------

async function copy(text: string) {
  try {
    await navigator.clipboard.writeText(text);
  } catch (e) {
    ws.fail(e);
  }
}

export function copyUrl(entry: HistoryEntrySummary) {
  return copy(entry.url);
}

export async function copyCurl(entry: HistoryEntrySummary) {
  try {
    const full = await api.historyGet(entry.id);
    await copy(toCurl(full.resolved_request ?? full.request));
  } catch (e) {
    ws.fail(e);
  }
}

export async function deleteHistory(ids: number[]) {
  try {
    await history.remove(ids);
    ws.closeHistoryTabs(ids);
  } catch (e) {
    ws.fail(e);
  }
}

/** Ask, then delete every history entry. Returns true if cleared. */
export async function clearHistory(): Promise<boolean> {
  let ok = false;
  try {
    ok = await ask(t("history.clearConfirm"), { title: t("history.clearTitle"), kind: "warning" });
  } catch (e) {
    ws.fail(e);
  }
  if (!ok) return false;
  try {
    await history.clear();
    ws.closeHistoryTabs("all");
    return true;
  } catch (e) {
    ws.fail(e);
    return false;
  }
}
