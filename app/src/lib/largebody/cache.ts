// LRU cache of fixed-size line windows for the virtualized large-body viewer (D41).
// Pure module: no Svelte / Tauri imports (tested by selftest.mjs).

export class LineWindowCache {
  readonly windowSize: number;
  readonly maxWindows: number;
  /** Insertion order = recency (Map keeps it; re-set on access). */
  private windows = new Map<number, string[]>();

  constructor(windowSize = 500, maxWindows = 40) {
    this.windowSize = windowSize;
    this.maxWindows = maxWindows;
  }

  windowOf(line: number): number {
    return Math.floor(line / this.windowSize);
  }

  /** The line's text, or undefined when its window isn't loaded. Marks the window as recently used. */
  get(line: number): string | undefined {
    const w = this.windowOf(line);
    const lines = this.windows.get(w);
    if (!lines) return undefined;
    this.windows.delete(w);
    this.windows.set(w, lines);
    return lines[line - w * this.windowSize];
  }

  has(window: number): boolean {
    return this.windows.has(window);
  }

  set(window: number, lines: string[]) {
    this.windows.delete(window);
    this.windows.set(window, lines);
    while (this.windows.size > this.maxWindows) {
      const oldest = this.windows.keys().next().value as number;
      this.windows.delete(oldest);
    }
  }

  /** Windows covering lines [from, to] (inclusive, clamped to [0, total)) that are not cached. */
  missing(from: number, to: number, total: number): number[] {
    if (total <= 0) return [];
    const a = this.windowOf(Math.max(0, from));
    const b = this.windowOf(Math.min(total - 1, Math.max(0, to)));
    const out: number[] = [];
    for (let w = a; w <= b; w++) if (!this.windows.has(w)) out.push(w);
    return out;
  }

  clear() {
    this.windows.clear();
  }

  get size(): number {
    return this.windows.size;
  }
}
