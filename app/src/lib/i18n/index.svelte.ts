// Minimal, dependency-free i18n with Svelte 5 runes.
//
// Usage in components:   import { t } from "$lib/i18n/index.svelte";
//                        {t("settings.title")}   {t("tree.deleteConfirm", { name })}
// `t` reads reactive state, so text updates instantly when the locale changes.
//
// Adding a language: create locales/<code>.ts typed as `Messages`, then add it to LOCALES.

import en from "./locales/en";
import es from "./locales/es";

export type MessageKey = keyof typeof en;
export type Messages = Record<MessageKey, string>;

export interface LocaleInfo {
  code: string;
  /** Name of the language in that language, e.g. "Español". */
  label: string;
  messages: Messages;
}

export const LOCALES: LocaleInfo[] = [
  { code: "en", label: "English", messages: en },
  { code: "es", label: "Español", messages: es },
];

const STORAGE_KEY = "ilixpatch.locale";

function detect(): string {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved && LOCALES.some((l) => l.code === saved)) return saved;
  } catch {}
  const nav = typeof navigator !== "undefined" ? navigator.language.slice(0, 2) : "en";
  return LOCALES.some((l) => l.code === nav) ? nav : "en";
}

const state = $state({ locale: detect() });

export const i18n = {
  get locale() {
    return state.locale;
  },
  set locale(code: string) {
    if (!LOCALES.some((l) => l.code === code)) return;
    state.locale = code;
    try {
      localStorage.setItem(STORAGE_KEY, code);
    } catch {}
    if (typeof document !== "undefined") document.documentElement.lang = code;
  },
};

if (typeof document !== "undefined") document.documentElement.lang = state.locale;

export function t(key: MessageKey, params?: Record<string, string | number>): string {
  const messages = LOCALES.find((l) => l.code === state.locale)?.messages ?? en;
  let text: string = messages[key] ?? en[key] ?? key;
  if (params) {
    for (const [k, v] of Object.entries(params)) text = text.replaceAll(`{${k}}`, String(v));
  }
  return text;
}
