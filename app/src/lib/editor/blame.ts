// Git annotate (blame) gutter for CodeEditor (D25), plus label helpers shared
// with non-CodeMirror annotations (URL field, key/value tables).
//
// The per-line array is mapped through document changes: any line touched by an
// edit becomes "uncommitted" (null). Gutter markers are only built for visible
// lines (CodeMirror gutters are viewport-based), and colours come from theme vars.

import { EditorState, Prec, StateEffect, StateField, type Extension, type Text, type Transaction } from "@codemirror/state";
import { EditorView, GutterMarker, gutter } from "@codemirror/view";
import type { BlameInfo } from "$lib/api";
import { t, i18n } from "$lib/i18n/index.svelte";

// ---------- labels (shared) ----------

/** "Jorge" from "Jorge Segovia" (max 12 chars). */
export function shortAuthor(author: string | null): string {
  const first = (author ?? "").trim().split(/\s+/)[0] ?? "";
  return first.length > 12 ? first.slice(0, 11) + "…" : first;
}

/** Compact relative age: "5 min", "3 h", "3 d", "2 mo", "1 y". */
export function shortAge(time: number, now = Date.now() / 1000): string {
  const s = Math.max(0, now - time);
  const n = (v: number) => ({ n: Math.max(1, Math.floor(v)) });
  if (s < 60) return t("blame.ago.now");
  if (s < 3600) return t("blame.ago.min", n(s / 60));
  if (s < 86400) return t("blame.ago.hour", n(s / 3600));
  if (s < 86400 * 30) return t("blame.ago.day", n(s / 86400));
  if (s < 86400 * 365) return t("blame.ago.month", n(s / (86400 * 30)));
  return t("blame.ago.year", n(s / (86400 * 365)));
}

export function isUncommitted(info: BlameInfo | null | undefined): boolean {
  return !info || !info.commit || info.time == null;
}

/** "Jorge · 3 d", or "—" for uncommitted lines. */
export function blameLabel(info: BlameInfo | null | undefined): string {
  if (!info || isUncommitted(info)) return "—";
  return `${shortAuthor(info.author)} · ${shortAge(info.time!)}`;
}

/** Multi-line tooltip: short id, author <email>, full date, summary. */
export function blameTooltip(info: BlameInfo | null | undefined): string {
  if (!info || isUncommitted(info)) return t("blame.uncommitted");
  const date = new Intl.DateTimeFormat(i18n.locale, { dateStyle: "medium", timeStyle: "short" }).format(
    new Date(info.time! * 1000),
  );
  const who = info.author_email ? `${info.author ?? ""} <${info.author_email}>` : (info.author ?? "");
  return [`${info.short_id ?? ""} · ${who}`, date, "", info.summary ?? "", "", t("blame.openHistory")].join("\n");
}

// ---------- state ----------

interface BlameData {
  /** One entry per document line; null = changed since the blame was loaded. */
  lines: (BlameInfo | null)[];
  /** Oldest / newest commit time in the file (for the heat colour). */
  min: number;
  max: number;
}

export const setBlame = StateEffect.define<BlameInfo[]>();

function build(infos: BlameInfo[], doc: Text): BlameData {
  let min = Infinity;
  let max = -Infinity;
  for (const i of infos) {
    if (i.time == null || !i.commit) continue;
    if (i.time < min) min = i.time;
    if (i.time > max) max = i.time;
  }
  const lines: (BlameInfo | null)[] = infos.slice(0, doc.lines);
  while (lines.length < doc.lines) lines.push(null);
  return { lines, min, max };
}

/** Map the per-line array through a change set; touched lines become null. */
function mapLines(data: BlameData, tr: Transaction): BlameData {
  const oldDoc = tr.startState.doc;
  const newDoc = tr.state.doc;
  const old = data.lines;
  const out: (BlameInfo | null)[] = [];
  let oldLine = 1; // next old line to copy
  tr.changes.iterChanges((fromA, toA, fromB, toB) => {
    const a1 = oldDoc.lineAt(fromA).number;
    const a2 = oldDoc.lineAt(toA).number;
    const b1 = newDoc.lineAt(fromB).number;
    const b2 = newDoc.lineAt(toB).number;
    while (oldLine < a1) out.push(old[oldLine++ - 1] ?? null);
    for (let l = b1; l <= b2; l++) out[l - 1] = null;
    oldLine = Math.max(oldLine, a2 + 1);
  });
  while (oldLine <= oldDoc.lines) out.push(old[oldLine++ - 1] ?? null);
  out.length = newDoc.lines;
  for (let i = 0; i < out.length; i++) if (out[i] === undefined) out[i] = null;
  return { lines: out, min: data.min, max: data.max };
}

