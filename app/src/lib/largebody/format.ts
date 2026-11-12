// Formatting helpers for large / binary bodies (D41). Pure module.

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v >= 100 ? v.toFixed(0) : v >= 10 ? v.toFixed(1) : v.toFixed(2)} ${units[i]}`;
}

export function base64ToBytes(b64: string): Uint8Array {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

/** Classic hex dump: "00000000  48 65 6c 6c 6f …  |Hello…|", 16 bytes per line. */
export function hexDump(bytes: Uint8Array, baseOffset = 0): string[] {
  const lines: string[] = [];
  for (let i = 0; i < bytes.length; i += 16) {
    const row = bytes.subarray(i, Math.min(i + 16, bytes.length));
    let hex = "";
    let ascii = "";
    for (let j = 0; j < 16; j++) {
      if (j < row.length) {
        hex += row[j].toString(16).padStart(2, "0") + " ";
        ascii += row[j] >= 0x20 && row[j] < 0x7f ? String.fromCharCode(row[j]) : ".";
      } else hex += "   ";
      if (j === 7) hex += " ";
    }
    lines.push(`${(baseOffset + i).toString(16).padStart(8, "0")}  ${hex} |${ascii}|`);
  }
  return lines;
}

/** Cut very long lines for rendering: returns the visible part and how many chars were hidden. */
export function clipLine(line: string, max: number): { text: string; hidden: number } {
  return line.length <= max ? { text: line, hidden: 0 } : { text: line.slice(0, max), hidden: line.length - max };
}

/** Group search matches by line (input sorted by line then column). */
export function matchesByLine<T extends { line: number }>(matches: T[]): Map<number, T[]> {
  const map = new Map<number, T[]>();
  for (const m of matches) {
    const list = map.get(m.line);
    if (list) list.push(m);
    else map.set(m.line, [m]);
  }
  return map;
}

/** Map a scroll position to the first visible line, supporting scaled (very tall) scroll areas. */
export function firstLineAt(scrollTop: number, viewH: number, rowH: number, total: number, maxPx: number): number {
  const full = total * rowH;
  if (full <= maxPx) return Math.max(0, Math.floor(scrollTop / rowH));
  const visible = Math.ceil(viewH / rowH);
  const range = Math.max(1, maxPx - viewH);
  return Math.max(0, Math.min(total - visible, Math.round((scrollTop / range) * (total - visible))));
}

/** Inverse of firstLineAt: scrollTop that shows `line` at the top. */
export function scrollTopFor(line: number, viewH: number, rowH: number, total: number, maxPx: number): number {
  const full = total * rowH;
  if (full <= maxPx) return line * rowH;
  const visible = Math.ceil(viewH / rowH);
  const range = Math.max(1, maxPx - viewH);
  return Math.round((Math.min(line, total - visible) / Math.max(1, total - visible)) * range);
}
