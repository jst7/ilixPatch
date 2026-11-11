import { EditorState, type Extension } from "@codemirror/state";
import {
  EditorView,
  keymap,
  lineNumbers,
  highlightActiveLine,
  highlightActiveLineGutter,
  highlightSpecialChars,
  drawSelection,
  placeholder as placeholderExt,
} from "@codemirror/view";
import {
  HighlightStyle,
  syntaxHighlighting,
  bracketMatching,
  codeFolding,
  foldGutter,
  foldKeymap,
  indentOnInput,
} from "@codemirror/language";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { search, searchKeymap, highlightSelectionMatches } from "@codemirror/search";
import { createFindPanel, findKeymap, findPanelTheme } from "./searchPanel";
import {
  autocompletion,
  closeBrackets,
  closeBracketsKeymap,
  completionKeymap,
  type Completion,
  type CompletionContext,
  type CompletionResult,
} from "@codemirror/autocomplete";
import { json } from "@codemirror/lang-json";
import { javascript, javascriptLanguage } from "@codemirror/lang-javascript";
import { tags as t } from "@lezer/highlight";
import { t as tr, type MessageKey } from "$lib/i18n/index.svelte";

export type EditorLanguage = "json" | "text" | "javascript";

// ---------- Syntax colours (all from theme CSS vars) ----------

const highlight = HighlightStyle.define([
  { tag: t.propertyName, color: "var(--syntax-key)" },
  { tag: t.string, color: "var(--syntax-string)" },
  { tag: t.number, color: "var(--syntax-number)" },
  { tag: t.bool, color: "var(--syntax-bool)", fontWeight: "500" },
  { tag: t.null, color: "var(--syntax-null)", fontStyle: "italic" },
  { tag: [t.brace, t.squareBracket, t.separator, t.punctuation], color: "var(--syntax-punct)" },
  { tag: [t.keyword, t.controlKeyword, t.definitionKeyword, t.moduleKeyword, t.operatorKeyword, t.self], color: "var(--syntax-keyword)" },
  { tag: [t.comment, t.lineComment, t.blockComment], color: "var(--syntax-comment)", fontStyle: "italic" },
  { tag: [t.function(t.variableName), t.function(t.propertyName), t.function(t.definition(t.variableName))], color: "var(--syntax-function)" },
  { tag: [t.operator, t.arithmeticOperator, t.logicOperator, t.compareOperator, t.updateOperator, t.definitionOperator], color: "var(--syntax-punct)" },
  { tag: [t.regexp, t.special(t.string)], color: "var(--syntax-string)" },
  { tag: t.invalid, color: "var(--err)", textDecoration: "underline wavy" },
]);

// ---------- Editor chrome (all from theme CSS vars) ----------

