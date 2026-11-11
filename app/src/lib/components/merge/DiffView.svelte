<script lang="ts">
  // Side-by-side, read-only two-way line comparator (D24): aligned chunks with ribbons, line and word
  // highlights, synchronised scrolling and previous/next change navigation. `null` = the file doesn't exist.
  import { onDestroy } from "svelte";
  import { Text, type Extension } from "@codemirror/state";
  import { EditorView } from "@codemirror/view";
  import { t } from "$lib/i18n/index.svelte";
  import type { EditorLanguage } from "$lib/editor/setup";
  import CodeEditor from "$lib/components/CodeEditor.svelte";
  import Strip from "./Strip.svelte";
  import { joinLines, splitLines, twoWayChunks, wordDiff, type DiffChunk, type Range } from "$lib/merge/diff3";
  import { buildDecorations, decorationField, mergeTheme, setDecorations, type ChunkTone, type LineSpan, type WordSpan } from "$lib/merge/cm";
  import { ScrollSync, bandOf, lineRangeToPos, revealLine, topLine, type Ribbon } from "$lib/merge/panes";

  interface Props {
    left: string | null;
    right: string | null;
    leftTitle: string;
    rightTitle: string;
    language?: EditorLanguage;
  }

  let { left, right, leftTitle, rightTitle, language = "text" }: Props = $props();

  const MAX_WORD_DIFF_LINES = 200;

  interface Model {
    leftText: string;
    rightText: string;
    chunks: DiffChunk[];
    leftSpans: LineSpan[];
    rightSpans: LineSpan[];
    leftWords: WordSpan[];
    rightWords: WordSpan[];
    /** Indexes of the non-equal chunks. */
    changes: number[];
  }

  function tone(c: DiffChunk): ChunkTone {
    return c.kind === "added" ? "added" : c.kind === "removed" ? "removed" : "changed";
  }

  function build(l: string | null, r: string | null): Model {
    const leftText = (l ?? "").replace(/\r\n?/g, "\n");
    const rightText = (r ?? "").replace(/\r\n?/g, "\n");
    const L = splitLines(leftText);
    const R = splitLines(rightText);
    const chunks = twoWayChunks(L, R);
    const ld = Text.of(leftText.split("\n"));
    const rd = Text.of(rightText.split("\n"));
    const m: Model = { leftText, rightText, chunks, leftSpans: [], rightSpans: [], leftWords: [], rightWords: [], changes: [] };
    chunks.forEach((c, i) => {
      if (c.kind === "equal") return;
      m.changes.push(i);
      const [lf, lt] = lineRangeToPos(ld, c.left);
      const [rf, rt] = lineRangeToPos(rd, c.right);
      m.leftSpans.push({ from: lf, to: lt, tone: tone(c) });
      m.rightSpans.push({ from: rf, to: rt, tone: tone(c) });
      const n = c.left[1] - c.left[0] + c.right[1] - c.right[0];
      if (c.kind === "changed" && n <= MAX_WORD_DIFF_LINES) {
        const w = wordDiff(joinLines(L, c.left), joinLines(R, c.right));
        for (const [a, b] of w.a) m.leftWords.push({ from: lf + a, to: lf + b });
        for (const [a, b] of w.b) m.rightWords.push({ from: rf + a, to: rf + b });
      }
    });
    return m;
  }

  const model = $derived(build(left, right));

  let leftView: EditorView | null = null;
  let rightView: EditorView | null = null;
  let strip = $state<HTMLDivElement>();
  let ribbons = $state.raw<Ribbon[]>([]);
  let sync: ScrollSync | null = null;
  let raf = 0;

  const geometryListener = EditorView.updateListener.of((u) => {
    if (u.geometryChanged || u.viewportChanged) schedule();
  });

  function extensions(): Extension[] {
    return [decorationField, mergeTheme, geometryListener];
  }

  function ranges(m: Model, pane: number): Range[] {
    return m.chunks.map((c) => (pane === 0 ? c.left : c.right));
  }

  function onView(which: 0 | 1, view: EditorView | null, m: Model) {
    if (!view) {
      // A destroyed view (re-keyed model): drop it and the scroll sync built on it.
      sync?.destroy();
      sync = null;
      if (which === 0) leftView = null;
      else rightView = null;
      return;
    }
    if (which === 0) leftView = view;
    else rightView = view;
    const spans = which === 0 ? m.leftSpans : m.rightSpans;
    const words = which === 0 ? m.leftWords : m.rightWords;
    view.dispatch({ effects: setDecorations.of(buildDecorations(view.state.doc, spans, words)) });
    if (leftView && rightView && !sync) {
      sync = new ScrollSync([leftView, rightView], (p) => ranges(m, p), schedule);
      schedule();
    }
  }

  function schedule() {
    if (!raf) raf = requestAnimationFrame(layout);
  }

  function layout() {
    raf = 0;
    if (!leftView || !rightView || !strip) return;
    const origin = strip.getBoundingClientRect().top;
    const h = strip.clientHeight;
    const out: Ribbon[] = [];
    for (const i of model.changes) {
      const c = model.chunks[i];
      const [lt, lb] = bandOf(leftView, c.left, origin);
      const [rt, rb] = bandOf(rightView, c.right, origin);
      if (Math.max(lb, rb) < -4 || Math.min(lt, rt) > h + 4) continue;
      out.push({ key: i, lt, lb, rt, rb, tone: tone(c) });
    }
    ribbons = out;
  }

  /** Jump to the next (+1) or previous (-1) change after/before the top of the right pane. */
  function go(dir: 1 | -1) {
    if (!rightView || model.changes.length === 0) return;
    const top = topLine(rightView);
    const starts = model.changes.map((i) => model.chunks[i].right[0]);
    let k: number;
    if (dir > 0) {
      k = starts.findIndex((s) => s > top + 0.5);
      if (k < 0) k = 0;
    } else {
      k = -1;
      for (let j = starts.length - 1; j >= 0; j--) {
        if (starts[j] < top - 0.5) {
          k = j;
          break;
        }
      }
      if (k < 0) k = starts.length - 1;
    }
    revealLine(rightView, starts[k]);
    current = k;
  }

  let current = $state(-1);

  onDestroy(() => {
    sync?.destroy();
    if (raf) cancelAnimationFrame(raf);
  });
