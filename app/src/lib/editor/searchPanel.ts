// Custom CodeMirror search panel (D23): multi-line query/replace textareas,
// regex / case / whole-word toggles, "n of m" counter, replace & replace all
// (editable editors only).
//
// Performance: CodeMirror's literal SearchCursor normalises char by char and is
// ~20-50x slower than its RegExpCursor on multi-MB docs (≈600ms vs ≈20ms for a
// no-match scan of 6.7MB). So literal mode is executed as an escaped regex (and
// `$` in the replacement is escaped as `$$`), giving identical matches/replacements.
//
// Multi-line notes:
// - Literal mode: the query text (incl. newlines) is matched verbatim; an escaped
//   pattern containing a newline uses the multi-line regex cursor.
// - Regex mode: CodeMirror's RegExpCursor is line-based unless the pattern
//   contains `\n`, `\r`, `\s`, `\W`, `\D`, a literal newline or `[^`; then it
//   switches to a multi-line cursor, so `a\nb`, `[\s\S]*?`, `[^x]+` all cross
//   lines. Escapes such as `\x0a`, `\u000a`, `\cJ` don't trigger that switch, so we
//   append a no-op `(?:\n){0}` to such patterns (stripped again when shown).
//   Limitation: `.` never matches a newline (no `s` flag), as in plain JS.
// - Replacement in regex mode supports `$1`, `$&`, `$$` and `\n`/`\t` escapes.

import { EditorSelection, EditorState, type Text } from "@codemirror/state";
import {
  EditorView,
  runScopeHandlers,
  type KeyBinding,
  type Panel,
  type ViewUpdate,
} from "@codemirror/view";
import {
  SearchQuery,
  closeSearchPanel,
  findNext,
  findPrevious,
  getSearchQuery,
  openSearchPanel,
  replaceAll,
  replaceNext,
  selectMatches,
  setSearchQuery,
} from "@codemirror/search";
import { t } from "$lib/i18n/index.svelte";

/** Max matches counted for the "n of m" indicator. */
const COUNT_CAP = 10_000;
/** Debounce for recounting after typing / doc edits. */
const COUNT_DELAY = 120;
/** Max selection length used to prefill the query. */
const PREFILL_MAX = 5_000;

const FORCE_MULTILINE = "(?:\\n){0}";
const NEEDS_FORCE = /\\x0[aAdD]|\\u000[aAdD]|\\u\{0*[aAdD]\}|\\c[jJmM]/;
const ALREADY_MULTILINE = /\\[sWDnr]|\n|\r|\[\^/;

const panels = new WeakMap<EditorView, FindPanel>();
/** Queries built from literal-mode input → the text the user actually typed. */
const literalQueries = new WeakMap<SearchQuery, { search: string; replace: string }>();

function toCursorPattern(src: string): string {
  return NEEDS_FORCE.test(src) && !ALREADY_MULTILINE.test(src) ? src + FORCE_MULTILINE : src;
}
function fromCursorPattern(src: string): string {
  return src.endsWith(FORCE_MULTILINE) ? src.slice(0, -FORCE_MULTILINE.length) : src;
}
function escapeRegExp(s: string): string {
  return s.replace(/[\\^$.*+?()[\]{}|/]/g, "\\$&");
}

// ---------- tiny DOM helpers ----------

type Attrs = Record<string, string>;
function h<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  attrs: Attrs = {},
  ...children: (Node | string)[]
): HTMLElementTagNameMap[K] {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, v);
  for (const c of children) el.append(c);
  return el;
}

const SVG_NS = "http://www.w3.org/2000/svg";
function icon(d: string): SVGSVGElement {
  const svg = document.createElementNS(SVG_NS, "svg");
  svg.setAttribute("viewBox", "0 0 16 16");
  svg.setAttribute("width", "14");
  svg.setAttribute("height", "14");
  svg.setAttribute("aria-hidden", "true");
  const p = document.createElementNS(SVG_NS, "path");
  p.setAttribute("d", d);
  p.setAttribute("fill", "none");
  p.setAttribute("stroke", "currentColor");
  p.setAttribute("stroke-width", "1.6");
  p.setAttribute("stroke-linecap", "round");
  p.setAttribute("stroke-linejoin", "round");
  svg.append(p);
  return svg;
}
const ICONS = {
  up: "M4 10l4-4 4 4",
  down: "M4 6l4 4 4-4",
  chevron: "M6 4l4 4-4 4",
  close: "M4.5 4.5l7 7M11.5 4.5l-7 7",
  selectAll: "M2.5 4h11M2.5 8h11M2.5 12h7",
};

