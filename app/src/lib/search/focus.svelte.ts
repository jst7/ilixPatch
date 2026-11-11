// Requests to focus a search box, bumped by keyboard shortcuts in +page.svelte.
export const searchFocus = $state({ sidebar: 0, tabs: 0 });

const isMac = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);

/** Human-readable shortcut, e.g. shortcutLabel("P") → "⌘P" on macOS, "Ctrl+P" elsewhere. */
export function shortcutLabel(key: string, shift = false): string {
  return isMac ? `${shift ? "⇧" : ""}⌘${key}` : `Ctrl+${shift ? "Shift+" : ""}${key}`;
}
