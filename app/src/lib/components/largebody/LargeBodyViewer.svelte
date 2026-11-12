<script lang="ts">
  // Virtualized viewer for large text bodies (D41). Lines are fetched from Rust in windows
  // (responseLines) and cached (LRU); only the visible rows are rendered. Search runs in Rust.
  import { untrack } from "svelte";
  import { api, type BodyHandle, type BodySearchMatch } from "$lib/api";
  import { t } from "$lib/i18n/index.svelte";
  import { LineWindowCache } from "$lib/largebody/cache";
  import { firstLineAt, formatBytes, matchesByLine, scrollTopFor } from "$lib/largebody/format";
  import { LongLine, utf8Length, type ByteReader } from "$lib/largebody/longline";
  import { segments } from "$lib/largebody/tokens";
  import { ws } from "$lib/stores/workspace.svelte";
  import BodyInfoBar from "./BodyInfoBar.svelte";

  interface Props {
    handle: BodyHandle;
  }

  let { handle }: Props = $props();

  const ROW = 18;
  const MAX_PX = 15_000_000;
  const WINDOW = 500;
  const OVERSCAN = 40;
  /** Bytes per line returned by responseLines; longer lines are paged with responseRead. */
  const MAX_LINE_BYTES = 65_536;
  const PAGE_BYTES = 65_536;
  const SEEK_BYTES = 1 << 20;
  const MAX_LONG_LINES = 200;
  const MAX_RESULTS = 5_000;

  // ----- data -----
  let pretty = $state(false);
  let total = $state(0);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let version = $state(0);
  let generation = 0;
  const cache = new LineWindowCache(WINDOW, 40);
  const inflight = new Set<string>();

  async function loadWindow(w: number) {
    const key = `${pretty}:${w}`;
    if (inflight.has(key) || cache.has(w)) return;
    inflight.add(key);
    const gen = generation;
    const isPretty = pretty;
    try {
      const res = await api.responseLines(handle.id, w * WINDOW, WINDOW, isPretty, MAX_LINE_BYTES);
      if (gen !== generation) return;
      total = res.total_lines;
      cache.set(w, res.lines);
      // Cut lines: keep their byte range so they can be paged chunk by chunk.
      res.lines.forEach((text, k) => {
        const full = res.byte_lengths?.[k];
        const line = res.start + k;
        if (full === undefined || res.offsets?.[k] === undefined || longLines.has(line)) return;
        if (full > utf8Length(text)) {
          longLines.set(line, new LongLine(res.offsets[k], full, text));
          while (longLines.size > MAX_LONG_LINES) longLines.delete(longLines.keys().next().value as number);
        }
      });
      if (pendingSeek && pendingSeek.line >= res.start && pendingSeek.line < res.start + res.lines.length) {
        const p = pendingSeek;
        pendingSeek = null;
        void seekInLine(p.line, p.column);
      }
      version++;
      error = null;
    } catch (e) {
      if (gen !== generation) return;
      if (isPretty) {
        // Pretty view unavailable (invalid JSON, still building, …): back to raw.
        error = t("large.prettyUnavailable", { msg: String(e) });
        setMode(false);
      } else {
        error = String(e);
      }
    } finally {
      inflight.delete(key);
      if (gen === generation) loading = false;
    }
  }

  function setMode(p: boolean) {
    generation++;
    pretty = p;
    cache.clear();
    inflight.clear();
    total = 0;
    loading = true;
    longLines.clear();
    pendingSeek = null;
    clearSearch();
    if (scroller) scroller.scrollTop = 0;
    scrollTop = 0;
    void loadWindow(0);
  }

  $effect(() => {
    void handle.id;
    untrack(() => setMode(false));
  });

  // ----- virtualization -----
  let scroller: HTMLDivElement | undefined = $state();
  let scrollTop = $state(0);
  let viewH = $state(400);

  const scaled = $derived(total * ROW > MAX_PX);
  const spacerH = $derived(scaled ? MAX_PX : total * ROW);
  const first = $derived(firstLineAt(scrollTop, viewH, ROW, total, MAX_PX));
  const count = $derived(Math.ceil(viewH / ROW) + 2);
  const offsetY = $derived(scaled ? scrollTop : first * ROW);
  const gutterW = $derived(`${String(Math.max(total, 1)).length + 1}ch`);

  $effect(() => {
    if (total === 0) return;
    const from = Math.max(0, first - OVERSCAN - WINDOW / 2);
    const to = first + count + OVERSCAN + WINDOW / 2;
    for (const w of cache.missing(from, to, total)) void loadWindow(w);
  });

  // ----- cut (very long) lines: paged with responseRead, one ≤64K-char chunk rendered at a time -----
  const longLines = new Map<number, LongLine>();
  let pendingSeek: { line: number; column: number } | null = null;
  let lineBusy = $state<Record<number, boolean>>({});

  const reader = (): ByteReader => {
    const id = handle.id;
    const isPretty = pretty;
    return async (offset, length) => {
      const c = await api.responseRead(id, offset, length, isPretty);
      return { data: c.data, length: c.length };
    };
  };

  async function pageLine(line: number, dir: 1 | -1) {
    const ll = longLines.get(line);
    if (!ll || lineBusy[line]) return;
    const gen = generation;
    lineBusy[line] = true;
    try {
      const target = ll.pos + dir;
      if (target < 0) return;
      if (await ll.ensure(target, reader(), PAGE_BYTES)) {
        if (gen !== generation) return;
        ll.pos = target;
        ll.evict(4);
        version++;
      }
    } catch (e) {
      error = String(e);
    } finally {
      lineBusy[line] = false;
    }
  }

  /** Show the chunk of a cut line that contains character `column` (search jumps). */
  async function seekInLine(line: number, column: number) {
    const ll = longLines.get(line);
    if (!ll) return;
    const c = ll.current;
    if (column >= c.charStart && column < c.charStart + c.charLength && c.text !== null) return;
    const gen = generation;
    lineBusy[line] = true;
    try {
      const idx = await ll.seekChar(column, reader(), SEEK_BYTES);
      if (idx < 0 || gen !== generation) return;
      await ll.ensure(idx, reader(), PAGE_BYTES);
      ll.pos = idx;
      ll.evict(4);
      version++;
    } catch (e) {
      error = String(e);
    } finally {
      lineBusy[line] = false;
    }
  }

  /** Scroll to a match and, for cut lines, load the chunk that contains it. */
  function revealMatch(m: BodySearchMatch) {
    jumpTo(m.line);
    if (longLines.has(m.line)) void seekInLine(m.line, m.column);
    else if (cache.get(m.line) === undefined) pendingSeek = { line: m.line, column: m.column };
  }

  /** What to render for a row: the whole line, or the current chunk of a cut line. */
  function view(i: number, text: string) {
    const ll = longLines.get(i);
    if (!ll) return { text, start: 0, prev: 0, more: 0, cut: false };
    const c = ll.current;
    const coveredAfter = ll.chunks.slice(ll.pos + 1).reduce((a, x) => a + x.byteLength, 0) + ll.remainingBytes;
    return {
      text: c.text ?? "",
      start: c.charStart,
      prev: c.byteOffset - ll.lineOffset,
      more: ll.hasNext() ? coveredAfter : 0,
      cut: true,
    };
  }

  /** Matches of a line shifted into the rendered chunk. */
  function shift(matches: BodySearchMatch[], start: number, len: number): BodySearchMatch[] {
    return matches
      .filter((m) => m.column + m.length > start && m.column < start + len)
      .map((m) => ({ ...m, column: m.column - start }));
  }

  const rows = $derived.by(() => {
    void version;
    const out: { i: number; text: string | undefined }[] = [];
    const end = Math.min(total, first + count);
    for (let i = first; i < end; i++) out.push({ i, text: cache.get(i) });
    return out;
  });

  function onScroll() {
    if (scroller) scrollTop = scroller.scrollTop;
  }

  function jumpTo(line: number) {
    if (!scroller || total === 0) return;
    const target = Math.max(0, Math.min(total - 1, line));
    const centered = Math.max(0, target - Math.floor(viewH / ROW / 2));
    scroller.scrollTop = scrollTopFor(centered, viewH, ROW, total, MAX_PX);
    scrollTop = scroller.scrollTop;
  }

  // ----- search -----
  let query = $state("");
  let useRegex = $state(false);
  let caseSensitive = $state(false);
  let results = $state.raw<BodySearchMatch[]>([]);
  let byLine = $state.raw(new Map<number, BodySearchMatch[]>());
  let truncated = $state(false);
  let current = $state(-1);
  let searching = $state(false);
  let searchError = $state<string | null>(null);
  let searchedKey = $state("");

  function clearSearch() {
    results = [];
    byLine = new Map();
    truncated = false;
    current = -1;
    searchedKey = "";
    searchError = null;
  }

  async function runSearch(): Promise<boolean> {
    const key = `${pretty}|${useRegex}|${caseSensitive}|${query}`;
    if (key === searchedKey) return true;
    if (!query) {
      clearSearch();
      return false;
    }
    searching = true;
    searchError = null;
    try {
      const res = await api.responseSearch(handle.id, query, {
        regex: useRegex,
        case_sensitive: caseSensitive,
        pretty,
        max_results: MAX_RESULTS,
      });
      results = res.matches;
      byLine = matchesByLine(res.matches);
      truncated = res.truncated;
      searchedKey = key;
      // Start from the first match at or after the current viewport.
      const idx = res.matches.findIndex((m) => m.line >= first);
      current = res.matches.length ? (idx < 0 ? 0 : idx) : -1;
      if (current >= 0) revealMatch(res.matches[current]);
      return false;
    } catch (e) {
      clearSearch();
      searchError = String(e);
      return false;
    } finally {
      searching = false;
    }
  }

  async function step(dir: 1 | -1) {
    const already = await runSearch();
    if (!already || results.length === 0) return;
    current = (current + dir + results.length) % results.length;
    revealMatch(results[current]);
  }

  function onSearchKey(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      void step(e.shiftKey ? -1 : 1);
    } else if (e.key === "Escape") {
      query = "";
      clearSearch();
    }
  }

  let searchInput: HTMLInputElement | undefined = $state();
  function onViewerKey(e: KeyboardEvent) {
    if ((e.metaKey || e.ctrlKey) && (e.key === "f" || e.key === "F")) {
      e.preventDefault();
      e.stopPropagation();
      searchInput?.focus();
      searchInput?.select();
    }
  }

  // ----- go to line -----
  let gotoValue = $state("");
  function onGotoKey(e: KeyboardEvent) {
    if (e.key !== "Enter") return;
    const n = Math.floor(Number(gotoValue));
    if (Number.isFinite(n) && n >= 1) jumpTo(n - 1);
  }

  // ----- copy -----
  let copied = $state(false);
  async function copy() {
    const sel = window.getSelection();
    let text = "";
    if (sel && !sel.isCollapsed && scroller && scroller.contains(sel.anchorNode)) text = sel.toString();
    else text = rows.map((r) => r.text ?? "").join("\n");
    try {
      await navigator.clipboard.writeText(text);
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch (e) {
      ws.fail(e);
    }
  }

  const json = $derived(handle.is_json);
  const currentMatch = $derived(current >= 0 ? results[current] : null);
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="viewer" onkeydown={onViewerKey}>
  <BodyInfoBar {handle} {pretty}>
    {#if handle.is_json}
      <span class="seg" role="radiogroup" aria-label={t("large.view")}>
        <button role="radio" aria-checked={!pretty} class:active={!pretty} onclick={() => pretty && setMode(false)}>{t("large.raw")}</button>
        <button role="radio" aria-checked={pretty} class:active={pretty} onclick={() => !pretty && setMode(true)}>{t("large.pretty")}</button>
      </span>
    {/if}
    <span class="muted">{t("large.lines", { n: total.toLocaleString() })}</span>
  </BodyInfoBar>

  <div class="toolbar">
    <span class="search">
      <input
        bind:this={searchInput}
        class="mono"
        bind:value={query}
        placeholder={t("large.searchPlaceholder")}
        aria-label={t("large.searchPlaceholder")}
        spellcheck="false"
        onkeydown={onSearchKey}
      />
      <button class="toggle" class:on={useRegex} aria-pressed={useRegex} title={t("large.regex")} onclick={() => (useRegex = !useRegex)}>.*</button>
      <button class="toggle" class:on={caseSensitive} aria-pressed={caseSensitive} title={t("large.matchCase")} onclick={() => (caseSensitive = !caseSensitive)}>Aa</button>
      <button class="ghost nav" title={t("large.prev")} aria-label={t("large.prev")} onclick={() => step(-1)}>↑</button>
      <button class="ghost nav" title={t("large.next")} aria-label={t("large.next")} onclick={() => step(1)}>↓</button>
      <span class="count">
        {#if searching}
          {t("large.searching")}
        {:else if searchError}
          <span class="err">{searchError}</span>
        {:else if searchedKey && results.length === 0}
          {t("large.noMatches")}
        {:else if results.length}
          {truncated
            ? t("large.matchesMore", { n: current + 1, m: results.length })
            : t("large.matches", { n: current + 1, m: results.length })}
        {/if}
      </span>
    </span>
    <span class="spacer"></span>
    <input
      class="goto"
      type="number"
      min="1"
      bind:value={gotoValue}
      placeholder={t("large.goto")}
      aria-label={t("large.goto")}
      onkeydown={onGotoKey}
    />
    <button class="ghost small" onclick={copy}>{copied ? t("large.copied") : t("large.copy")}</button>
  </div>

  {#if error}<p class="err banner">{error}</p>{/if}

  <div class="scroller" bind:this={scroller} bind:clientHeight={viewH} onscroll={onScroll} tabindex="-1">
    {#if loading && total === 0}
      <div class="loading"><span class="spinner"></span>{pretty ? t("large.buildingPretty") : t("common.loading")}</div>
    {/if}
    <div class="spacer-v" style:height="{spacerH}px"></div>
    <div class="layer" style:transform="translateY({offsetY}px)">
      {#each rows as row (row.i)}
        {@const lineMatches = byLine.get(row.i) ?? []}
        <div class="line" style:height="{ROW}px">
          <span class="ln" style:width={gutterW}>{row.i + 1}</span>
          {#if row.text === undefined}
            <span class="pending">…</span>
          {:else}
            {@const v = view(row.i, row.text)}
            {@const shown = v.cut ? shift(lineMatches, v.start, v.text.length) : lineMatches}
            {#if v.prev > 0}<button class="more" disabled={lineBusy[row.i]} onclick={() => pageLine(row.i, -1)}
                >{t("large.prevChars", { n: formatBytes(v.prev) })}</button
              >{/if}<span class="code"
              >{#each segments(v.text, json, shown) as seg, k (k)}<span
                  class="tok {seg.kind}"
                  class:hit={seg.match >= 0}
                  class:current={seg.match >= 0 &&
                    !!currentMatch &&
                    currentMatch.line === row.i &&
                    shown[seg.match].column + v.start === currentMatch.column}>{seg.text}</span
                >{/each}</span
            >{#if v.more > 0}<button class="more" disabled={lineBusy[row.i]} onclick={() => pageLine(row.i, 1)}
                >{lineBusy[row.i] ? t("common.loading") : t("large.moreChars", { n: formatBytes(v.more) })}</button
              >{/if}
          {/if}
        </div>
      {/each}
    </div>
  </div>
</div>

<style>
  .viewer {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .muted {
    color: var(--muted);
  }
  .seg {
    display: inline-flex;
    gap: 2px;
  }
  .seg button {
    font-size: 12px;
    padding: 2px 8px;
    color: var(--muted);
  }
  .seg button.active {
    color: var(--text);
    border-color: var(--accent);
    background: var(--surface-2);
  }
  .toolbar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding-bottom: 6px;
    flex-wrap: wrap;
  }
  .search {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    min-width: 0;
  }
  .search input {
    width: 220px;
    font-size: 12px;
    padding: 3px 8px;
  }
  .toggle {
    font-family: var(--mono);
    font-size: 11px;
    padding: 2px 6px;
    color: var(--muted);
  }
  .toggle.on {
    color: var(--accent-text);
    background: var(--accent);
    border-color: var(--accent);
  }
  .nav {
    padding: 2px 6px;
  }
  .count {
    font-size: 12px;
    color: var(--muted);
    margin-left: 4px;
    white-space: nowrap;
  }
  .spacer {
    flex: 1;
  }
  .goto {
    width: 110px;
    font-size: 12px;
    padding: 3px 8px;
  }
  .small {
    font-size: 12px;
    padding: 3px 8px;
  }
  .err {
    color: var(--err);
  }
  .banner {
    font-size: 12px;
    margin: 0 0 6px;
    word-break: break-word;
  }
  .scroller {
    position: relative;
    flex: 1;
    min-height: 0;
    overflow: auto;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--surface);
    font-family: var(--mono);
    font-size: 12.5px;
    outline: none;
  }
  .spacer-v {
    width: 1px;
  }
  .layer {
    position: absolute;
    top: 0;
    left: 0;
    min-width: 100%;
  }
  .line {
    display: flex;
    align-items: center;
    white-space: pre;
    line-height: 18px;
  }
  .ln {
    position: sticky;
    left: 0;
    flex: none;
    text-align: right;
    padding: 0 8px 0 6px;
    margin-right: 8px;
    color: var(--muted);
    background: var(--surface-2);
    border-right: 1px solid var(--border);
    user-select: none;
    box-sizing: content-box;
  }
  .code {
    padding-right: 12px;
  }
  .pending {
    color: var(--muted);
  }
  .tok.key {
    color: var(--syntax-key);
  }
  .tok.string {
    color: var(--syntax-string);
  }
  .tok.number {
    color: var(--syntax-number);
  }
  .tok.bool {
    color: var(--syntax-bool);
  }
  .tok.null {
    color: var(--syntax-null);
    font-style: italic;
  }
  .tok.punct {
    color: var(--syntax-punct);
  }
  .tok.hit {
    background: var(--search-match);
    border-radius: 2px;
  }
  .tok.current {
    outline: 1px solid var(--accent);
    background: color-mix(in srgb, var(--accent) 35%, transparent);
  }
  .more {
    margin-left: 6px;
    font-family: inherit;
    font-size: 11px;
    padding: 0 6px;
    color: var(--accent);
    background: var(--surface-2);
    line-height: 16px;
  }
  .loading {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    color: var(--muted);
    font-family: system-ui, sans-serif;
    font-size: 13px;
    z-index: 1;
  }
  .spinner {
    width: 14px;
    height: 14px;
    border-radius: 50%;
    border: 2px solid var(--border);
    border-top-color: var(--accent);
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
