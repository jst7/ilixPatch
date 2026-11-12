// Response previews (D44): which bodies can be shown rendered (image, PDF, audio, video, HTML)
// besides the hex/text view. Decided by Content-Type, else by the first bytes (magic numbers).

export type PreviewKind = "image" | "pdf" | "audio" | "video" | "html";

export interface PreviewType {
  kind: PreviewKind;
  /** MIME type for the Blob (the webview needs it to pick a decoder). */
  mime: string;
  /** File extension for "Open with system app". */
  ext: string;
}

const BY_MIME: Record<string, [PreviewKind, string]> = {
  "image/png": ["image", "png"],
  "image/apng": ["image", "png"],
  "image/jpeg": ["image", "jpg"],
  "image/jpg": ["image", "jpg"],
  "image/pjpeg": ["image", "jpg"],
  "image/gif": ["image", "gif"],
  "image/webp": ["image", "webp"],
  "image/avif": ["image", "avif"],
  "image/bmp": ["image", "bmp"],
  "image/x-ms-bmp": ["image", "bmp"],
  "image/x-icon": ["image", "ico"],
  "image/vnd.microsoft.icon": ["image", "ico"],
  "image/svg+xml": ["image", "svg"],
  "application/pdf": ["pdf", "pdf"],
  "application/x-pdf": ["pdf", "pdf"],
  "audio/mpeg": ["audio", "mp3"],
  "audio/mp3": ["audio", "mp3"],
  "audio/wav": ["audio", "wav"],
  "audio/x-wav": ["audio", "wav"],
  "audio/wave": ["audio", "wav"],
  "audio/ogg": ["audio", "ogg"],
  "audio/opus": ["audio", "opus"],
  "audio/aac": ["audio", "aac"],
  "audio/mp4": ["audio", "m4a"],
  "audio/x-m4a": ["audio", "m4a"],
  "audio/flac": ["audio", "flac"],
  "audio/x-flac": ["audio", "flac"],
  "audio/webm": ["audio", "webm"],
  "video/mp4": ["video", "mp4"],
  "video/webm": ["video", "webm"],
  "video/ogg": ["video", "ogv"],
  "video/quicktime": ["video", "mov"],
  "text/html": ["html", "html"],
  "application/xhtml+xml": ["html", "html"],
};

/** Content-Type without parameters, lower case. */
export function baseMime(contentType: string | null | undefined): string {
  return (contentType ?? "").split(";")[0].trim().toLowerCase();
}

export function previewFromMime(contentType: string | null | undefined): PreviewType | null {
  const mime = baseMime(contentType);
  const hit = BY_MIME[mime];
  return hit ? { kind: hit[0], mime, ext: hit[1] } : null;
}

const ascii = (b: Uint8Array, at: number, s: string) =>
  b.length >= at + s.length && [...s].every((c, i) => b[at + i] === c.charCodeAt(0));

/** Recognise a body by its first bytes (for application/octet-stream or a missing Content-Type). */
export function sniffPreview(b: Uint8Array): PreviewType | null {
  const t = (kind: PreviewKind, mime: string, ext: string): PreviewType => ({ kind, mime, ext });
  if (b[0] === 0x89 && ascii(b, 1, "PNG")) return t("image", "image/png", "png");
  if (b[0] === 0xff && b[1] === 0xd8 && b[2] === 0xff) return t("image", "image/jpeg", "jpg");
  if (ascii(b, 0, "GIF87a") || ascii(b, 0, "GIF89a")) return t("image", "image/gif", "gif");
  if (ascii(b, 0, "RIFF") && ascii(b, 8, "WEBP")) return t("image", "image/webp", "webp");
  if (ascii(b, 0, "RIFF") && ascii(b, 8, "WAVE")) return t("audio", "audio/wav", "wav");
  if (ascii(b, 0, "BM") && b.length > 14) return t("image", "image/bmp", "bmp");
  if (b[0] === 0 && b[1] === 0 && b[2] === 1 && b[3] === 0) return t("image", "image/x-icon", "ico");
  if (ascii(b, 0, "%PDF-")) return t("pdf", "application/pdf", "pdf");
  if (ascii(b, 0, "OggS")) return t("audio", "audio/ogg", "ogg");
  if (ascii(b, 0, "fLaC")) return t("audio", "audio/flac", "flac");
  if (ascii(b, 0, "ID3") || (b[0] === 0xff && (b[1] & 0xe0) === 0xe0 && b.length > 3)) {
    return t("audio", "audio/mpeg", "mp3");
  }
  if (b[0] === 0x1a && b[1] === 0x45 && b[2] === 0xdf && b[3] === 0xa3) return t("video", "video/webm", "webm");
  if (ascii(b, 4, "ftyp")) {
    if (ascii(b, 8, "avif") || ascii(b, 8, "avis")) return t("image", "image/avif", "avif");
    if (ascii(b, 8, "M4A ")) return t("audio", "audio/mp4", "m4a");
    if (ascii(b, 8, "qt  ")) return t("video", "video/quicktime", "mov");
    return t("video", "video/mp4", "mp4");
  }
  // SVG served as text/plain or octet-stream.
  const head = new TextDecoder().decode(b.subarray(0, 256)).trimStart().toLowerCase();
  if (head.startsWith("<svg") || (head.startsWith("<?xml") && head.includes("<svg"))) {
    return t("image", "image/svg+xml", "svg");
  }
  return null;
}

/** MIME types that only need sniffing (the Content-Type says nothing useful). */
export function isGenericMime(contentType: string | null | undefined): boolean {
  const m = baseMime(contentType);
  return m === "" || m === "application/octet-stream" || m === "binary/octet-stream" || m === "application/binary";
}