const chrome = EditorView.theme({
  "&": {
    height: "100%",
    color: "var(--text)",
    backgroundColor: "var(--surface)",
    fontSize: "13px",
  },
  "&.cm-focused": { outline: "none" },
  ".cm-scroller": {
    fontFamily:
      'ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, "Liberation Mono", monospace',
    lineHeight: "1.5",
    overflow: "auto",
  },
  ".cm-content": { caretColor: "var(--accent)", padding: "6px 0" },
  ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--accent)", borderLeftWidth: "2px" },
  "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection":
    { backgroundColor: "var(--selection)" },
  ".cm-gutters": {
    backgroundColor: "var(--surface-2)",
    color: "var(--muted)",
    borderRight: "1px solid var(--border)",
  },
  ".cm-lineNumbers .cm-gutterElement": { padding: "0 8px 0 12px", minWidth: "28px" },
  ".cm-activeLine": { backgroundColor: "var(--active-line)" },
  ".cm-activeLineGutter": { backgroundColor: "var(--active-line)", color: "var(--text)" },
  ".cm-placeholder": { color: "var(--muted)", fontStyle: "italic" },
  ".cm-matchingBracket, &.cm-focused .cm-matchingBracket": {
    backgroundColor: "var(--selection)",
    outline: "1px solid var(--muted)",
  },
  ".cm-nonmatchingBracket, &.cm-focused .cm-nonmatchingBracket": { color: "var(--err)" },
  ".cm-selectionMatch": { backgroundColor: "var(--search-match)" },
  ".cm-searchMatch": { backgroundColor: "var(--search-match)", borderRadius: "2px" },
  ".cm-searchMatch.cm-searchMatch-selected": {
    backgroundColor: "color-mix(in srgb, var(--accent) 40%, transparent)",
    outline: "1px solid var(--accent)",
  },

  // Folding
  ".cm-foldGutter .cm-gutterElement": {
    padding: "0 4px",
    cursor: "pointer",
    color: "var(--muted)",
  },
  ".cm-foldGutter .cm-gutterElement:hover": { color: "var(--text)" },
  ".cm-foldGutter .cm-gutterElement span": { fontSize: "10px" },
  ".cm-foldPlaceholder": {
    display: "inline-block",
    margin: "0 1px",
    padding: "0 5px",
    border: "1px solid var(--border)",
    borderRadius: "4px",
    backgroundColor: "var(--surface-2)",
    color: "var(--muted)",
    fontSize: "11px",
    lineHeight: "1.4",
    cursor: "pointer",
  },
  ".cm-foldPlaceholder:hover": { color: "var(--text)", borderColor: "var(--accent)" },

  // Search panel
  ".cm-panels": { backgroundColor: "var(--surface-2)", color: "var(--text)" },
  ".cm-panels.cm-panels-top": { borderBottom: "1px solid var(--border)" },
  ".cm-panels.cm-panels-bottom": { borderTop: "1px solid var(--border)" },
  ".cm-tooltip": {
    backgroundColor: "var(--surface)",
    color: "var(--text)",
    border: "1px solid var(--border)",
  },
  ".cm-tooltip.cm-tooltip-autocomplete": {
    borderRadius: "6px",
    boxShadow: "var(--shadow)",
    overflow: "hidden",
  },
  ".cm-tooltip.cm-tooltip-autocomplete > ul": {
    fontFamily:
      'ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, "Liberation Mono", monospace',
    fontSize: "12px",
    maxHeight: "16em",
  },
  ".cm-tooltip.cm-tooltip-autocomplete > ul > li": { padding: "2px 8px 2px 4px" },
  ".cm-tooltip.cm-tooltip-autocomplete > ul > li[aria-selected]": {
    backgroundColor: "var(--selection)",
    color: "var(--text)",
  },
  ".cm-completionDetail": { color: "var(--muted)", fontStyle: "normal", marginLeft: "8px" },
  ".cm-completionMatchedText": { textDecoration: "none", color: "var(--accent)", fontWeight: "600" },
  ".cm-completionIcon": { color: "var(--muted)", opacity: "1" },
  ".cm-tooltip.cm-completionInfo": {
    backgroundColor: "var(--surface-2)",
    borderRadius: "6px",
    padding: "4px 8px",
    fontSize: "12px",
  },
});

// ---------- pm.* API autocompletion (JavaScript scripts) ----------

interface ApiNode {
  /** Completion type: function, property, variable, namespace… */
  type: string;
  detail?: string;
  /** Text inserted (defaults to the member name). */
  apply?: string;
  children?: Record<string, ApiNode>;
}

const fn = (detail: string, apply?: string): ApiNode => ({ type: "function", detail, apply });
const prop = (detail?: string, children?: Record<string, ApiNode>): ApiNode => ({
  type: children ? "namespace" : "property",
  detail,
  children,
});

const scopeApi = (): Record<string, ApiNode> => ({
  get: fn("(key)"),
  set: fn("(key, value)"),
  unset: fn("(key)"),
  has: fn("(key)"),
  clear: fn("()"),
  toObject: fn("()"),
  replaceIn: fn("(template)"),
});