// ---------- panel ----------

class FindPanel implements Panel {
  dom: HTMLElement;
  top = true;

  private view: EditorView;
  private query: HTMLTextAreaElement;
  private replace: HTMLTextAreaElement;
  private caseBtn: HTMLButtonElement;
  private reBtn: HTMLButtonElement;
  private wordBtn: HTMLButtonElement;
  private counter: HTMLSpanElement;
  private error: HTMLDivElement;
  private replaceRow: HTMLDivElement;
  private toggleReplaceBtn: HTMLButtonElement;
  private prevBtn: HTMLButtonElement;
  private nextBtn: HTMLButtonElement;
  private allBtn: HTMLButtonElement;
  private replaceBtn: HTMLButtonElement;
  private replaceAllBtn: HTMLButtonElement;
  private closeBtn: HTMLButtonElement;

  private caseSensitive = false;
  private regexp = false;
  private wholeWord = false;
  private replaceOpen = false;

  // Match cache for the counter: positions of up to COUNT_CAP matches.
  private cacheDoc: Text | null = null;
  private cacheQuery: SearchQuery | null = null;
  private froms: number[] = [];
  private tos: number[] = [];
  private capped = false;
  private timer: ReturnType<typeof setTimeout> | undefined;

  constructor(view: EditorView) {
    this.view = view;
    panels.set(view, this);

    const textarea = (cls: string) =>
      h("textarea", {
        class: `cm-pm-field ${cls}`,
        rows: "1",
        spellcheck: "false",
        autocomplete: "off",
        autocapitalize: "off",
        autocorrect: "off",
      });
    const btn = (cls: string, content: Node | string) =>
      h("button", { type: "button", class: `cm-pm-btn ${cls}` }, content);

    this.query = textarea("cm-pm-query");
    this.query.setAttribute("main-field", "true");
    this.replace = textarea("cm-pm-replace");

    this.caseBtn = btn("cm-pm-toggle", "Aa");
    this.reBtn = btn("cm-pm-toggle", ".*");
    this.wordBtn = btn("cm-pm-toggle cm-pm-word", "W");
    this.counter = h("span", { class: "cm-pm-counter", "aria-live": "polite" });
    this.prevBtn = btn("cm-pm-icon", icon(ICONS.up));
    this.nextBtn = btn("cm-pm-icon", icon(ICONS.down));
    this.allBtn = btn("cm-pm-icon", icon(ICONS.selectAll));
    this.closeBtn = btn("cm-pm-icon cm-pm-close", icon(ICONS.close));
    this.toggleReplaceBtn = btn("cm-pm-icon cm-pm-expand", icon(ICONS.chevron));
    this.replaceBtn = btn("cm-pm-text", "");
    this.replaceAllBtn = btn("cm-pm-text", "");
    this.error = h("div", { class: "cm-pm-error", role: "alert" });

    const queryWrap = h(
      "div",
      { class: "cm-pm-fieldwrap" },
      this.query,
      h("div", { class: "cm-pm-toggles" }, this.caseBtn, this.reBtn, this.wordBtn),
    );
    const findRow = h(
      "div",
      { class: "cm-pm-row" },
      queryWrap,
      this.counter,
      h("div", { class: "cm-pm-actions" }, this.prevBtn, this.nextBtn, this.allBtn, this.closeBtn),
    );
    this.replaceRow = h(
      "div",
      { class: "cm-pm-row" },
      h("div", { class: "cm-pm-fieldwrap" }, this.replace),
      h("div", { class: "cm-pm-actions" }, this.replaceBtn, this.replaceAllBtn),
    );
    this.dom = h(
      "div",
      { class: "cm-pm-search" },
      this.toggleReplaceBtn,
      h("div", { class: "cm-pm-rows" }, findRow, this.replaceRow, this.error),
    );

    // Events
    this.query.addEventListener("input", () => {
      this.autosize(this.query);
      this.commit();
    });
    this.replace.addEventListener("input", () => {
      this.autosize(this.replace);
      this.commit();
    });
    this.query.addEventListener("keydown", (e) => this.onKey(e, "query"));
    this.replace.addEventListener("keydown", (e) => this.onKey(e, "replace"));
    this.dom.addEventListener("keydown", (e) => {
      if (e.target instanceof HTMLTextAreaElement) return;
      if (runScopeHandlers(this.view, e, "search-panel")) e.preventDefault();
    });

    this.caseBtn.onclick = () => {
      this.caseSensitive = !this.caseSensitive;
      this.commit();
    };
    this.reBtn.onclick = () => {
      this.regexp = !this.regexp;
      this.commit();
    };
    this.wordBtn.onclick = () => {
      this.wholeWord = !this.wholeWord;
      this.commit();
    };
    this.prevBtn.onclick = () => findPrevious(this.view);
    this.nextBtn.onclick = () => findNext(this.view);
    this.allBtn.onclick = () => selectMatches(this.view);
    this.replaceBtn.onclick = () => replaceNext(this.view);
    this.replaceAllBtn.onclick = () => replaceAll(this.view);
    this.closeBtn.onclick = () => closeSearchPanel(this.view);
    this.toggleReplaceBtn.onclick = () => this.setReplaceOpen(!this.replaceOpen);

    this.syncFromQuery(getSearchQuery(view.state));
    this.relabel();
    this.renderReplaceRow();
  }

