// URL bar highlighting: split a (possibly templated) URL into coloured parts.
// `{{name}}` variables are tokens of their own; the rest is classified by where it sits
// (scheme, host, port, path, :pathParam, query key/value, fragment).

export type UrlTokKind =
  | "scheme"
  | "host"
  | "port"
  | "path"
  | "param"
  | "punct"
  | "qkey"
  | "qval"
  | "fragment"
  | "var";

export interface UrlTok {
  text: string;
  kind: UrlTokKind;
  /** Offset in the URL. */
  start: number;
  /** Variable name (trimmed) for `var` tokens. */
  name?: string;
}

type State = "host" | "port" | "path" | "qkey" | "qval" | "fragment";

const SCHEME = /^[a-zA-Z][a-zA-Z0-9+.-]*:\/\//;
const PARAM_CHAR = /[A-Za-z0-9_-]/;

export function tokenizeUrl(url: string): UrlTok[] {
  const out: UrlTok[] = [];
  const push = (kind: UrlTokKind, text: string, start: number, name?: string) => {
    const last = out[out.length - 1];
    if (last && last.kind === kind && kind !== "var" && last.start + last.text.length === start) last.text += text;
    else out.push(name === undefined ? { kind, text, start } : { kind, text, start, name });
  };

  let i = 0;
  let state: State = "host";
  const m = SCHEME.exec(url);
  if (m) {
    push("scheme", m[0], 0);
    i = m[0].length;
  } else if (url.startsWith("/")) {
    state = "path";
  }

  while (i < url.length) {
    // Variables keep the current state: `{{base}}/users` → var, then path.
    if (url.startsWith("{{", i)) {
      const end = url.indexOf("}}", i + 2);
      if (end !== -1) {
        const text = url.slice(i, end + 2);
        push("var", text, i, url.slice(i + 2, end).trim());
        i = end + 2;
        continue;
      }
    }
    const c = url[i];
    if (state === "fragment") {
      push("fragment", c, i);
    } else if (c === "#") {
      state = "fragment";
      push("fragment", c, i);
    } else if (c === "?" && state !== "qkey" && state !== "qval") {
      state = "qkey";
      push("punct", c, i);
    } else if (state === "host" || state === "port") {
      if (c === "/") {
        state = "path";
        push("punct", c, i);
      } else if (c === ":" && state === "host") {
        state = "port";
        push("punct", c, i);
      } else {
        push(state === "port" ? "port" : "host", c, i);
      }
    } else if (state === "path") {
      if (c === "/") {
        push("punct", c, i);
      } else if (c === ":" && url[i - 1] === "/" && PARAM_CHAR.test(url[i + 1] ?? "")) {
        let j = i + 1;
        while (j < url.length && PARAM_CHAR.test(url[j])) j++;
        push("param", url.slice(i, j), i);
        i = j;
        continue;
      } else {
        push("path", c, i);
      }
    } else if (state === "qkey") {
      if (c === "=") {
        state = "qval";
        push("punct", c, i);
      } else if (c === "&") {
        push("punct", c, i);
      } else {
        push("qkey", c, i);
      }
    } else {
      // qval
      if (c === "&") {
        state = "qkey";
        push("punct", c, i);
      } else {
        push("qval", c, i);
      }
    }
    i++;
  }
  return out;
}

/** Postman dynamic variables generated on each send (crates/core/src/vars.rs `dynamic_variable`). */
export const DYNAMIC_VARS = new Set(["$guid", "$randomUUID", "$timestamp", "$isoTimestamp", "$randomInt"]);
