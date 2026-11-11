// Theme system. Every colour in the UI comes from a CSS custom property
// `--<token>` set on <html>. Components must never hard-code colours.
//
// - Built-in themes: light, dark (read-only).
// - Users can duplicate a theme, edit any token, export/import it as JSON.
// - "system" follows the OS light/dark preference live.
// - <html data-theme="light|dark"> exposes the base so CSS can adapt shadows etc.

export const TOKENS = [
  // Interface
  "bg", "surface", "surface-2", "border", "text", "muted", "accent", "accent-text", "selection",
  // Status
  "ok", "warn", "err",
  // HTTP methods
  "method-get", "method-post", "method-put", "method-patch", "method-delete", "method-other",
  // Syntax (editor)
  "syntax-key", "syntax-string", "syntax-number", "syntax-bool", "syntax-null", "syntax-punct",
  "syntax-keyword", "syntax-comment", "syntax-function",
  "active-line", "search-match",
  // Git (tree status colours and diffs)
  "git-modified", "git-added", "git-untracked", "git-deleted", "git-conflicted", "diff-added-bg", "diff-removed-bg",
  // Git annotate (blame) heat colours
  "blame-recent", "blame-old", "blame-text",
  // Merge resolver and side-by-side diff (D24)
  "merge-changed-bg", "merge-added-bg", "merge-removed-bg", "merge-conflict-bg", "merge-resolved-bg", "merge-word-bg",
] as const;

export type Token = (typeof TOKENS)[number];
export type Palette = Record<Token, string>;

export const TOKEN_GROUPS: { id: "base" | "status" | "methods" | "syntax" | "git" | "merge"; tokens: Token[] }[] = [
  { id: "base", tokens: ["bg", "surface", "surface-2", "border", "text", "muted", "accent", "accent-text", "selection"] },
  { id: "status", tokens: ["ok", "warn", "err"] },
  { id: "methods", tokens: ["method-get", "method-post", "method-put", "method-patch", "method-delete", "method-other"] },
  { id: "syntax", tokens: ["syntax-key", "syntax-string", "syntax-number", "syntax-bool", "syntax-null", "syntax-punct", "syntax-keyword", "syntax-comment", "syntax-function", "active-line", "search-match"] },
  { id: "git", tokens: ["git-modified", "git-added", "git-untracked", "git-deleted", "git-conflicted", "diff-added-bg", "diff-removed-bg", "blame-recent", "blame-old", "blame-text"] },
  { id: "merge", tokens: ["merge-changed-bg", "merge-added-bg", "merge-removed-bg", "merge-conflict-bg", "merge-resolved-bg", "merge-word-bg"] },
];

export interface Theme {
  id: string;
  name: string;
  base: "light" | "dark";
  builtin?: boolean;
  colors: Palette;
}

const LIGHT: Palette = {
  bg: "#f4f4f6", surface: "#ffffff", "surface-2": "#f8f8fa", border: "#e2e2e7",
  text: "#1d1d22", muted: "#6b6b76", accent: "#0b8a5c", "accent-text": "#ffffff", selection: "#cdf3e2",
  ok: "#2b8a3e", warn: "#e67700", err: "#e03131",
  "method-get": "#2b8a3e", "method-post": "#e67700", "method-put": "#1c7ed6",
  "method-patch": "#7048e8", "method-delete": "#e03131", "method-other": "#6b6b76",
  "syntax-key": "#a61e4d", "syntax-string": "#2b8a3e", "syntax-number": "#1c7ed6",
  "syntax-bool": "#7048e8", "syntax-null": "#868e96", "syntax-punct": "#495057",
  "syntax-keyword": "#9c36b5", "syntax-comment": "#868e96", "syntax-function": "#1971c2",
  "active-line": "#f1f3f5", "search-match": "#ffe066",
  "git-modified": "#1c6fd1", "git-added": "#2b8a3e", "git-untracked": "#b4502a", "git-deleted": "#868e96",
  "git-conflicted": "#e03131", "diff-added-bg": "#e3f6e7", "diff-removed-bg": "#fde8e8",
  "blame-recent": "#b9f0d6", "blame-old": "#f8f8fa", "blame-text": "#495057",
  "merge-changed-bg": "#dde9fb", "merge-added-bg": "#dcf3e1", "merge-removed-bg": "#e9e9ed",
  "merge-conflict-bg": "#fbdcdc", "merge-resolved-bg": "#f1f1f4", "merge-word-bg": "#a9c8f5",
};

