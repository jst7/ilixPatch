<script lang="ts">
  // IntelliJ-style 3-way merge resolver (D24): yours | result (editable) | theirs, with chunk ribbons,
  // apply/ignore actions, synchronised scrolling and whole-file resolution.
  import { onDestroy } from "svelte";
  import { ask } from "@tauri-apps/plugin-dialog";
  import { Text, type ChangeSpec, type Extension } from "@codemirror/state";
  import { EditorView } from "@codemirror/view";
  import { isolateHistory } from "@codemirror/commands";
  import { api, type GitConflictVersions, type GitMergeState } from "$lib/api";
  import { t } from "$lib/i18n/index.svelte";
  import { ws, tabKey } from "$lib/stores/workspace.svelte";
  import { git, gitError } from "$lib/stores/git.svelte";
  import type { EditorLanguage } from "$lib/editor/setup";
  import CodeEditor from "$lib/components/CodeEditor.svelte";
  import Strip from "./Strip.svelte";
  import {
    buildResult,
    diff3Chunks,
    joinLines,
    splitLines,
    wordDiff,
    type MergeChunk,
    type Range,
  } from "$lib/merge/diff3";
  import {
    buildDecorations,
    decorationField,
    isResolved,
    mergeTheme,
    resetChunks,
    resultChunks,
    resultExtensions,
    setChunkStates,
    setDecorations,
    type ChunkStateChange,
    type ChunkTone,
    type LineSpan,
    type ResultChunk,
    type SideState,
    type WordSpan,
  } from "$lib/merge/cm";
  import {
    ScrollSync,
    bandOf,
    lineRangeToPos,
    posRangeToLines,
    revealLine,
    type Ribbon,
    type StripAction,
  } from "$lib/merge/panes";

  let { repo, path }: { repo: string; path: string } = $props();

  type Side = "ours" | "theirs";
  type Phase = "loading" | "error" | "editor" | "special" | "status";

  /** Chunks this large get no word-level highlights (keeps opening big files fast). */
  const MAX_WORD_DIFF_LINES = 200;
  const AUTO_KEY = "ilixpatch.merge.autoApply";

  let phase = $state<Phase>("loading");
  let error = $state<string | null>(null);
  let actionError = $state<string | null>(null);
  let versions = $state.raw<GitConflictVersions | null>(null);
  let mergeState = $state.raw<GitMergeState | null>(null);
  let busy = $state(false);
  let statusNote = $state<string | null>(null);
  let autoApply = $state(readAutoApply());
  let counts = $state({ conflicts: 0, changes: 0 });

  // ----- model (fixed once loaded) -----
  let B: string[] = [];
  let O: string[] = [];
  let T: string[] = [];
  let eol = "\n";
  let chunks: MergeChunk[] = [];
  let oursText = $state.raw("");
  let theirsText = $state.raw("");
  let initialResult = $state.raw("");
  let initialChunks: ResultChunk[] = [];
  let oursPos: [number, number][] = [];
  let theirsPos: [number, number][] = [];
  let oursWords: WordSpan[][] = [];
  let theirsWords: WordSpan[][] = [];
  let oursRanges: Range[] = [];
  let theirsRanges: Range[] = [];

  // ----- views & layout -----
  let oursView: EditorView | null = null;
  let resultView: EditorView | null = null;
  let theirsView: EditorView | null = null;
  let leftStrip = $state<HTMLDivElement>();
  let rightStrip = $state<HTMLDivElement>();
  let leftRibbons = $state.raw<Ribbon[]>([]);
  let rightRibbons = $state.raw<Ribbon[]>([]);
  let leftActions = $state.raw<StripAction[]>([]);
  let rightActions = $state.raw<StripAction[]>([]);
  let sync: ScrollSync | null = null;
  let raf = 0;
  let sidesDirty = true;
  /** The user changed the result since it was built (asks before discarding). */
  let touched = false;

  const fileName = $derived(basename(path));
  const node = $derived(ws.nodes.get(path));
  const title = $derived(node?.kind === "request" ? node.name : fileName);
  const relPath = $derived(path.startsWith(repo) ? path.slice(repo.length).replace(/^[\\/]/, "") : path);
  const language = $derived(languageOf(path));
  const oursLabel = $derived(mergeState?.ours_label ?? "");
  const theirsLabel = $derived(mergeState?.theirs_label ?? "");

  function basename(p: string): string {
    return p.split(/[\\/]/).pop() || p;
  }

  function languageOf(p: string): EditorLanguage {
    const ext = p.toLowerCase().split(".").pop() ?? "";
    if (ext === "json") return "json";
    if (["js", "mjs", "cjs", "ts"].includes(ext)) return "javascript";
    return "text";
  }

  function readAutoApply(): boolean {
    try {
      return localStorage.getItem(AUTO_KEY) !== "false";
    } catch {
      return true;
    }
  }

  function setAutoApply(v: boolean) {
    autoApply = v;
    try {
      localStorage.setItem(AUTO_KEY, String(v));
    } catch {}
  }

  /** Same line splitting as CodeMirror, for computing positions before the views exist. */
  function toText(s: string): Text {
    return Text.of(s.split("\n"));
  }

  // ----- loading -----
  async function load() {
    phase = "loading";
    error = null;
    try {
      const st = await api.gitMergeState(repo);
      mergeState = st;
      if (!st.conflicted.includes(path)) {
        statusNote = t("merge.notConflicted");
        phase = "status";
        return;
      }
      const v = await api.gitConflictVersions(repo, path);
      versions = v;
      if (v.binary || v.ours === null || v.theirs === null) {
        phase = "special";
        return;
      }
      prepare(v);
      phase = "editor";
    } catch (e) {
      error = gitError(e);
      phase = "error";
    }
  }
  void load();

  function prepare(v: GitConflictVersions) {
    const norm = (s: string | null) => (s ?? "").replace(/\r\n?/g, "\n");
    eol = /\r\n/.test(v.ours ?? "") || /\r\n/.test(v.theirs ?? "") ? "\r\n" : "\n";
    oursText = norm(v.ours);
    theirsText = norm(v.theirs);
    const baseText = norm(v.base);
    B = splitLines(baseText);
    O = splitLines(oursText);
    T = splitLines(theirsText);
    chunks = diff3Chunks(B, O, T);
    oursRanges = chunks.map((c) => c.ours);
    theirsRanges = chunks.map((c) => c.theirs);
    const od = toText(oursText);
    const td = toText(theirsText);
    oursPos = oursRanges.map((r) => lineRangeToPos(od, r));
    theirsPos = theirsRanges.map((r) => lineRangeToPos(td, r));
    oursWords = chunks.map((c, i) => words(c, c.ours, O, oursPos[i][0]));
    theirsWords = chunks.map((c, i) => words(c, c.theirs, T, theirsPos[i][0]));
    buildInitial();
  }

  /** Word-level changes of a side's chunk against the base. */
  function words(c: MergeChunk, r: Range, lines: string[], offset: number): WordSpan[] {
    const baseLen = c.base[1] - c.base[0];
    const len = r[1] - r[0];
    if (c.kind === "equal" || baseLen === 0 || len === 0 || baseLen + len > MAX_WORD_DIFF_LINES) return [];
    const d = wordDiff(joinLines(B, c.base), joinLines(lines, r));
    return d.b.map(([from, to]) => ({ from: offset + from, to: offset + to }));
  }

  function buildInitial() {
    const r = buildResult(B, O, T, chunks, autoApply);
    initialResult = r.text;
    const doc = toText(r.text);
    initialChunks = chunks.map((c, i) => {
      const [from, to] = lineRangeToPos(doc, r.ranges[i]);
      const state = (changed: boolean): SideState => (!changed ? "none" : r.applied[i] ? "applied" : "pending");
      return {
        kind: c.kind,
        from,
        to,
        ours: state(c.kind === "ours" || c.kind === "both" || c.kind === "conflict"),
        theirs: state(c.kind === "theirs" || c.kind === "both" || c.kind === "conflict"),
      };
    });
    updateCounts(initialChunks);
  }

  function updateCounts(rc: readonly ResultChunk[]) {
    let conflicts = 0;
    let changes = 0;
    for (const c of rc) {
      if (c.kind === "equal" || isResolved(c)) continue;
      if (c.kind === "conflict") conflicts++;
      else changes++;
    }
    if (conflicts !== counts.conflicts || changes !== counts.changes) counts = { conflicts, changes };
  }

  // ----- editor extensions -----
  const geometryListener = EditorView.updateListener.of((u) => {
    if (u.geometryChanged || u.viewportChanged) schedule();
  });

  const resultListener = EditorView.updateListener.of((u) => {
    const before = u.startState.field(resultChunks);
    const after = u.state.field(resultChunks);
    if (u.docChanged || before !== after) touched = true;
    if (before !== after) {
      if (before.length !== after.length || before.some((c, i) => c.ours !== after[i].ours || c.theirs !== after[i].theirs)) {
        sidesDirty = true;
      }
      updateCounts(after);
    }
    if (u.docChanged || u.geometryChanged || u.viewportChanged || before !== after) schedule();
  });

  function sideExtensions(): Extension[] {
    return [decorationField, mergeTheme, geometryListener];
  }

  function resultExts(): Extension[] {
    return [resultExtensions(initialChunks), mergeTheme, resultListener];
  }

  function onView(which: "ours" | "result" | "theirs", view: EditorView | null) {
    if (which === "ours") oursView = view;
    else if (which === "result") resultView = view;
    else theirsView = view;
    if (!view) {
      sync?.destroy();
      sync = null;
      return;
    }
    if (oursView && resultView && theirsView && !sync) {
      sync = new ScrollSync([oursView, resultView, theirsView], paneRanges, schedule);
      sidesDirty = true;
      schedule();
      requestAnimationFrame(() => navigate(1, true));
    }
  }

  function paneRanges(pane: number): Range[] {
    if (pane === 0) return oursRanges;
    if (pane === 2) return theirsRanges;
    if (!resultView) return oursRanges;
    const doc = resultView.state.doc;
    return resultView.state.field(resultChunks).map((c) => posRangeToLines(doc, c.from, c.to));
  }

  onDestroy(() => {
    sync?.destroy();
    if (raf) cancelAnimationFrame(raf);
  });

  // ----- decorations & ribbons -----
  function sideTone(c: MergeChunk, side: Side, state: SideState): ChunkTone {
    if (state === "applied" || state === "ignored") return "resolved";
    if (c.kind === "conflict") return "conflict";
    const r = side === "ours" ? c.ours : c.theirs;
    if (c.base[0] === c.base[1]) return "added";
    if (r[0] === r[1]) return "removed";
    return "changed";
  }

  function paintSide(view: EditorView, side: Side, rc: readonly ResultChunk[]) {
    const spans: LineSpan[] = [];
    const ws_: WordSpan[] = [];
    const pos = side === "ours" ? oursPos : theirsPos;
    const words = side === "ours" ? oursWords : theirsWords;
    chunks.forEach((c, i) => {
      const st = rc[i]?.[side] ?? "none";
      if (st === "none") return;
      spans.push({ from: pos[i][0], to: pos[i][1], tone: sideTone(c, side, st) });
      if (st === "pending") ws_.push(...words[i]);
    });
    view.dispatch({ effects: setDecorations.of(buildDecorations(view.state.doc, spans, ws_)) });
  }

  function schedule() {
    if (!raf) raf = requestAnimationFrame(layout);
  }

  /** Button position for a chunk band starting at `top`, ending at `bottom`; null when not visible. */
  function buttonY(top: number, bottom: number, height: number): number | null {
    let y = top + 1;
    if (y < 2) y = Math.min(2, bottom - 17);
    return y < -16 || y > height - 4 ? null : y;
  }

  function layout() {
    raf = 0;
    if (!oursView || !resultView || !theirsView) return;
    const rc = resultView.state.field(resultChunks);
    if (sidesDirty) {
      sidesDirty = false;
      paintSide(oursView, "ours", rc);
      paintSide(theirsView, "theirs", rc);
    }
    const rdoc = resultView.state.doc;
    const resLines = rc.map((c) => posRangeToLines(rdoc, c.from, c.to));
    const strips: [HTMLDivElement | undefined, Side][] = [
      [leftStrip, "ours"],
      [rightStrip, "theirs"],
    ];
    for (const [strip, side] of strips) {
      const ribbons: Ribbon[] = [];
      const actions: StripAction[] = [];
      if (strip) {
        const origin = strip.getBoundingClientRect().top;
        const h = strip.clientHeight;
        const sideView = side === "ours" ? oursView : theirsView;
        const sideRanges = side === "ours" ? oursRanges : theirsRanges;
        for (let i = 0; i < chunks.length; i++) {
          const st = rc[i]?.[side] ?? "none";
          if (st === "none") continue;
          const [st0, st1] = bandOf(sideView, sideRanges[i], origin);
          const [rs0, rs1] = bandOf(resultView, resLines[i], origin);
          if (Math.max(st1, rs1) < -4 || Math.min(st0, rs0) > h + 4) continue;
          const tone = sideTone(chunks[i], side, st);
          ribbons.push(
            side === "ours"
              ? { key: i, lt: st0, lb: st1, rt: rs0, rb: rs1, tone }
              : { key: i, lt: rs0, lb: rs1, rt: st0, rb: st1, tone },
          );
          if (st !== "pending") continue;
          const y = buttonY(st0, st1, h);
          if (y === null) continue;
          const other: Side = side === "ours" ? "theirs" : "ours";
          const append = rc[i].kind === "conflict" && rc[i][other] === "applied";
          const edge = side === "ours" ? "left" : "right";
          actions.push({
            key: `a${i}`,
            y,
            side: edge,
            kind: "apply",
            label: side === "ours" ? "»" : "«",
            title:
              side === "ours"
                ? t(append ? "merge.appendOurs" : "merge.applyOurs")
                : t(append ? "merge.appendTheirs" : "merge.applyTheirs"),
            run: () => applySide(i, side),
          });
          actions.push({
            key: `i${i}`,
            y,
            side: edge,
            kind: "ignore",
            label: "×",
            title: t(side === "ours" ? "merge.ignoreOurs" : "merge.ignoreTheirs"),
            run: () => ignoreSide(i, side),
          });
        }
      }
      if (side === "ours") {
        leftRibbons = ribbons;
        leftActions = actions;
      } else {
        rightRibbons = ribbons;
        rightActions = actions;
      }
    }
  }

  // ----- chunk actions -----
  function sideText(i: number, side: Side): string {
    return side === "ours" ? joinLines(O, chunks[i].ours) : joinLines(T, chunks[i].theirs);
  }

  function nextState(c: ResultChunk, i: number, side: Side, value: SideState): ChunkStateChange {
    const s: ChunkStateChange = { index: i, ours: c.ours, theirs: c.theirs };
    s[side] = value;
    // An identical change on both sides is one decision.
    if (c.kind === "both") s.ours = s.theirs = value;
    return s;
  }

  /** Change that puts side `side` of chunk `c` into the result (appending after the other side if applied). */
  function applyChange(c: ResultChunk, i: number, side: Side, doc: Text) {
    const other: Side = side === "ours" ? "theirs" : "ours";
    let text = sideText(i, side);
    if (c.kind === "conflict" && c[other] === "applied") {
      const cur = doc.sliceString(c.from, c.to);
      const sep = cur.length > 0 && !cur.endsWith("\n") ? "\n" : "";
      if (text && !text.endsWith("\n") && c.to < doc.length) text += "\n";
      return { from: c.to, insert: sep + text };
    }
    if (text && !text.endsWith("\n") && c.to < doc.length) text += "\n";
    return { from: c.from, to: c.to, insert: text };
  }

  function applySide(i: number, side: Side) {
    const view = resultView;
    if (!view) return;
    const c = view.state.field(resultChunks)[i];
    if (!c || c[side] !== "pending") return;
    view.dispatch({
      changes: applyChange(c, i, side, view.state.doc),
      effects: setChunkStates.of([nextState(c, i, side, "applied")]),
      annotations: isolateHistory.of("full"),
      userEvent: "input.merge",
    });
  }

  function ignoreSide(i: number, side: Side) {
    const view = resultView;
    if (!view) return;
    const c = view.state.field(resultChunks)[i];
    if (!c || c[side] !== "pending") return;
    view.dispatch({
      effects: setChunkStates.of([nextState(c, i, side, "ignored")]),
      annotations: isolateHistory.of("full"),
      userEvent: "input.merge",
    });
  }

  /** Apply every pending non-conflicting change from one side (or both) in one undoable step. */
  function applyNonConflicting(which: Side | "all") {
    const view = resultView;
    if (!view) return;
    const rc = view.state.field(resultChunks);
    const changes: ChangeSpec[] = [];
    const states: ChunkStateChange[] = [];
    rc.forEach((c, i) => {
      if (c.kind === "equal" || c.kind === "conflict" || isResolved(c)) return;
      const side: Side | null =
        c.kind === "both" ? (which === "theirs" ? "theirs" : "ours") : c.kind === "ours" ? "ours" : "theirs";
      if (which !== "all" && c.kind !== "both" && side !== which) return;
      changes.push(applyChange(c, i, side, view.state.doc));
      states.push(nextState(c, i, side, "applied"));
    });
    if (states.length === 0) return;
    view.dispatch({
      changes,
      effects: setChunkStates.of(states),
      annotations: isolateHistory.of("full"),
      userEvent: "input.merge",
    });
  }

  const pendingNonConflicting = $derived(counts.changes > 0);

  /** Move to the next/previous unresolved chunk of the result (F7 / Shift+F7). */
  function navigate(dir: 1 | -1, fromStart = false) {
    const view = resultView;
    if (!view) return;
    const rc = view.state.field(resultChunks);
    const list = rc.map((_, i) => i).filter((i) => rc[i].kind !== "equal" && !isResolved(rc[i]));
    if (list.length === 0) return;
    const head = view.state.selection.main.head;
    let target: number;
    if (fromStart) target = list.find((i) => rc[i].kind === "conflict") ?? list[0];
    else if (dir > 0) target = list.find((i) => rc[i].from > head) ?? list[0];
    else target = [...list].reverse().find((i) => rc[i].from < head) ?? list[list.length - 1];
    const c = rc[target];
    view.dispatch({ selection: { anchor: c.from } });
    revealLine(view, posRangeToLines(view.state.doc, c.from, c.to)[0]);
    if (!fromStart) view.focus();
  }

  function onKeydown(e: KeyboardEvent) {
    if (phase !== "editor" || e.key !== "F7") return;
    e.preventDefault();
    navigate(e.shiftKey ? -1 : 1);
  }

  async function confirm(message: string, title: string, okLabel: string): Promise<boolean> {
    try {
      return await ask(message, { title, kind: "warning", okLabel, cancelLabel: t("merge.cancel") });
    } catch (e) {
      actionError = gitError(e);
      return false;
    }
  }

  async function reset() {
    const view = resultView;
    if (!view) return;
    if (touched && !(await confirm(t("merge.resetConfirm"), t("merge.reset"), t("merge.reset")))) return;
    buildInitial();
    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: initialResult },
      effects: resetChunks.of(initialChunks),
      annotations: isolateHistory.of("full"),
    });
    touched = false;
    sidesDirty = true;
    schedule();
  }

  // ----- resolving -----
  const myKey = $derived(tabKey({ kind: "merge", repo, path }));

  async function run(fn: () => Promise<void>) {
    if (busy) return;
    busy = true;
    actionError = null;
    try {
      await fn();
    } catch (e) {
      actionError = gitError(e);
    } finally {
      busy = false;
    }
  }

  function close() {
    void ws.closeTab(myKey);
  }

  async function afterResolve(st: GitMergeState) {
    mergeState = st;
    void git.afterChange(repo);
    if (st.conflicted.length > 0 && !st.conflicted.includes(path)) {
      close();
      return;
    }
    statusNote = st.conflicted.includes(path) ? null : t("merge.resolved");
    phase = "status";
  }

  function save() {
    void run(async () => {
      const view = resultView;
      if (!view) return;
      const rc = view.state.field(resultChunks);
      const unresolved = rc.filter((c) => c.kind !== "equal" && !isResolved(c)).length;
      const content = view.state.doc.sliceString(0, view.state.doc.length, eol);
      if (unresolved > 0) {
        const ok = await confirm(t("merge.unresolvedConfirm", { count: unresolved }), t("merge.unresolvedTitle"), t("merge.saveAnyway"));
        if (!ok) return;
      }
      if (/^(<{7}|>{7})(\s|$)/m.test(content)) {
        const ok = await confirm(t("merge.markersConfirm"), t("merge.unresolvedTitle"), t("merge.saveAnyway"));
        if (!ok) return;
      }
      await afterResolve(await api.gitResolveFile(repo, path, content));
    });
  }

  function acceptSide(side: Side) {
    void run(async () => {
      await afterResolve(await api.gitResolveWith(repo, path, side));
    });
  }

  async function cancel() {
    if (phase === "editor" && touched) {
      const ok = await confirm(t("merge.discardConfirm"), t("merge.discardTitle"), t("merge.discard"));
      if (!ok) return;
    }
    close();
  }

  function continueOperation() {
    void run(async () => {
      const st = await api.gitContinueOperation(repo, null);
      mergeState = st;
      await git.afterChange(repo);
      if (!st.operation && st.conflicted.length === 0) close();
      else statusNote = null;
    });
  }

  function abortOperation() {
    void run(async () => {
      const ok = await confirm(t("merge.abortConfirm"), t("merge.abortTitle"), t("merge.abort"));
      if (!ok) return;
      await api.gitAbortOperation(repo);
      await git.afterChange(repo);
      close();
    });
  }

  const specialNote = $derived.by(() => {
    const v = versions;
    if (!v) return "";
    if (v.binary) return t("merge.binary");
    const args = { ours: oursLabel, theirs: theirsLabel };
    if (v.ours === null && v.theirs !== null) return t("merge.deletedOurs", args);
    if (v.theirs === null && v.ours !== null) return t("merge.deletedTheirs", args);
    return t("merge.deletedBoth");
  });
