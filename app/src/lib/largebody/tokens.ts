// Lightweight per-line JSON colouring and search-match segmentation (D41). Pure module.

export type TokenKind = "key" | "string" | "number" | "bool" | "null" | "punct" | "plain";

export interface Token {
  text: string;
  kind: TokenKind;
}

// Strings (with escapes), numbers, literals, punctuation; everything else is plain.
const TOKEN_RE = /("(?:[^"\\]|\\.)*"?)|(-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)|\b(true|false)\b|\b(null)\b|([{}[\],:])/g;

/** Split one line of (pretty or raw) JSON into coloured tokens. Never throws; unknown text is "plain". */
export function tokenizeJsonLine(line: string): Token[] {
  const out: Token[] = [];
  let last = 0;
  TOKEN_RE.lastIndex = 0;
  let m: RegExpExecArray | null;
  while ((m = TOKEN_RE.exec(line))) {
    if (m.index > last) out.push({ text: line.slice(last, m.index), kind: "plain" });
    let kind: TokenKind;
    if (m[1] !== undefined) {
      // A string followed (after optional spaces) by ':' is an object key.
      kind = /^\s*:/.test(line.slice(m.index + m[0].length)) ? "key" : "string";
    } else if (m[2] !== undefined) kind = "number";
    else if (m[3] !== undefined) kind = "bool";
    else if (m[4] !== undefined) kind = "null";
    else kind = "punct";
    out.push({ text: m[0], kind });
    last = m.index + m[0].length;
    if (m[0].length === 0) TOKEN_RE.lastIndex++;
  }
  if (last < line.length) out.push({ text: line.slice(last), kind: "plain" });
  return out;
}

export interface Segment {
  text: string;
  kind: TokenKind;
  /** Index into the `matches` given to `segments` when this text is part of a match, else -1. */
  match: number;
}

/**
 * Merge tokens with highlighted ranges ([column, length] pairs, sorted, non-overlapping) into
 * render segments. `json=false` yields a single plain token stream.
 */
export function segments(line: string, json: boolean, matches: { column: number; length: number }[]): Segment[] {
  const tokens = json ? tokenizeJsonLine(line) : [{ text: line, kind: "plain" as TokenKind }];
  if (!matches.length) return tokens.map((t) => ({ ...t, match: -1 }));
  const out: Segment[] = [];
  let pos = 0;
  for (const tok of tokens) {
    const start = pos;
    const end = pos + tok.text.length;
    let cur = start;
    for (let i = 0; i < matches.length && cur < end; i++) {
      const ms = matches[i].column;
      const me = ms + matches[i].length;
      if (me <= cur || ms >= end) continue;
      if (ms > cur) out.push({ text: line.slice(cur, ms), kind: tok.kind, match: -1 });
      const a = Math.max(ms, cur);
      const b = Math.min(me, end);
      out.push({ text: line.slice(a, b), kind: tok.kind, match: i });
      cur = b;
    }
    if (cur < end) out.push({ text: line.slice(cur, end), kind: tok.kind, match: -1 });
    pos = end;
  }
  return out;
}