  // ----- Panel lifecycle -----

  mount() {
    this.autosize(this.query);
    this.autosize(this.replace);
    this.focusQuery();
    this.scheduleCount(0);
  }

  update(u: ViewUpdate) {
    const q = getSearchQuery(u.state);
    const queryChanged = !q.eq(getSearchQuery(u.startState));
    if (queryChanged) this.syncFromQuery(q);
    if (u.startState.readOnly !== u.state.readOnly) this.renderReplaceRow();
    if (u.startState.facet(EditorState.phrases) !== u.state.facet(EditorState.phrases)) this.relabel();
    if (queryChanged || u.docChanged) this.scheduleCount();
    else if (u.selectionSet) this.renderCounter();
  }

  destroy() {
    clearTimeout(this.timer);
    if (panels.get(this.view) === this) panels.delete(this.view);
  }

  // ----- public helpers used by commands -----

  focusQuery() {
    this.query.focus();
    this.query.select();
  }

  focusReplace() {
    this.replace.focus();
    this.replace.select();
  }

  setReplaceOpen(open: boolean) {
    this.replaceOpen = open;
    this.renderReplaceRow();
  }

  get editable() {
    return !this.view.state.readOnly;
  }

  get hasQuery() {
    return this.query.value.length > 0;
  }

  // ----- internals -----

  private buildQuery(): SearchQuery {
    const search = this.query.value;
    const replace = this.replace.value;
    if (this.regexp) {
      // Regex mode: replacement gets `\n`/`\t` unquoting plus `$1`/`$&`.
      return new SearchQuery({
        search: toCursorPattern(search),
        replace,
        caseSensitive: this.caseSensitive,
        regexp: true,
        wholeWord: this.wholeWord,
      });
    }
    // Literal mode, run as an escaped regex (see top of file); no unquoting.
    const q = new SearchQuery({
      search: escapeRegExp(search),
      replace: replace.replace(/\$/g, "$$$$"),
      caseSensitive: this.caseSensitive,
      regexp: true,
      wholeWord: this.wholeWord,
      literal: true,
    });
    literalQueries.set(q, { search, replace });
    return q;
  }

  /** Put `text` in the query field (escaped in regex mode) and apply it. */
  setSearchText(text: string) {
    this.query.value = this.regexp ? escapeRegExp(text) : text;
    this.autosize(this.query);
    this.commit();
  }

  private commit() {
    const q = this.buildQuery();
    if (!q.eq(getSearchQuery(this.view.state))) {
      this.view.dispatch({ effects: setSearchQuery.of(q) });
    } else {
      this.renderToggles();
    }
  }