const assertions: Record<string, ApiNode> = {
  status: fn("(code)"),
  header: fn("(name, value?)"),
  body: fn("(text?)"),
  jsonBody: fn("(path?, value?)"),
  responseTime: fn("(ms)"),
};

const PM_API: Record<string, ApiNode> = {
  pm: prop("pm", {
    test: fn("(name, fn)", 'test("", function () {\n  \n});'),
    expect: fn("(value)"),
    response: prop("response", {
      json: fn("()"),
      text: fn("()"),
      code: prop("number"),
      status: prop("string"),
      responseTime: prop("number"),
      responseSize: prop("number"),
      headers: prop("headers", { get: fn("(name)"), has: fn("(name)"), toObject: fn("()") }),
      to: prop("assertions", {
        have: prop("assertions", assertions),
        be: prop("assertions", {
          ok: prop("2xx"),
          success: prop("2xx"),
          error: prop("4xx/5xx"),
          clientError: prop("4xx"),
          serverError: prop("5xx"),
          notFound: prop("404"),
          json: prop("json"),
        }),
        not: prop("negate", { have: prop("assertions", assertions) }),
      }),
    }),
    request: prop("request", {
      url: prop("string"),
      method: prop("string"),
      headers: prop("headers", { get: fn("(name)"), add: fn("({ key, value })"), upsert: fn("({ key, value })"), remove: fn("(name)") }),
      body: prop("body"),
    }),
    environment: prop("environment", { ...scopeApi(), name: prop("string") }),
    variables: prop("variables", scopeApi()),
    collectionVariables: prop("collection variables", scopeApi()),
    globals: prop("globals", scopeApi()),
    iterationData: prop("data row", { get: fn("(key)"), has: fn("(key)"), toObject: fn("()") }),
    info: prop("info", {
      requestName: prop("string"),
      iteration: prop("number"),
      iterationCount: prop("number"),
      eventName: prop("string"),
    }),
  }),
  console: prop("console", { log: fn("(...args)"), info: fn("(...args)"), warn: fn("(...args)"), error: fn("(...args)") }),
  JSON: prop("JSON", { parse: fn("(text)"), stringify: fn("(value, null, 2)") }),
};

/** Chai-style chain members offered after pm.expect(...). */
const EXPECT_CHAIN: Completion[] = [
  "to", "be", "been", "is", "that", "which", "and", "has", "have", "with", "not", "deep",
  "equal", "eql", "include", "a", "an", "oneOf", "above", "below", "least", "most",
  "property", "lengthOf", "match", "ok", "true", "false", "null", "undefined", "empty", "exist",
].map((label) => ({ label, type: "property" }));

function toOptions(children: Record<string, ApiNode>): Completion[] {
  return Object.entries(children).map(([label, n]) => ({
    label,
    type: n.type,
    detail: n.type === "namespace" ? "{…}" : n.detail,
    apply: n.apply,
    boost: n.type === "namespace" ? 1 : 0,
  }));
}

/** Completion source for the pm.* script API. */
export function pmCompletions(ctx: CompletionContext): CompletionResult | null {
  // After pm.expect(...)  →  chai chain words
  const chain = ctx.matchBefore(/\bexpect\([^()]*(?:\([^()]*\))?[^()]*\)(?:\.\w+)*\.\w*$/);
  if (chain) {
    const dot = chain.text.lastIndexOf(".");
    return { from: chain.from + dot + 1, options: EXPECT_CHAIN, validFor: /^\w*$/ };
  }
  const word = ctx.matchBefore(/[\w$]+(?:\.[\w$]*)*$/);
  if (!word) return ctx.explicit ? { from: ctx.pos, options: toOptions(PM_API) } : null;
  const parts = word.text.split(".");
  if (parts.length === 1) {
    if (word.from === word.to && !ctx.explicit) return null;
    return { from: word.from, options: toOptions(PM_API), validFor: /^[\w$]*$/ };
  }
  let node: Record<string, ApiNode> | undefined = PM_API;
  for (const part of parts.slice(0, -1)) node = node?.[part]?.children;
  if (!node) return null;
  const last = parts[parts.length - 1];
  return { from: word.to - last.length, options: toOptions(node), validFor: /^[\w$]*$/ };
}