function blameField(initial: BlameInfo[]) {
  return StateField.define<BlameData>({
    create: (state) => build(initial, state.doc),
    update(value, tr) {
      for (const e of tr.effects) if (e.is(setBlame)) return build(e.value, tr.state.doc);
      return tr.docChanged ? mapLines(value, tr) : value;
    },
  });
}

// ---------- gutter ----------

class BlameMarker extends GutterMarker {
  constructor(
    readonly label: string,
    readonly title: string,
    /** 0..100 share of --blame-recent, or -1 for uncommitted. */
    readonly heat: number,
    readonly first: boolean,
  ) {
    super();
  }
  eq(other: BlameMarker) {
    return other.label === this.label && other.title === this.title && other.heat === this.heat && other.first === this.first;
  }
  toDOM() {
    const el = document.createElement("div");
    el.className = "cm-blame" + (this.heat < 0 ? " cm-blame-uncommitted" : "") + (this.first ? " cm-blame-first" : "");
    if (this.heat >= 0)
      el.style.background = `color-mix(in srgb, var(--blame-recent) ${this.heat}%, var(--blame-old))`;
    el.textContent = this.label;
    el.title = this.title;
    return el;
  }
}

export interface BlameOptions {
  onclick?: (info: BlameInfo) => void;
}

/** Annotate gutter, placed before the line numbers. Update data with `setBlame`. */
export function blameExtension(initial: BlameInfo[], opts: BlameOptions = {}): Extension {
  const field = blameField(initial);
  const infoAt = (state: EditorState, lineNo: number) => state.field(field).lines[lineNo - 1] ?? null;
  return [
    field,
    Prec.high(
      gutter({
        class: "cm-blame-gutter",
        lineMarker(view, line) {
          const state = view.state;
          const data = state.field(field);
          const n = state.doc.lineAt(line.from).number;
          const info = data.lines[n - 1] ?? null;
          if (isUncommitted(info)) return new BlameMarker("—", t("blame.uncommitted"), -1, true);
          const prev = n > 1 ? data.lines[n - 2] : null;
          const first = !prev || prev.commit !== info!.commit;
          const span = data.max - data.min;
          const heat = span > 0 ? Math.round(((info!.time! - data.min) / span) * 100) : 100;
          return new BlameMarker(first ? blameLabel(info) : "", blameTooltip(info), heat, first);
        },
        lineMarkerChange: (u) =>
          u.startState.field(field, false) !== u.state.field(field, false) ||
          u.startState.facet(EditorState.phrases) !== u.state.facet(EditorState.phrases),
        domEventHandlers: {
          click(view, line) {
            const info = infoAt(view.state, view.state.doc.lineAt(line.from).number);
            if (info && !isUncommitted(info) && opts.onclick) {
              opts.onclick(info);
              return true;
            }
            return false;
          },
        },
      }),
    ),
    blameTheme,
  ];
}

const blameTheme = EditorView.theme({
  ".cm-blame-gutter": { width: "130px", cursor: "pointer" },
  ".cm-blame-gutter .cm-gutterElement": { padding: 0 },
  ".cm-blame": {
    height: "100%",
    boxSizing: "border-box",
    padding: "0 8px",
    color: "var(--blame-text)",
    fontSize: "11px",
    overflow: "hidden",
    textOverflow: "ellipsis",
    whiteSpace: "nowrap",
  },
  ".cm-blame-first": { borderTop: "1px solid var(--surface)" },
  ".cm-blame-uncommitted": { color: "var(--muted)", cursor: "default" },
  ".cm-blame-gutter .cm-gutterElement:hover .cm-blame:not(.cm-blame-uncommitted)": {
    color: "var(--text)",
    textDecoration: "underline",
  },
});