  private syncFromQuery(q: SearchQuery) {
    const lit = literalQueries.get(q);
    const search = lit ? lit.search : q.regexp ? fromCursorPattern(q.search) : q.search;
    const replace = lit ? lit.replace : q.replace;
    if (this.query.value !== search) {
      this.query.value = search;
      this.autosize(this.query);
    }
    if (this.replace.value !== replace) {
      this.replace.value = replace;
      this.autosize(this.replace);
    }
    this.caseSensitive = q.caseSensitive;
    this.regexp = lit ? false : q.regexp;
    this.wholeWord = q.wholeWord;
    this.renderToggles();
  }

  private onKey(e: KeyboardEvent, field: "query" | "replace") {
    if (e.isComposing) return;
    if (e.key === "Enter") {
      e.preventDefault();
      if (e.altKey || e.ctrlKey) {
        // Insert a literal newline in the field.
        const ta = e.target as HTMLTextAreaElement;
        ta.setRangeText("\n", ta.selectionStart, ta.selectionEnd, "end");
        ta.dispatchEvent(new Event("input"));
        return;
      }
      if (e.shiftKey) findPrevious(this.view);
      else if (field === "replace" && this.editable) replaceNext(this.view);
      else findNext(this.view);
      return;
    }
    // Esc, Mod-f, Mod-g, F3… (scope "search-panel")
    if (runScopeHandlers(this.view, e, "search-panel")) e.preventDefault();
  }

  private autosize(ta: HTMLTextAreaElement) {
    // Up to ~5 rows, then scroll. line-height 18px + 2*3px padding + 2*1px border.
    const max = 5 * 18 + 8;
    ta.style.height = "auto";
    const needed = ta.scrollHeight + 2;
    ta.style.height = Math.min(needed, max) + "px";
    ta.style.overflowY = needed > max ? "auto" : "hidden";
  }

  private renderToggles() {
    const set = (b: HTMLButtonElement, on: boolean) => {
      b.setAttribute("aria-pressed", String(on));
      b.classList.toggle("cm-pm-on", on);
    };
    set(this.caseBtn, this.caseSensitive);
    set(this.reBtn, this.regexp);
    set(this.wordBtn, this.wholeWord);
  }

  private renderReplaceRow() {
    const editable = this.editable;
    const open = editable && this.replaceOpen;
    this.toggleReplaceBtn.style.display = editable ? "" : "none";
    this.toggleReplaceBtn.setAttribute("aria-expanded", String(open));
    this.toggleReplaceBtn.classList.toggle("cm-pm-open", open);
    this.replaceRow.style.display = open ? "" : "none";
    this.dom.classList.toggle("cm-pm-readonly", !editable);
  }

  private relabel() {
    const label = (el: HTMLElement, text: string) => {
      el.title = text;
      el.setAttribute("aria-label", text);
    };
    this.query.placeholder = t("editor.find");
    label(this.query, t("editor.find"));
    this.query.title = t("editor.search.findHint");
    this.replace.placeholder = t("editor.replace");
    label(this.replace, t("editor.replace"));
    this.replace.title = t("editor.search.replaceHint");
    label(this.caseBtn, t("editor.search.matchCase"));
    label(this.reBtn, t("editor.search.regexp"));
    label(this.wordBtn, t("editor.search.wholeWord"));
    label(this.prevBtn, t("editor.search.previous"));
    label(this.nextBtn, t("editor.search.next"));
    label(this.allBtn, t("editor.search.selectAll"));
    label(this.closeBtn, t("editor.search.close"));
    label(this.toggleReplaceBtn, t("editor.search.toggleReplace"));
    this.replaceBtn.textContent = t("editor.search.replaceButton");
    this.replaceBtn.title = t("editor.search.replaceButtonHint");
    this.replaceAllBtn.textContent = t("editor.search.replaceAllButton");
    this.replaceAllBtn.title = t("editor.search.replaceAllButton");
    this.error.textContent = t("editor.search.invalidRegexp");
    this.renderCounter();
  }

  // ----- counting -----