const DARK: Palette = {
  bg: "#141418", surface: "#1d1d22", "surface-2": "#24242a", border: "#2f2f37",
  text: "#ececf1", muted: "#8e8e99", accent: "#20c997", "accent-text": "#0b1f17", selection: "#12392e",
  ok: "#51cf66", warn: "#fcc419", err: "#ff6b6b",
  "method-get": "#51cf66", "method-post": "#fcc419", "method-put": "#4dabf7",
  "method-patch": "#b197fc", "method-delete": "#ff6b6b", "method-other": "#adb5bd",
  "syntax-key": "#f783ac", "syntax-string": "#8ce99a", "syntax-number": "#74c0fc",
  "syntax-bool": "#b197fc", "syntax-null": "#868e96", "syntax-punct": "#adb5bd",
  "syntax-keyword": "#e599f7", "syntax-comment": "#7a7a85", "syntax-function": "#66d9e8",
  "active-line": "#26262d", "search-match": "#5c4a00",
  "git-modified": "#6ea8fe", "git-added": "#69db7c", "git-untracked": "#e8875b", "git-deleted": "#868e96",
  "git-conflicted": "#ff6b6b", "diff-added-bg": "#15331f", "diff-removed-bg": "#3a1b1e",
  "blame-recent": "#16473a", "blame-old": "#24242a", "blame-text": "#c1c2c5",
  "merge-changed-bg": "#1d2e48", "merge-added-bg": "#183323", "merge-removed-bg": "#2c2c33",
  "merge-conflict-bg": "#45222a", "merge-resolved-bg": "#202026", "merge-word-bg": "#2f4f80",
};

export const BUILTIN_THEMES: Theme[] = [
  { id: "light", name: "Light", base: "light", builtin: true, colors: LIGHT },
  { id: "dark", name: "Dark", base: "dark", builtin: true, colors: DARK },
];

const STORAGE_KEY = "ilixpatch.theme";

interface Persisted {
  selected: string; // "system" | theme id
  custom: Theme[];
}

function load(): Persisted {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) {
      const p = JSON.parse(raw) as Persisted;
      return { selected: p.selected ?? "system", custom: (p.custom ?? []).map(normalize) };
    }
  } catch {}
  return { selected: "system", custom: [] };
}

/** Fill missing tokens from the matching built-in so old/imported themes keep working. */
function normalize(t: Theme): Theme {
  const fallback = t.base === "dark" ? DARK : LIGHT;
  return { ...t, builtin: false, colors: { ...fallback, ...t.colors } };
}

const state = $state({ ...load(), systemDark: false });

function persist() {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ selected: state.selected, custom: state.custom }));
  } catch {}
}

export const themes = {
  get all(): Theme[] {
    return [...BUILTIN_THEMES, ...state.custom];
  },
  get selected(): string {
    return state.selected;
  },
  /** The theme actually applied (resolves "system"). */
  get active(): Theme {
    const id = state.selected === "system" ? (state.systemDark ? "dark" : "light") : state.selected;
    return this.all.find((t) => t.id === id) ?? BUILTIN_THEMES[state.systemDark ? 1 : 0];
  },
  select(id: string) {
    state.selected = id;
    persist();
  },
  /** Copy any theme into a new editable custom theme and select it. */
  duplicate(id: string, name: string): Theme {
    const src = this.all.find((t) => t.id === id) ?? this.active;
    const copy: Theme = { id: `custom-${Date.now().toString(36)}`, name, base: src.base, colors: { ...src.colors } };
    state.custom.push(copy);
    state.selected = copy.id;
    persist();
    return copy;
  },
  update(id: string, patch: Partial<Pick<Theme, "name" | "base">> & { colors?: Partial<Palette> }) {
    const t = state.custom.find((c) => c.id === id);
    if (!t) return; // built-ins are read-only
    if (patch.name !== undefined) t.name = patch.name;
    if (patch.base !== undefined) t.base = patch.base;
    if (patch.colors) Object.assign(t.colors, patch.colors);
    persist();
  },
  /** Reset a custom theme's colours to its base built-in. */
  reset(id: string) {
    const t = state.custom.find((c) => c.id === id);
    if (!t) return;
    t.colors = { ...(t.base === "dark" ? DARK : LIGHT) };
    persist();
  },
  remove(id: string) {
    state.custom = state.custom.filter((c) => c.id !== id);
    if (state.selected === id) state.selected = "system";
    persist();
  },
  export(id: string): string {
    const t = this.all.find((x) => x.id === id);
    if (!t) throw new Error(`unknown theme ${id}`);
    const { builtin: _, ...rest } = t;
    return JSON.stringify({ ...rest, id: undefined }, null, 2);
  },
  /** Import a theme JSON (as produced by export). Throws on invalid input. */
  import(json: string): Theme {
    const data = JSON.parse(json);
    if (typeof data?.name !== "string" || (data.base !== "light" && data.base !== "dark") || typeof data.colors !== "object") {
      throw new Error("invalid theme");
    }
    const colors = Object.fromEntries(
      Object.entries(data.colors).filter(([k, v]) => (TOKENS as readonly string[]).includes(k) && typeof v === "string"),
    ) as Partial<Palette>;
    const theme = normalize({ id: `custom-${Date.now().toString(36)}`, name: data.name, base: data.base, colors: colors as Palette });
    state.custom.push(theme);
    state.selected = theme.id;
    persist();
    return theme;
  },
};

/** Apply the active theme to <html>. Call once from the root layout. */
export function initTheme() {
  const mq = window.matchMedia("(prefers-color-scheme: dark)");
  state.systemDark = mq.matches;
  mq.addEventListener("change", (e) => (state.systemDark = e.matches));

  $effect.root(() => {
    $effect(() => {
      const theme = themes.active;
      const root = document.documentElement;
      for (const token of TOKENS) root.style.setProperty(`--${token}`, theme.colors[token]);
      root.dataset.theme = theme.base;
      root.style.colorScheme = theme.base;
    });
  });
}
