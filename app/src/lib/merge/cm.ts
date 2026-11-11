// CodeMirror pieces of the merge resolver / side-by-side diff (D24): chunk decorations, the result pane's chunk
// tracking field and the theme. All colours come from theme CSS variables.
import { StateEffect, StateField, type Extension, type Range as CMRange, type Text } from "@codemirror/state";
import { Decoration, EditorView, type DecorationSet } from "@codemirror/view";
import { invertedEffects } from "@codemirror/commands";
import type { MergeChunkKind } from "./diff3";

/** Colour class of a chunk: background on its lines, coloured marker for empty ranges, ribbon fill. */
export type ChunkTone = "changed" | "added" | "removed" | "conflict" | "resolved";

/** Whole lines [from, to) (character offsets at line starts) painted with a tone; from === to marks a gap. */
export interface LineSpan {
  from: number;
  to: number;
  tone: ChunkTone;
}

/** Character range highlighted as a changed word. */
export interface WordSpan {
  from: number;
  to: number;
}

const wordMark = Decoration.mark({ class: "cm-merge-word" });
const lineDecos = new Map<string, Decoration>();
function lineDeco(cls: string): Decoration {
  let d = lineDecos.get(cls);
  if (!d) lineDecos.set(cls, (d = Decoration.line({ class: cls })));
  return d;
}

/** True when `pos` is past the last line (end of a document without a trailing newline). */
function afterLastLine(doc: Text, pos: number): boolean {
  return pos >= doc.length && doc.length > 0 && doc.sliceString(doc.length - 1) !== "\n";
}

export function buildDecorations(doc: Text, spans: readonly LineSpan[], words: readonly WordSpan[] = []): DecorationSet {
  const out: CMRange<Decoration>[] = [];
  for (const s of spans) {
    if (s.from >= s.to) {
      const pos = Math.min(s.from, doc.length);
      if (afterLastLine(doc, pos)) out.push(lineDeco(`cm-merge-gap-bottom cm-merge-gap-${s.tone}`).range(doc.lineAt(pos).from));
      else out.push(lineDeco(`cm-merge-gap-top cm-merge-gap-${s.tone}`).range(doc.lineAt(pos).from));
      continue;
    }
    const deco = lineDeco(`cm-merge-bg-${s.tone}`);
    const last = doc.lineAt(Math.min(s.to - 1, doc.length)).number;
    for (let n = doc.lineAt(s.from).number; n <= last; n++) out.push(deco.range(doc.line(n).from));
  }
  for (const w of words) if (w.to > w.from) out.push(wordMark.range(w.from, Math.min(w.to, doc.length)));
  return Decoration.set(out, true);
}

// ---------- Static panes (side panes, diff panes): decorations replaced wholesale ----------

export const setDecorations = StateEffect.define<DecorationSet>();

export const decorationField = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(decos, tr) {
    decos = decos.map(tr.changes);
    for (const e of tr.effects) if (e.is(setDecorations)) decos = e.value;
    return decos;
  },
  provide: (f) => EditorView.decorations.from(f),
});

// ---------- Result pane: chunk ranges tracked through every edit ----------

/** none = that side did not change the chunk. */
export type SideState = "none" | "pending" | "applied" | "ignored";

export interface ResultChunk {
  kind: MergeChunkKind;
  /** Character range in the result document (line starts). */
  from: number;
  to: number;
  ours: SideState;
  theirs: SideState;
}

export interface ChunkStateChange {
  index: number;
  ours: SideState;
  theirs: SideState;
}

export const setChunkStates = StateEffect.define<ChunkStateChange[]>();
/** Replace all chunks (positions refer to the document after the transaction's changes). */
export const resetChunks = StateEffect.define<ResultChunk[]>();

export function isResolved(c: ResultChunk): boolean {
  return c.ours !== "pending" && c.theirs !== "pending";
}

export const resultChunks = StateField.define<ResultChunk[]>({
  create: () => [],
  update(chunks, tr) {
    let out = chunks;
    if (!tr.changes.empty) {
      out = chunks.map((c) => {
        // Changed chunks grow to include text typed at their edges; unchanged ones shrink, so they never overlap.
        const eq = c.kind === "equal";
        const from = tr.changes.mapPos(c.from, eq ? 1 : -1);
        const to = Math.max(from, tr.changes.mapPos(c.to, eq ? -1 : 1));
        return from === c.from && to === c.to ? c : { ...c, from, to };
      });
    }
    for (const e of tr.effects) {
      if (e.is(resetChunks)) out = e.value;
      else if (e.is(setChunkStates)) {
        out = out.slice();
        for (const s of e.value) if (out[s.index]) out[s.index] = { ...out[s.index], ours: s.ours, theirs: s.theirs };
      }
    }
    return out;
  },
});

/** Undo/redo restore chunk states (and whole resets) together with the text. */
const invertChunkEffects = invertedEffects.of((tr) => {
  const out: StateEffect<unknown>[] = [];
  const before = tr.startState.field(resultChunks);
  for (const e of tr.effects) {
    if (e.is(setChunkStates)) {
      out.push(
        setChunkStates.of(e.value.map((s) => ({ index: s.index, ours: before[s.index].ours, theirs: before[s.index].theirs }))),
      );
    } else if (e.is(resetChunks)) out.push(resetChunks.of(before));
  }
  return out;
});

export function resultTone(c: ResultChunk): ChunkTone | null {
  if (c.kind === "equal") return null;
  if (isResolved(c)) return "resolved";
  return c.kind === "conflict" ? "conflict" : "changed";
}

const resultDecorations = EditorView.decorations.compute([resultChunks], (state) => {
  const spans: LineSpan[] = [];
  for (const c of state.field(resultChunks)) {
    const tone = resultTone(c);
    if (tone) spans.push({ from: c.from, to: c.to, tone });
  }
  return buildDecorations(state.doc, spans);
});

/** Extensions of the result pane, starting with `chunks`. */
export function resultExtensions(chunks: ResultChunk[]): Extension {
  return [resultChunks.init(() => chunks), resultDecorations, invertChunkEffects];
}

// ---------- Theme ----------

export const mergeTheme = EditorView.theme({
  ".cm-merge-bg-changed": { backgroundColor: "var(--merge-changed-bg)" },
  ".cm-merge-bg-added": { backgroundColor: "var(--merge-added-bg)" },
  ".cm-merge-bg-removed": { backgroundColor: "var(--merge-removed-bg)" },
  ".cm-merge-bg-conflict": { backgroundColor: "var(--merge-conflict-bg)" },
  ".cm-merge-bg-resolved": { backgroundColor: "var(--merge-resolved-bg)" },
  ".cm-merge-gap-top": { boxShadow: "inset 0 2px 0 0 var(--merge-gap)" },
  ".cm-merge-gap-bottom": { boxShadow: "inset 0 -2px 0 0 var(--merge-gap)" },
  ".cm-merge-gap-changed": { "--merge-gap": "var(--git-modified)" },
  ".cm-merge-gap-added": { "--merge-gap": "var(--git-added)" },
  ".cm-merge-gap-removed": { "--merge-gap": "var(--git-deleted)" },
  ".cm-merge-gap-conflict": { "--merge-gap": "var(--git-conflicted)" },
  ".cm-merge-gap-resolved": { "--merge-gap": "var(--border)" },
  ".cm-merge-word": { backgroundColor: "var(--merge-word-bg)", borderRadius: "2px" },
});