  private scheduleCount(delay = COUNT_DELAY) {
    clearTimeout(this.timer);
    this.timer = setTimeout(() => {
      this.timer = undefined;
      this.count();
      this.renderCounter();
    }, delay);
  }

  private count() {
    const state = this.view.state;
    const q = getSearchQuery(state);
    if (this.cacheDoc === state.doc && this.cacheQuery && this.cacheQuery.eq(q)) return;
    this.cacheDoc = state.doc;
    this.cacheQuery = q;
    this.froms = [];
    this.tos = [];
    this.capped = false;
    if (!q.valid) return;
    const cursor = q.getCursor(state);
    for (let r = cursor.next(); !r.done; r = cursor.next()) {
      if (this.froms.length >= COUNT_CAP) {
        this.capped = true;
        break;
      }
      this.froms.push(r.value.from);
      this.tos.push(r.value.to);
    }
  }

  private renderCounter() {
    const state = this.view.state;
    const q = getSearchQuery(state);
    const invalid = this.regexp && this.query.value.length > 0 && !q.valid;
    this.dom.classList.toggle("cm-pm-invalid", invalid);
    const stale = this.cacheDoc !== state.doc || !this.cacheQuery || !this.cacheQuery.eq(q);
    const empty = !this.query.value || invalid;
    const none = !empty && !stale && this.froms.length === 0;
    this.dom.classList.toggle("cm-pm-noresults", none);
    for (const b of [this.prevBtn, this.nextBtn, this.allBtn, this.replaceBtn, this.replaceAllBtn])
      b.disabled = empty || none;

    if (empty) {
      this.counter.textContent = "";
      return;
    }
    if (stale) return; // keep the previous text until the debounced recount lands
    if (none) {
      this.counter.textContent = t("editor.search.noResults");
      return;
    }
    const total = this.capped ? `${COUNT_CAP}+` : String(this.froms.length);
    const sel = state.selection.main;
    // Binary search for a match equal to the main selection.
    let lo = 0;
    let hi = this.froms.length - 1;
    let idx = -1;
    while (lo <= hi) {
      const mid = (lo + hi) >> 1;
      if (this.froms[mid] < sel.from) lo = mid + 1;
      else if (this.froms[mid] > sel.from) hi = mid - 1;
      else {
        if (this.tos[mid] === sel.to) idx = mid;
        break;
      }
    }
    this.counter.textContent =
      idx >= 0
        ? t("editor.search.count", { current: idx + 1, total })
        : t("editor.search.countUnknown", { total });
  }
}

export function createFindPanel(view: EditorView): Panel {
  return new FindPanel(view);
}

/** Open (or focus) the panel, prefilled with the current selection. */
export function openFind(view: EditorView, withReplace = false): boolean {
  const sel = view.state.selection.main;
  const selText =
    !sel.empty && sel.to - sel.from <= PREFILL_MAX ? view.state.sliceDoc(sel.from, sel.to) : "";
  openSearchPanel(view);
  const panel = panels.get(view);
  if (!panel) return true;
  if (selText) panel.setSearchText(selText);
  if (withReplace && panel.editable) {
    panel.setReplaceOpen(true);
    if (panel.hasQuery) panel.focusReplace();
    else panel.focusQuery();
  } else {
    panel.focusQuery();
  }
  return true;
}

export const findKeymap: KeyBinding[] = [
  { key: "Mod-f", run: (v) => openFind(v, false), scope: "editor search-panel", preventDefault: true },
  { key: "Mod-Alt-f", run: (v) => openFind(v, true), scope: "editor search-panel", preventDefault: true },
  // Cmd+H hides the app on macOS, so only bind it elsewhere.
  { win: "Mod-h", linux: "Mod-h", run: (v) => openFind(v, true), scope: "editor search-panel", preventDefault: true },
];