const jsExtensions: Extension = [
  javascript(),
  javascriptLanguage.data.of({ autocomplete: pmCompletions }),
  autocompletion({ icons: true }),
  keymap.of(completionKeymap),
];

// ---------- Localised CodeMirror phrases ----------

/** Maps CodeMirror's built-in English phrases to our i18n keys. */
const PHRASE_KEYS: Record<string, MessageKey> = {
  Find: "editor.find",
  Replace: "editor.replace",
  next: "editor.next",
  previous: "editor.previous",
  all: "editor.all",
  "match case": "editor.matchCase",
  regexp: "editor.regexp",
  "by word": "editor.byWord",
  replace: "editor.replaceOne",
  "replace all": "editor.replaceAll",
  close: "editor.close",
  "Go to line": "editor.goToLine",
  go: "editor.go",
  "current match": "editor.currentMatch",
  "on line": "editor.onLine",
  "replaced match on line $": "editor.replacedMatchOnLine",
  "replaced $ matches": "editor.replacedMatches",
  "folded code": "editor.foldedCode",
  unfold: "editor.unfold",
  "Fold line": "editor.foldLine",
  "Unfold line": "editor.unfoldLine",
  "Folded lines": "editor.foldedLines",
  "Unfolded lines": "editor.unfoldedLines",
  to: "editor.to",
  "Control character": "editor.controlCharacter",
};

/** Builds the phrases facet for the current locale (call inside a reactive context to track it). */
export function phrasesExtension(): Extension {
  const phrases: Record<string, string> = {};
  for (const [cm, key] of Object.entries(PHRASE_KEYS)) phrases[cm] = tr(key);
  return EditorState.phrases.of(phrases);
}

export function themeExtension(): Extension {
  return [chrome, findPanelTheme, syntaxHighlighting(highlight)];
}

export function languageExtension(lang: EditorLanguage): Extension {
  if (lang === "json") return json();
  if (lang === "javascript") return jsExtensions;
  return [];
}

/** Extensions toggled by the `readonly` prop. */
export function modeExtension(readonly: boolean): Extension {
  if (readonly) {
    return [
      EditorState.readOnly.of(true),
      EditorView.editable.of(false),
      // Keep the content focusable so keyboard selection, Cmd+F and copy work.
      EditorView.contentAttributes.of({ tabindex: "0" }),
    ];
  }
  return [
    history(),
    closeBrackets(),
    indentOnInput(),
    highlightActiveLine(),
    highlightActiveLineGutter(),
    keymap.of([...closeBracketsKeymap, ...historyKeymap, indentWithTab]),
  ];
}

export function placeholderExtension(text: string | undefined): Extension {
  return text ? placeholderExt(text) : [];
}

/** Static extensions shared by every editor instance. */
export function staticExtensions(): Extension {
  return [
    lineNumbers(),
    highlightSpecialChars(),
    drawSelection(),
    bracketMatching(),
    codeFolding({
      placeholderDOM(view, onclick) {
        const el = document.createElement("span");
        el.className = "cm-foldPlaceholder";
        el.textContent = "…";
        el.title = view.state.phrase("unfold");
        el.setAttribute("aria-label", view.state.phrase("folded code"));
        el.onclick = onclick;
        return el;
      },
    }),
    // Default markers carry localised "Fold line"/"Unfold line" titles via phrases.
    foldGutter({ openText: "▾", closedText: "▸" }),
    // Custom panel (D23): multi-line textareas, regex/case/word toggles, counter, replace.
    search({ top: true, literal: true, createPanel: createFindPanel }),
    highlightSelectionMatches(),
    keymap.of([...findKeymap, ...searchKeymap, ...foldKeymap, ...defaultKeymap]),
  ];
}
