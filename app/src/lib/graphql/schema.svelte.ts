// D61: GraphQL schemas loaded by introspection, per request URL, for completion and validation in the editor.
// Not saved: loaded on demand (the endpoint, its auth and variables are the request's own).
import { buildClientSchema, getIntrospectionQuery, type GraphQLSchema, type IntrospectionQuery } from "graphql";
import { api, type Request, type SendResult, type SendVariables } from "$lib/api";

export interface SchemaState {
  loading: boolean;
  schema: GraphQLSchema | null;
  error: string | null;
  /** Named types in the schema (shown as "N types"). */
  types: number;
}

const CHUNK = 1024 * 1024;

/** The whole response body, reading the Rust-side handle when the inline text is only a preview. */
async function fullBody(res: SendResult): Promise<string> {
  const h = res.body_handle;
  if (!h || h.size <= res.body.length) return res.body;
  let out = "";
  try {
    for (let offset = 0; offset < h.size; ) {
      const c = await api.responseRead(h.id, offset, CHUNK);
      out += c.data;
      if (c.eof || c.length === 0) break;
      offset += c.length;
    }
  } finally {
    void api.responseRelease(h.id).catch(() => {});
  }
  return out;
}

class GraphqlSchemas {
  byUrl = $state<Record<string, SchemaState>>({});

  get(url: string): SchemaState | null {
    return this.byUrl[url] ?? null;
  }

  /** Introspect `request`'s endpoint with its headers, auth and variables (no scripts, no history). */
  async load(request: Request, variables: SendVariables) {
    const url = request.url;
    this.byUrl[url] = { loading: true, schema: this.byUrl[url]?.schema ?? null, error: null, types: this.byUrl[url]?.types ?? 0 };
    const probe: Request = {
      name: request.name,
      method: "POST",
      url,
      headers: request.headers,
      query: request.query,
      body: { type: "graphql", query: getIntrospectionQuery() },
      settings: request.settings,
      auth: request.auth,
    };
    try {
      const res = await api.sendRequest(probe, variables, null);
      const text = await fullBody(res);
      let doc: { data?: IntrospectionQuery; errors?: { message?: string }[] };
      try {
        doc = JSON.parse(text);
      } catch {
        throw new Error(`HTTP ${res.status}: the response is not JSON`);
      }
      if (!doc.data?.__schema) {
        const msg = doc.errors?.map((e) => e.message).filter(Boolean).join("; ");
        throw new Error(msg || `HTTP ${res.status}: no schema in the response (is introspection disabled?)`);
      }
      const schema = buildClientSchema(doc.data);
      const types = Object.keys(schema.getTypeMap()).filter((n) => !n.startsWith("__")).length;
      this.byUrl[url] = { loading: false, schema, error: null, types };
    } catch (e) {
      const message =
        e && typeof e === "object" && "detail" in e ? String((e as { detail: unknown }).detail) : e instanceof Error ? e.message : String(e);
      this.byUrl[url] = { loading: false, schema: this.byUrl[url]?.schema ?? null, error: message, types: this.byUrl[url]?.types ?? 0 };
    }
  }
}

export const gqlSchemas = new GraphqlSchemas();