</script>

<div class="diff">
  <div class="bar">
    <span class="count">
      {model.changes.length === 0 ? t("diff.identical") : t("diff.changes", { count: model.changes.length })}
    </span>
    <button class="ghost nav" disabled={model.changes.length === 0} title={t("diff.prev")} aria-label={t("diff.prev")} onclick={() => go(-1)}
      >↑</button
    >
    <button class="ghost nav" disabled={model.changes.length === 0} title={t("diff.next")} aria-label={t("diff.next")} onclick={() => go(1)}
      >↓</button
    >
    {#if current >= 0 && model.changes.length > 0}<span class="muted">{current + 1} / {model.changes.length}</span>{/if}
  </div>
  <div class="grid">
    <div class="head" title={leftTitle}>{leftTitle}{#if left === null}&nbsp;<span class="muted">{t("merge.missing")}</span>{/if}</div>
    <div class="gap"></div>
    <div class="head" title={rightTitle}>{rightTitle}{#if right === null}&nbsp;<span class="muted">{t("merge.missing")}</span>{/if}</div>
    {#key model}
      {@const m = model}
      <div class="pane">
        <CodeEditor value={m.leftText} readonly {language} extensions={extensions()} onview={(v) => onView(0, v, m)} />
      </div>
      <Strip bind:el={strip} {ribbons} />
      <div class="pane">
        <CodeEditor value={m.rightText} readonly {language} extensions={extensions()} onview={(v) => onView(1, v, m)} />
      </div>
    {/key}
  </div>
</div>

<style>
  .diff {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    min-width: 0;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 8px;
    font-size: 12px;
    border-bottom: 1px solid var(--border);
    background: var(--surface);
  }
  .bar button.nav {
    padding: 2px 6px;
    font-size: 12px;
  }
  .count,
  .muted {
    color: var(--muted);
  }
  .grid {
    flex: 1 1 auto;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr) 40px minmax(0, 1fr);
    grid-template-rows: auto minmax(0, 1fr);
  }
  .head,
  .gap {
    padding: 4px 10px;
    font-size: 12px;
    color: var(--text);
    background: var(--surface-2);
    border-bottom: 1px solid var(--border);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pane {
    min-width: 0;
    min-height: 0;
    overflow: hidden;
  }
</style>
