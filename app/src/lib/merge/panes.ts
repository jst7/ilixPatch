// Geometry helpers shared by the merge resolver and the side-by-side diff: converting between line indexes
// (the `splitLines` model of diff3.ts) and editor positions, measuring chunks, and synchronised scrolling.
import type { Text } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";
import { mapLine, type Range } from "./diff3";
import type { ChunkTone } from "./cm";

/** Number of lines in the `splitLines` model (a trailing newline does not start a new line). */
export function modelLines(doc: Text): number {
  if (doc.length === 0) return 0;
  return doc.sliceString(doc.length - 1) === "\n" ? doc.lines - 1 : doc.lines;
}

/** Offset where model line `idx` starts (document length past the end). */
export function posOfLine(doc: Text, idx: number): number {
  return idx < doc.lines ? doc.line(idx + 1).from : doc.length;
}

export function lineRangeToPos(doc: Text, r: Range): [number, number] {
  return [posOfLine(doc, r[0]), posOfLine(doc, r[1])];
}

/** Model line index of the line containing `pos` (the model length when `pos` is past the last line). */
export function lineOfPos(doc: Text, pos: number): number {
  if (pos >= doc.length) return modelLines(doc);
  return doc.lineAt(pos).number - 1;
}

/** Character range of whole lines → model line range. */
export function posRangeToLines(doc: Text, from: number, to: number): Range {
  const start = lineOfPos(doc, from);
  return [start, to > from ? lineOfPos(doc, to - 1) + 1 : start];
}

/** Vertical position (document coordinates) of the top of model line `idx`. */
export function yOfLine(view: EditorView, idx: number): number {
  const doc = view.state.doc;
  if (idx >= doc.lines) return view.lineBlockAt(doc.length).bottom;
  return view.lineBlockAt(doc.line(idx + 1).from).top;
}

/** Offset from the scroller's scroll origin to the document top (panels, padding). */
function docOffset(view: EditorView): number {
  const el = view.scrollDOM;
  return view.documentTop - el.getBoundingClientRect().top + el.scrollTop;
}

/** Fractional model line at the top of the visible area. */
export function topLine(view: EditorView): number {
  const h = view.scrollDOM.scrollTop - docOffset(view);
  if (h <= 0) return 0;
  const block = view.lineBlockAtHeight(h);
  const idx = view.state.doc.lineAt(block.from).number - 1;
  return idx + Math.min(1, Math.max(0, (h - block.top) / Math.max(1, block.height)));
}

/** scrollTop that puts fractional model line `line` at the top. */
export function scrollTopFor(view: EditorView, line: number): number {
  const idx = Math.max(0, Math.floor(line));
  const top = yOfLine(view, idx);
  const frac = line - idx;
  const y = frac > 0 ? top + frac * (yOfLine(view, idx + 1) - top) : top;
  return docOffset(view) + y;
}

/** Scroll so that model line `line` sits a third of the way down the pane. */
export function revealLine(view: EditorView, line: number) {
  const el = view.scrollDOM;
  el.scrollTop = Math.max(0, scrollTopFor(view, line) - el.clientHeight / 3);
}

/**
 * Keeps several editors scrolled to corresponding lines. `ranges(pane)` returns, per aligned chunk, its model
 * line range in that pane (the chunk lists of all panes have the same length and order).
 */
export class ScrollSync {
  private expected = new Map<HTMLElement, number>();
  private cleanup: (() => void)[] = [];

  constructor(
    private views: EditorView[],
    private ranges: (pane: number) => Range[],
    private onscroll: () => void,
  ) {
    views.forEach((view, i) => {
      const el = view.scrollDOM;
      const handler = () => this.scrolled(i);
      el.addEventListener("scroll", handler, { passive: true });
      this.cleanup.push(() => el.removeEventListener("scroll", handler));
    });
  }

  private scrolled(i: number) {
    const el = this.views[i].scrollDOM;
    const exp = this.expected.get(el);
    this.expected.delete(el);
    // Echo of a scroll we caused: don't propagate it back.
    if (exp !== undefined && Math.abs(el.scrollTop - exp) < 2) {
      this.onscroll();
      return;
    }
    this.syncFrom(i);
    this.onscroll();
  }

  /** Align every other pane to pane `i`. */
  syncFrom(i: number) {
    const src = this.views[i];
    const line = topLine(src);
    const fromRanges = this.ranges(i);
    this.views.forEach((view, j) => {
      if (j === i) return;
      const toRanges = this.ranges(j);
      const segments = fromRanges.map((from, k) => ({ from, to: toRanges[k] }));
      const target = mapLine(segments, line);
      const el = view.scrollDOM;
      const max = Math.max(0, el.scrollHeight - el.clientHeight);
      const top = Math.round(Math.min(max, Math.max(0, scrollTopFor(view, target))));
      if (Math.abs(top - el.scrollTop) < 1) return;
      this.expected.set(el, top);
      el.scrollTop = top;
    });
  }

  destroy() {
    for (const c of this.cleanup) c();
    this.cleanup = [];
  }
}

// ---------- Ribbons between two panes ----------

/** A band connecting a chunk in the left pane (lt..lb) to one in the right pane (rt..rb), in strip pixels. */
export interface Ribbon {
  key: number;
  lt: number;
  lb: number;
  rt: number;
  rb: number;
  tone: ChunkTone;
}

/** A small button drawn in a strip at height `y`, against its left or right edge. */
export interface StripAction {
  key: string;
  y: number;
  side: "left" | "right";
  label: string;
  title: string;
  kind: "apply" | "ignore";
  run: () => void;
}

/** Vertical extent of a model line range in client coordinates relative to `originTop`. */
export function bandOf(view: EditorView, r: Range, originTop: number): [number, number] {
  const base = view.documentTop - originTop;
  return [base + yOfLine(view, r[0]), base + yOfLine(view, r[1])];
}
