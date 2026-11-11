// Presentation helpers for git file states (letters and theme colours, IntelliJ style).
import type { GitFileState } from "$lib/api";

const LETTER: Record<GitFileState, string> = {
  modified: "M",
  added: "A",
  deleted: "D",
  renamed: "R",
  untracked: "U",
  conflicted: "C",
};

/** One-letter code shown next to a changed file. */
export function stateLetter(state: GitFileState): string {
  return LETTER[state];
}

/** CSS colour (theme variable) for a git file state. */
export function stateColor(state: GitFileState): string {
  switch (state) {
    case "added":
      return "var(--git-added)";
    case "untracked":
      return "var(--git-untracked)";
    case "deleted":
      return "var(--git-deleted)";
    case "conflicted":
      return "var(--git-conflicted)";
    default:
      return "var(--git-modified)";
  }
}

/** Path of `path` relative to `root` ("" when equal), with "/" separators. */
export function relativePath(path: string, root: string): string {
  if (path === root) return "";
  const rel = path.startsWith(root) ? path.slice(root.length).replace(/^[\\/]+/, "") : path;
  return rel.replace(/\\/g, "/");
}

export function basename(p: string): string {
  return p.split(/[\\/]/).filter(Boolean).pop() ?? p;
}
