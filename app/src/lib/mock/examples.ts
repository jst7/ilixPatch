// D62: build an example from a real response ("Save as example").
import { api, type Example, type SendResult } from "$lib/api";

const KEEP_HEADERS = new Set(["content-type", "cache-control", "location", "etag", "link", "www-authenticate", "retry-after"]);
const CHUNK = 1024 * 1024;
/** Bigger bodies are saved truncated in the request file (keeps it reviewable in git). */
const MAX_BODY = 1024 * 1024;

export async function exampleFromResponse(r: SendResult): Promise<Example> {
  let body = r.body;
  const h = r.body_handle;
  if (h && h.size > body.length && h.size <= MAX_BODY && h.is_text !== false) {
    let out = "";
    for (let offset = 0; offset < h.size; ) {
      const c = await api.responseRead(h.id, offset, CHUNK);
      out += c.data;
      if (c.eof || c.length === 0) break;
      offset += c.length;
    }
    body = out;
  }
  const headers: Record<string, string> = {};
  for (const [k, v] of r.headers) if (KEEP_HEADERS.has(k.toLowerCase())) headers[k] = v;
  let pretty = body;
  try {
    pretty = JSON.stringify(JSON.parse(body), null, 2);
  } catch {
    // not JSON: keep as is
  }
  return { name: `${r.status} ${r.status_text}`.trim(), status: r.status, headers, body: pretty };
}