/** Theme for the panel; all colours from theme CSS vars. */
export const findPanelTheme = EditorView.theme({
  ".cm-pm-search": {
    display: "flex",
    alignItems: "flex-start",
    gap: "4px",
    padding: "6px 8px",
    fontSize: "12px",
    fontFamily: "inherit",
    color: "var(--text)",
  },
  ".cm-pm-rows": { flex: "1 1 auto", minWidth: 0, display: "flex", flexDirection: "column", gap: "4px" },
  ".cm-pm-row": { display: "flex", alignItems: "flex-start", gap: "6px", minWidth: 0 },
  ".cm-pm-fieldwrap": {
    position: "relative",
    flex: "1 1 auto",
    minWidth: "80px",
    maxWidth: "520px",
  },
  ".cm-pm-field": {
    display: "block",
    width: "100%",
    boxSizing: "border-box",
    resize: "none",
    margin: 0,
    padding: "3px 7px",
    lineHeight: "18px",
    minHeight: "26px",
    fontFamily:
      'ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, "Liberation Mono", monospace',
    fontSize: "12px",
    color: "var(--text)",
    backgroundColor: "var(--surface)",
    border: "1px solid var(--border)",
    borderRadius: "5px",
    outline: "none",
    overflowY: "hidden",
  },
  ".cm-pm-field::placeholder": { color: "var(--muted)" },
  ".cm-pm-field:focus": { borderColor: "var(--accent)" },
  ".cm-pm-query": { paddingRight: "82px" },
  ".cm-pm-invalid .cm-pm-query, .cm-pm-invalid .cm-pm-query:focus": { borderColor: "var(--err)" },
  ".cm-pm-noresults .cm-pm-query, .cm-pm-noresults .cm-pm-query:focus": { borderColor: "var(--err)" },
  ".cm-pm-toggles": {
    position: "absolute",
    top: "3px",
    right: "4px",
    display: "flex",
    gap: "2px",
  },
  ".cm-pm-btn": {
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    margin: 0,
    border: "1px solid transparent",
    borderRadius: "4px",
    background: "none",
    color: "var(--muted)",
    cursor: "pointer",
    fontFamily: "inherit",
    fontSize: "12px",
    lineHeight: "1",
  },
  ".cm-pm-btn:hover:not(:disabled)": { color: "var(--text)", backgroundColor: "var(--surface)" },
  ".cm-pm-btn:focus-visible": { outline: "1px solid var(--accent)", outlineOffset: "0" },
  ".cm-pm-btn:disabled": { opacity: "0.4", cursor: "default" },
  ".cm-pm-toggle": {
    minWidth: "24px",
    height: "20px",
    padding: "0 4px",
    fontFamily:
      'ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, "Liberation Mono", monospace',
    fontSize: "11px",
    fontWeight: "600",
  },
  ".cm-pm-toggle.cm-pm-on, .cm-pm-toggle.cm-pm-on:hover:not(:disabled)": {
    color: "var(--accent-text)",
    backgroundColor: "var(--accent)",
    borderColor: "var(--accent)",
  },
  ".cm-pm-icon": { width: "24px", height: "24px", padding: 0, flex: "none" },
  ".cm-pm-expand": { marginTop: "1px" },
  ".cm-pm-expand svg": { transition: "transform 0.12s" },
  ".cm-pm-expand.cm-pm-open svg": { transform: "rotate(90deg)" },
  ".cm-pm-text": {
    height: "26px",
    padding: "0 9px",
    color: "var(--text)",
    backgroundColor: "var(--surface)",
    border: "1px solid var(--border)",
    whiteSpace: "nowrap",
  },
  ".cm-pm-text:hover:not(:disabled)": { borderColor: "var(--accent)" },
  ".cm-pm-text:active:not(:disabled)": { backgroundColor: "var(--accent)", color: "var(--accent-text)" },
  ".cm-pm-actions": { display: "flex", alignItems: "center", gap: "2px", flex: "none", minHeight: "26px" },
  ".cm-pm-counter": {
    flex: "none",
    minWidth: "64px",
    lineHeight: "26px",
    color: "var(--muted)",
    fontVariantNumeric: "tabular-nums",
    whiteSpace: "nowrap",
  },
  ".cm-pm-noresults .cm-pm-counter": { color: "var(--err)" },
  ".cm-pm-close": { marginLeft: "4px" },
  ".cm-pm-error": { display: "none", color: "var(--err)", fontSize: "11px", paddingLeft: "2px" },
  ".cm-pm-invalid .cm-pm-error": { display: "block" },
});
