// Response comparisons (D64): each "compare" tab has a left and a right side (label + text).
// Kept in memory only: a comparison is a quick check, not something to save.
import { api, type SendResult } from "$lib/api";
import { ws } from "./workspace.svelte";
import { t } from "$lib/i18n/index.svelte";

export interface Side {
  label: string;
  text: string;
}

export interface Comparison {
  left: Side;
  right: Side;
  /** The right side is pasted by the user. */
  pasted: boolean;
}

const CHUNK = 1024 * 1024;
const MAX_BODY = 20 * 1024 * 1024;

/** The whole body of a send result (reads the Rust-side handle when the inline text is a preview). */
export async function bodyOf(r: SendResult): Promise<string> {
  const h = r.body_handle;
  if (!h || h.size <= r.body.length || !h.is_text || h.size > MAX_BODY) return r.body;
  let out = "";
  for (let offset = 0; offset < h.size; ) {
    const c = await api.responseRead(h.id, offset, CHUNK);
    out += c.data;
    if (c.eof || c.length === 0) break;
    offset += c.length;
  }
  return out;
}

class CompareStore {
  sessions = $state<Record<string, Comparison>>({});

  open(left: Side, right: Side | null) {
    const id = crypto.randomUUID();
    this.sessions[id] = { left, right: right ?? { label: t("compare.pasted"), text: "" }, pasted: !right };
    ws.openTab({ kind: "compare", id });
  }

  swap(id: string) {
    const s = this.sessions[id];
    if (s && !s.pasted) [s.left, s.right] = [s.right, s.left];
  }
}

export const compares = new CompareStore();

const time = (ms: number) => new Date(ms).toLocaleString();

/** Menu of what to compare the response of `path` with: examples, earlier responses, pasted JSON. */
export async function compareMenu(path: string, x: number, y: number) {
  const result = ws.responses[path]?.result;
  const draft = ws.drafts[path];
  if (!result) return;
  const left: Side = { label: t("compare.current", { status: result.status }), text: await bodyOf(result) };
  const items: { label: string; action: () => void }[] = [];
  for (const e of draft?.examples ?? []) {
    items.push({
      label: t("compare.withExample", { name: e.name, status: e.status }),
      action: () => compares.open(left, { label: t("compare.example", { name: e.name }), text: e.body ?? "" }),
    });
  }
  // Earlier responses of this request, from the local history.
  const filePath = ws.index.get(path)?.file.path ?? null;
  if (filePath && draft) {
    try {
      const list = (await api.historyList(draft.name || null, null, 60)).filter((h) => h.request_path === filePath && h.status !== null);
      for (const h of list.slice(1, 11)) {
        items.push({
          label: t("compare.withHistory", { time: time(h.timestamp), status: h.status ?? "" }),
          action: () =>
            void api.historyGet(h.id).then((entry) =>
              compares.open(left, { label: t("compare.history", { time: time(h.timestamp) }), text: entry.response?.body ?? "" }),
            ),
        });
      }
    } catch {
      // history disabled or unavailable: just no entries
    }
  }
  items.push({ label: t("compare.withPasted"), action: () => compares.open(left, null) });
  ws.menu = { x, y, items };
}
