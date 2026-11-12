// Progressive loading of lines that `responseLines` returned cut (D41): the line is split into
// contiguous chunks of at most CHUNK_CHARS characters; only one chunk is rendered at a time and
// texts far from it are dropped (re-read by byte range when needed), so a 50 MB single line never
// becomes one string in memory or in the DOM. Pure module: the byte reader is injected.

export const CHUNK_CHARS = 65_536;

/** Reads `length` bytes at absolute byte `offset` of the view; returns UTF-8 text and bytes covered. */
export type ByteReader = (offset: number, length: number) => Promise<{ data: string; length: number }>;

export interface Chunk {
  byteOffset: number;
  byteLength: number;
  /** Character position of the chunk start within the whole line. */
  charStart: number;
  charLength: number;
  /** null when evicted (reload with `loadText`). */
  text: string | null;
}

const enc = new TextEncoder();
export function utf8Length(s: string): number {
  return enc.encode(s).length;
}

/** Split `text` (covering `byteLength` bytes) into ≤ max-char pieces with their byte lengths. */
export function splitText(text: string, byteLength: number, max = CHUNK_CHARS): { text: string; bytes: number }[] {
  if (text.length <= max) return [{ text, bytes: byteLength }];
  const out: { text: string; bytes: number }[] = [];
  let i = 0;
  let used = 0;
  while (i < text.length) {
    let end = Math.min(text.length, i + max);
    // Don't split a surrogate pair.
    if (end < text.length && /[\uDC00-\uDFFF]/.test(text[end])) end--;
    const piece = text.slice(i, end);
    const last = end >= text.length;
    // The last piece takes the remainder so lossy decoding never desynchronises offsets.
    const bytes = last ? byteLength - used : utf8Length(piece);
    out.push({ text: piece, bytes });
    used += bytes;
    i = end;
  }
  return out;
}

export class LongLine {
  readonly lineOffset: number;
  readonly fullBytes: number;
  chunks: Chunk[] = [];
  /** Index of the chunk being displayed. */
  pos = 0;

  constructor(lineOffset: number, fullBytes: number, firstText: string) {
    this.lineOffset = lineOffset;
    this.fullBytes = fullBytes;
    this.append(firstText, utf8Length(firstText));
  }

  /** Bytes of the line covered by loaded (or evicted) chunks. */
  get coveredBytes(): number {
    const last = this.chunks[this.chunks.length - 1];
    return last ? last.byteOffset + last.byteLength - this.lineOffset : 0;
  }

  get remainingBytes(): number {
    return Math.max(0, this.fullBytes - this.coveredBytes);
  }

  get current(): Chunk {
    return this.chunks[this.pos];
  }

  hasPrev(): boolean {
    return this.pos > 0;
  }

  hasNext(): boolean {
    return this.pos < this.chunks.length - 1 || this.remainingBytes > 0;
  }

  /** Add text read right after the covered part. */
  append(text: string, byteLength: number) {
    for (const p of splitText(text, byteLength)) {
      const last = this.chunks[this.chunks.length - 1];
      this.chunks.push({
        byteOffset: last ? last.byteOffset + last.byteLength : this.lineOffset,
        byteLength: p.bytes,
        charStart: last ? last.charStart + last.charLength : 0,
        charLength: p.text.length,
        text: p.text,
      });
    }
  }

  /** Read the next part of the line (`size` bytes). Returns false at the end. */
  async fetchMore(read: ByteReader, size: number): Promise<boolean> {
    const remaining = this.remainingBytes;
    if (remaining <= 0) return false;
    const chunk = await read(this.lineOffset + this.coveredBytes, Math.min(size, remaining));
    if (chunk.length <= 0) return false;
    this.append(chunk.data, chunk.length);
    return true;
  }

  /** Make sure chunk `index` exists and has its text. */
  async ensure(index: number, read: ByteReader, size: number): Promise<boolean> {
    while (index >= this.chunks.length) {
      if (!(await this.fetchMore(read, size))) return false;
    }
    const c = this.chunks[index];
    if (c.text === null) c.text = (await read(c.byteOffset, c.byteLength)).data;
    return true;
  }

  /** Index of the chunk containing character `col`, fetching forward (in `seekSize` reads) as needed; -1 if beyond the line. */
  async seekChar(col: number, read: ByteReader, seekSize: number): Promise<number> {
    for (;;) {
      const idx = this.chunks.findIndex((c) => col >= c.charStart && col < c.charStart + c.charLength);
      if (idx >= 0) return idx;
      // Drop texts while seeking far so memory stays bounded.
      this.evict(2);
      if (!(await this.fetchMore(read, seekSize))) return -1;
    }
  }

  /** Forget texts of chunks more than `keep` away from the displayed one. */
  evict(keep = 4) {
    for (let k = 0; k < this.chunks.length; k++) if (Math.abs(k - this.pos) > keep) this.chunks[k].text = null;
  }
}