</script>

<svelte:window onkeydown={onKeydown} />

<div class="merge">
  <header class="top">
    <div class="file">
      <svg viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.4" aria-hidden="true"
        ><circle cx="4" cy="3" r="1.6" /><circle cx="4" cy="13" r="1.6" /><circle cx="12" cy="8" r="1.6" /><path
          d="M4 4.6v6.8M4.6 4.4C6 7 8 8 10.4 8"
        /></svg
      >
      <strong>{title}</strong>
      <span class="muted mono" title={path}>{relPath}</span>
    </div>
    {#if phase === "editor"}
      <div class="toolbar">
        <div class="group">
          <span class="muted">{t("merge.applyNonConflicting")}</span>
          <button class="ghost" disabled={!pendingNonConflicting} title={t("merge.applyLeftHint")} onclick={() => applyNonConflicting("ours")}
            >{t("merge.applyLeft")}</button
          >
          <button class="ghost" disabled={!pendingNonConflicting} title={t("merge.applyAllHint")} onclick={() => applyNonConflicting("all")}
            >{t("merge.applyAll")}</button
          >
          <button class="ghost" disabled={!pendingNonConflicting} title={t("merge.applyRightHint")} onclick={() => applyNonConflicting("theirs")}
            >{t("merge.applyRight")}</button
          >
        </div>
        <div class="group">
          <button class="ghost nav" title={t("merge.prev")} aria-label={t("merge.prev")} onclick={() => navigate(-1)}>↑</button>
          <button class="ghost nav" title={t("merge.next")} aria-label={t("merge.next")} onclick={() => navigate(1)}>↓</button>
          {#if counts.conflicts === 0 && counts.changes === 0}
            <span class="count done">{t("merge.allProcessed")}</span>
          {:else}
            <span class="count" class:bad={counts.conflicts > 0}>{t("merge.remainingConflicts", { count: counts.conflicts })}</span>
            <span class="count">{t("merge.remainingChanges", { count: counts.changes })}</span>
          {/if}
        </div>
        <div class="group">
          <label class="auto" title={t("merge.autoApplyHint")}>
            <input type="checkbox" checked={autoApply} onchange={(e) => setAutoApply(e.currentTarget.checked)} />
            {t("merge.autoApply")}
          </label>
          <button class="ghost" title={t("merge.resetHint")} onclick={reset}>{t("merge.reset")}</button>
        </div>
        <div class="group end">
          <button disabled={busy} title={t("merge.acceptYoursHint")} onclick={() => acceptSide("ours")}>{t("merge.acceptYours")}</button>
          <button disabled={busy} title={t("merge.acceptTheirsHint")} onclick={() => acceptSide("theirs")}>{t("merge.acceptTheirs")}</button>
          <button disabled={busy} onclick={cancel}>{t("merge.cancel")}</button>
          <button class="primary" disabled={busy} title={t("merge.applyHint")} onclick={save}>{t("merge.apply")}</button>
        </div>
      </div>
    {/if}
    {#if actionError}<p class="error">{actionError}</p>{/if}
  </header>

  {#if phase === "loading"}
    <p class="muted pad">{t("merge.loading")}</p>
  {:else if phase === "error"}
    <div class="panel">
      <p class="error">{error}</p>
      <div class="row"><button onclick={close}>{t("merge.close")}</button></div>
    </div>
  {:else if phase === "special"}
    <div class="panel">
      <p>{specialNote}</p>
      <div class="row">
        <button class="primary" disabled={busy} title={t("merge.acceptYoursHint")} onclick={() => acceptSide("ours")}>
          {t("merge.acceptYours")}
          <span class="label">{oursLabel}</span>
          {#if versions?.ours === null}<span class="label">{t("merge.deletesFile")}</span>{/if}
        </button>
        <button class="primary" disabled={busy} title={t("merge.acceptTheirsHint")} onclick={() => acceptSide("theirs")}>
          {t("merge.acceptTheirs")}
          <span class="label">{theirsLabel}</span>
          {#if versions?.theirs === null}<span class="label">{t("merge.deletesFile")}</span>{/if}
        </button>
        <button disabled={busy} onclick={close}>{t("merge.cancel")}</button>
      </div>
    </div>
  {:else if phase === "status"}
    <div class="panel">
      {#if statusNote}<p>{statusNote}</p>{/if}
      {#if mergeState?.operation && mergeState.conflicted.length === 0}
        <div class="banner">
          <strong>{t("merge.allResolved")}</strong>
          <p class="muted">{mergeState.operation === "merge" ? t("merge.allResolvedMerge") : t("merge.allResolvedOther")}</p>
          <div class="row">
            <button class="primary" disabled={busy} onclick={continueOperation}>
              {mergeState.operation === "merge"
                ? t("merge.commitMerge")
                : mergeState.operation === "rebase"
                  ? t("merge.continueRebase")
                  : t("merge.continue")}
            </button>
            <button class="danger" disabled={busy} onclick={abortOperation}>{t("merge.abort")}</button>
          </div>
        </div>
      {:else if mergeState && mergeState.conflicted.length > 0}
        <p class="muted">{t("merge.newConflicts")}</p>
        <ul class="files">
          {#each mergeState.conflicted as p (p)}
            <li>
              <span class="mono">{basename(p)}</span>
              <button onclick={() => ws.openMerge(repo, p)}>{t("merge.resolve")}</button>
            </li>
          {/each}
        </ul>
      {:else}
        <p class="muted">{t("merge.finished")}</p>
      {/if}
      <div class="row"><button onclick={close}>{t("merge.close")}</button></div>
    </div>
  {:else}
    <div class="grid">
      <div class="head" title={oursLabel}>{t("merge.yours", { label: oursLabel })}</div>
      <div></div>
      <div class="head result">{t("merge.result")}</div>
      <div></div>
      <div class="head" title={theirsLabel}>{t("merge.theirs", { label: theirsLabel })}</div>

      <div class="pane">
        <CodeEditor value={oursText} readonly {language} extensions={sideExtensions()} onview={(v) => onView("ours", v)} />
      </div>
      <Strip bind:el={leftStrip} ribbons={leftRibbons} actions={leftActions} />
      <div class="pane">
        <CodeEditor value={initialResult} {language} extensions={resultExts()} onview={(v) => onView("result", v)} />
      </div>
      <Strip bind:el={rightStrip} ribbons={rightRibbons} actions={rightActions} />
      <div class="pane">
        <CodeEditor value={theirsText} readonly {language} extensions={sideExtensions()} onview={(v) => onView("theirs", v)} />
      </div>
    </div>
  {/if}
</div>

<style>
  .merge {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    min-width: 0;
    background: var(--bg);
    color: var(--text);
  }
  .top {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 8px 12px;
    border-bottom: 1px solid var(--border);
    background: var(--surface);
  }
  .file {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .file .mono {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
  }
  .toolbar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 14px;
  }
  .group {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 12px;
  }
  .group.end {
    margin-left: auto;
  }
  .group button {
    padding: 3px 8px;
    font-size: 12px;
  }
  .group button.nav {
    padding: 3px 6px;
  }
  .count {
    padding: 2px 6px;
    border-radius: 4px;
    background: var(--surface-2);
    color: var(--muted);
    white-space: nowrap;
  }
  .count.bad {
    color: var(--err);
  }
  .count.done {
    color: var(--ok);
  }
  .auto {
    display: flex;
    align-items: center;
    gap: 4px;
    color: var(--muted);
    cursor: pointer;
  }
  .auto input {
    padding: 0;
  }
  .muted {
    color: var(--muted);
  }
  .mono {
    font-family: var(--mono);
  }
  .pad {
    padding: 16px;
  }
  .error {
    color: var(--err);
    margin: 0;
    font-size: 12px;
    white-space: pre-wrap;
  }
  .grid {
    flex: 1 1 auto;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr) 44px minmax(0, 1fr) 44px minmax(0, 1fr);
    grid-template-rows: auto minmax(0, 1fr);
  }
  .head {
    padding: 4px 10px;
    font-size: 12px;
    color: var(--muted);
    background: var(--surface-2);
    border-bottom: 1px solid var(--border);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .grid > div:nth-child(2),
  .grid > div:nth-child(4) {
    background: var(--surface-2);
    border-bottom: 1px solid var(--border);
  }
  .head.result {
    color: var(--text);
    font-weight: 600;
  }
  .pane {
    min-width: 0;
    min-height: 0;
    overflow: hidden;
  }
  .panel {
    padding: 20px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    max-width: 720px;
  }
  .panel p {
    margin: 0;
  }
  .row {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }
  .label {
    font-weight: 400;
    opacity: 0.85;
    margin-left: 4px;
  }
  .banner {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px 14px;
    border: 1px solid var(--ok);
    border-radius: 8px;
    background: var(--surface);
  }
  button.danger {
    color: var(--err);
    border-color: var(--err);
  }
  .files {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .files li {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .files button {
    padding: 2px 8px;
    font-size: 12px;
  }
</style>
