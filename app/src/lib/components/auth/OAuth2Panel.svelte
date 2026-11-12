<script lang="ts">
  // OAuth 2.0 form (D39): current token (stored per user, never in the repo), token actions and
  // the grant-specific configuration.
  import { api, type Auth, type Collection, type OAuth2Config, type OAuth2Grant, type OAuth2StoredToken, type SendVariables } from "$lib/api";
  import { cleanAuth, isLiteralSecret } from "$lib/auth";
  import { t, type MessageKey } from "$lib/i18n/index.svelte";
  import { ws } from "$lib/stores/workspace.svelte";
  import { oauth2Selection } from "$lib/stores/oauth2.svelte";
  import SecretInput from "./SecretInput.svelte";

  interface Props {
    /** Always an oauth2 auth here. */
    auth: Auth;
    collection: Collection | null;
    inGit: boolean;
  }

  let { auth = $bindable(), collection, inGit }: Props = $props();

  type O2 = { type: "oauth2" } & OAuth2Config;
  const a = $derived(auth as O2);
  const rec = $derived(auth as unknown as Record<string, unknown>);
  function set(key: string, value: unknown) {
    (auth as unknown as Record<string, unknown>)[key] = value;
  }
  const str = (v: unknown) => (v === null || v === undefined ? "" : String(v));
  const nul = (v: string) => (v === "" ? null : v);

  const GRANTS: OAuth2Grant[] = ["authorization_code", "authorization_code_pkce", "implicit", "password", "client_credentials"];
  const g = $derived(a.grant_type);
  const usesBrowser = $derived(g === "authorization_code" || g === "authorization_code_pkce" || g === "implicit");
  const usesTokenUrl = $derived(g !== "implicit");

  function variables(): SendVariables {
    return collection
      ? ws.sendVariablesFor(collection)
      : { globals: {}, collection: {}, environment: {}, environment_name: null };
  }

  function config(): OAuth2Config {
    const { type: _type, ...rest } = cleanAuth(auth) as O2;
    return rest;
  }

  // ----- stored tokens -----
  let tokens = $state<OAuth2StoredToken[]>([]);
  let loadError = $state<string | null>(null);
  let listTimer: ReturnType<typeof setTimeout> | undefined;

  async function loadTokens(name: string) {
    try {
      tokens = name.trim() ? await api.oauth2ListTokens(name) : [];
      loadError = null;
    } catch (e) {
      tokens = [];
      loadError = String(e);
    }
  }

  $effect(() => {
    const name = a.token_name ?? "";
    clearTimeout(listTimer);
    listTimer = setTimeout(() => void loadTokens(name), 250);
    return () => clearTimeout(listTimer);
  });

  /** "literal" = use the access_token field; otherwise a stored token id. */
  const selected = $derived.by(() => {
    const name = a.token_name ?? "";
    const chosen = oauth2Selection.get(name);
    if (chosen === "literal") return "literal";
    if (chosen && tokens.some((x) => x.id === chosen)) return chosen;
    if (str(a.access_token).trim()) return "literal";
    return tokens[0]?.id ?? "literal";
  });
  const current = $derived(selected === "literal" ? null : (tokens.find((x) => x.id === selected) ?? null));

  async function choose(value: string) {
    const name = a.token_name ?? "";
    oauth2Selection.set(name, value);
    if (value !== "literal") {
      set("access_token", null);
      try {
        await api.oauth2SelectToken(name, value);
      } catch (e) {
        ws.fail(e);
      }
    }
  }

  // ----- flows -----
  let busy = $state<"get" | "refresh" | null>(null);
  let flowError = $state<string | null>(null);
  let notice = $state<string | null>(null);

  async function getNew() {
    busy = "get";
    flowError = null;
    notice = null;
    try {
      const tok = await api.oauth2GetToken(config(), variables());
      await loadTokens(a.token_name ?? "");
      await choose(tok.id);
      notice = t("auth.oauth2.got");
    } catch (e) {
      const msg = e && typeof e === "object" && "message" in e ? String((e as { message: unknown }).message) : String(e);
      if (!/cancel/i.test(msg)) flowError = msg;
    } finally {
      busy = null;
    }
  }

  async function cancelFlow() {
    try {
      await api.oauth2CancelFlow();
    } catch (e) {
      ws.fail(e);
    }
  }

  async function refresh() {
    if (!current) return;
    busy = "refresh";
    flowError = null;
    notice = null;
    try {
      const tok = await api.oauth2RefreshToken(current.id, config(), variables());
      await loadTokens(a.token_name ?? "");
      await choose(tok.id);
      notice = t("auth.oauth2.refreshed");
    } catch (e) {
      flowError = e && typeof e === "object" && "message" in e ? String((e as { message: unknown }).message) : String(e);
    } finally {
      busy = null;
    }
  }

  // ----- token details -----
  let now = $state(Date.now() / 1000);
  $effect(() => {
    const id = setInterval(() => (now = Date.now() / 1000), 30_000);
    return () => clearInterval(id);
  });

  function duration(secs: number): string {
    if (secs < 60) return `${Math.max(0, Math.round(secs))} s`;
    if (secs < 3600) return `${Math.round(secs / 60)} min`;
    if (secs < 86400) return `${Math.round(secs / 3600)} h`;
    return `${Math.round(secs / 86400)} d`;
  }

  function expiryText(tok: OAuth2StoredToken): { text: string; bad: boolean } {
    if (tok.expires_at === null) return { text: t("auth.oauth2.noExpiry"), bad: false };
    const left = tok.expires_at - now;
    if (left <= 0) return { text: t("auth.oauth2.expired"), bad: true };
    return { text: t("auth.oauth2.expiresIn", { time: duration(left) }), bad: left < 120 };
  }

  const dateOf = (secs: number) => new Date(secs * 1000).toLocaleString();

  // ----- manage dialog -----
  let dialog: HTMLDialogElement | undefined = $state();
  let all = $state<OAuth2StoredToken[]>([]);
  let rawId = $state<string | null>(null);

  async function openManage() {
    rawId = null;
    try {
      all = await api.oauth2ListTokens(null);
    } catch (e) {
      ws.fail(e);
      return;
    }
    dialog?.showModal();
  }

  async function remove(ids: string[]) {
    try {
      await api.oauth2DeleteTokens(ids);
      all = all.filter((x) => !ids.includes(x.id));
      await loadTokens(a.token_name ?? "");
    } catch (e) {
      ws.fail(e);
    }
  }

  // ----- extra params (record edited as rows) -----
  const extra = $derived(Object.entries(a.extra_params ?? {}));
  function setExtra(rows: [string, string][]) {
    const out: Record<string, string> = {};
    for (const [k, v] of rows) if (k.trim() || v) out[k] = v;
    set("extra_params", out);
  }

  const secretWarn = (v: unknown) => (inGit && isLiteralSecret(v) ? t("auth.literalSecret", { name: "client_secret" }) : null);

  let showAdvanced = $state(false);
</script>

<div class="o2">
  <section class="token">
    <div class="sec-title">{t("auth.oauth2.tokenTitle")}</div>
    <div class="row">
      <label class="lbl" for="o2-current">{t("auth.oauth2.currentToken")}</label>
      <div class="ctl inline">
        <select id="o2-current" value={selected} onchange={(e) => void choose(e.currentTarget.value)}>
          {#each tokens as tok (tok.id)}
            <option value={tok.id}>{tok.token_name} · {dateOf(tok.created_at)}</option>
          {/each}
          <option value="literal">{t("auth.oauth2.useLiteral")}</option>
        </select>
        <button class="small" onclick={openManage}>{t("auth.oauth2.manage")}</button>
      </div>
    </div>
    {#if selected === "literal"}
      <div class="row">
        <span class="lbl">{t("auth.field.access_token")}</span>
        <SecretInput
          value={str(a.access_token)}
          label={t("auth.field.access_token")}
          warning={inGit && isLiteralSecret(a.access_token) ? t("auth.literalSecret", { name: "access_token" }) : null}
          oninput={(v) => set("access_token", nul(v))}
        />
      </div>
    {:else if current}
      {@const exp = expiryText(current)}
      <div class="details">
        <span class:bad={exp.bad} class:ok={!exp.bad}>{exp.text}</span>
        {#if current.scope}<span>{t("auth.oauth2.scope", { scope: current.scope })}</span>{/if}
        <span>{current.refresh_token ? t("auth.oauth2.refreshAvailable") : t("auth.oauth2.noRefresh")}</span>
        {#if current.token_type}<span class="mono">{current.token_type}</span>{/if}
      </div>
    {/if}
    {#if loadError}<p class="err-text">{loadError}</p>{/if}

    <div class="actions">
      {#if busy === "get"}
        <span class="waiting">{usesBrowser ? t("auth.oauth2.waiting") : t("auth.oauth2.requesting")}</span>
        <button class="small" onclick={cancelFlow}>{t("auth.oauth2.cancel")}</button>
      {:else}
        <button class="primary small" disabled={busy !== null} onclick={getNew}>{t("auth.oauth2.getNew")}</button>
        <button class="small" disabled={busy !== null || !current?.refresh_token} onclick={refresh}>
          {busy === "refresh" ? t("auth.oauth2.refreshing") : t("auth.oauth2.refresh")}
        </button>
      {/if}
    </div>
    {#if flowError}<p class="err-text">{flowError}</p>{/if}
    {#if notice}<p class="ok-text">{notice}</p>{/if}

    <label class="check-row">
      <input type="checkbox" checked={a.auto_refresh ?? false} onchange={(e) => set("auto_refresh", e.currentTarget.checked)} />
      <span>{t("auth.field.auto_refresh")}</span>
    </label>
    <label class="check-row">
      <input type="checkbox" checked={a.auto_fetch ?? false} onchange={(e) => set("auto_fetch", e.currentTarget.checked)} />
      <span>{t("auth.field.auto_fetch")}</span>
    </label>
  </section>

  <section>
    <div class="sec-title">{t("auth.oauth2.configTitle")}</div>
    <p class="muted small">{t("auth.oauth2.storeHint")}</p>
    <div class="row">
      <label class="lbl" for="o2-name">{t("auth.field.token_name")}</label>
      <input id="o2-name" value={str(a.token_name)} oninput={(e) => set("token_name", e.currentTarget.value)} />
    </div>
    <div class="row">
      <label class="lbl" for="o2-grant">{t("auth.field.grant_type")}</label>
      <select id="o2-grant" value={a.grant_type} onchange={(e) => set("grant_type", e.currentTarget.value)}>
        {#each GRANTS as gr (gr)}<option value={gr}>{t(`auth.grant.${gr}` as MessageKey)}</option>{/each}
      </select>
    </div>
    {#if usesBrowser}
      <div class="row">
        <label class="lbl" for="o2-cb">{t("auth.field.callback_url")}</label>
        <div class="ctl">
          <input
            id="o2-cb"
            class="mono"
            value={str(a.callback_url)}
            placeholder="http://127.0.0.1/callback"
            oninput={(e) => set("callback_url", nul(e.currentTarget.value))}
          />
          <label class="check-row">
            <input
              type="checkbox"
              checked={a.use_local_callback ?? true}
              onchange={(e) => set("use_local_callback", e.currentTarget.checked)}
            />
            <span>{t("auth.field.use_local_callback")}</span>
          </label>
        </div>
      </div>
      <div class="row">
        <label class="lbl" for="o2-auth">{t("auth.field.auth_url")}</label>
        <input id="o2-auth" class="mono" value={str(a.auth_url)} oninput={(e) => set("auth_url", nul(e.currentTarget.value))} />
      </div>
    {/if}
    {#if usesTokenUrl}
      <div class="row">
        <label class="lbl" for="o2-tok">{t("auth.field.access_token_url")}</label>
        <input
          id="o2-tok"
          class="mono"
          value={str(a.access_token_url)}
          oninput={(e) => set("access_token_url", nul(e.currentTarget.value))}
        />
      </div>
    {/if}
    <div class="row">
      <label class="lbl" for="o2-cid">{t("auth.field.client_id")}</label>
      <input id="o2-cid" class="mono" value={str(a.client_id)} oninput={(e) => set("client_id", e.currentTarget.value)} />
    </div>
    {#if g !== "implicit"}
      <div class="row">
        <span class="lbl">{t("auth.field.client_secret")}</span>
        <SecretInput
          value={str(a.client_secret)}
          label={t("auth.field.client_secret")}
          warning={secretWarn(a.client_secret)}
          oninput={(v) => set("client_secret", nul(v))}
        />
      </div>
    {/if}
    {#if g === "authorization_code_pkce"}
      <div class="row">
        <label class="lbl" for="o2-pkce">{t("auth.field.code_challenge_method")}</label>
        <select
          id="o2-pkce"
          value={a.code_challenge_method ?? "S256"}
          onchange={(e) => set("code_challenge_method", e.currentTarget.value)}
        >
          <option value="S256">SHA-256 (S256)</option>
          <option value="plain">{t("auth.oauth2.plain")}</option>
        </select>
      </div>
      <div class="row">
        <label class="lbl" for="o2-ver">{t("auth.field.code_verifier")}</label>
        <input
          id="o2-ver"
          class="mono"
          value={str(a.code_verifier)}
          placeholder={t("auth.oauth2.autoGenerated")}
          oninput={(e) => set("code_verifier", nul(e.currentTarget.value))}
        />
      </div>
    {/if}
    {#if g === "password"}
      <div class="row">
        <label class="lbl" for="o2-user">{t("auth.field.username")}</label>
        <input id="o2-user" value={str(a.username)} oninput={(e) => set("username", nul(e.currentTarget.value))} />
      </div>
      <div class="row">
        <span class="lbl">{t("auth.field.password")}</span>
        <SecretInput
          value={str(a.password)}
          label={t("auth.field.password")}
          warning={inGit && isLiteralSecret(a.password) ? t("auth.literalSecret", { name: "password" }) : null}
          oninput={(v) => set("password", nul(v))}
        />
      </div>
    {/if}
    <div class="row">
      <label class="lbl" for="o2-scope">{t("auth.field.scope")}</label>
      <input id="o2-scope" class="mono" value={str(a.scope)} placeholder="read write" oninput={(e) => set("scope", nul(e.currentTarget.value))} />
    </div>
    {#if usesBrowser}
      <div class="row">
        <label class="lbl" for="o2-state">{t("auth.field.state")}</label>
        <input id="o2-state" class="mono" value={str(a.state)} oninput={(e) => set("state", nul(e.currentTarget.value))} />
      </div>
    {/if}
    {#if g !== "implicit"}
      <div class="row">
        <label class="lbl" for="o2-ca">{t("auth.field.client_authentication")}</label>
        <select
          id="o2-ca"
          value={a.client_authentication ?? "basic"}
          onchange={(e) => set("client_authentication", e.currentTarget.value)}
        >
          <option value="basic">{t("auth.oauth2.clientAuth.basic")}</option>
          <option value="body">{t("auth.oauth2.clientAuth.body")}</option>
        </select>
      </div>
    {/if}

    <button class="adv-toggle" aria-expanded={showAdvanced} onclick={() => (showAdvanced = !showAdvanced)}>
      <span class="chev">{showAdvanced ? "▾" : "▸"}</span>{t("auth.advanced")}
    </button>
    {#if showAdvanced}
      <div class="row">
        <label class="lbl" for="o2-aud">{t("auth.field.audience")}</label>
        <input id="o2-aud" class="mono" value={str(a.audience)} oninput={(e) => set("audience", nul(e.currentTarget.value))} />
      </div>
      <div class="row">
        <label class="lbl" for="o2-res">{t("auth.field.resource")}</label>
        <input id="o2-res" class="mono" value={str(a.resource)} oninput={(e) => set("resource", nul(e.currentTarget.value))} />
      </div>
      <div class="row">
        <label class="lbl" for="o2-addto">{t("auth.field.add_to")}</label>
        <select id="o2-addto" value={a.add_to ?? "header"} onchange={(e) => set("add_to", e.currentTarget.value)}>
          <option value="header">{t("auth.addTo.header")}</option>
          <option value="query">{t("auth.addTo.query")}</option>
        </select>
      </div>
      <div class="row">
        <label class="lbl" for="o2-prefix">{t("auth.field.header_prefix")}</label>
        <input
          id="o2-prefix"
          class="mono"
          value={str(a.header_prefix)}
          placeholder="Bearer"
          oninput={(e) => set("header_prefix", nul(e.currentTarget.value))}
        />
      </div>
      <div class="row top">
        <span class="lbl">{t("auth.field.extra_params")}</span>
        <div class="ctl">
          {#each extra as [k, v], i (i)}
            <div class="kv">
              <input
                class="mono"
                value={k}
                aria-label={t("request.key")}
                oninput={(e) => setExtra(extra.map((r, j) => (j === i ? [e.currentTarget.value, r[1]] : r)))}
              />
              <input
                class="mono"
                value={v}
                aria-label={t("request.value")}
                oninput={(e) => setExtra(extra.map((r, j) => (j === i ? [r[0], e.currentTarget.value] : r)))}
              />
              <button
                class="ghost x"
                title={t("request.removeRow")}
                aria-label={t("request.removeRow")}
                onclick={() => setExtra(extra.filter((_, j) => j !== i))}>×</button
              >
            </div>
          {/each}
          <button class="small add" onclick={() => setExtra([...extra, [`param${extra.length + 1}`, ""]])}>
            {t("auth.oauth2.addParam")}
          </button>
        </div>
      </div>
    {/if}
    <p class="muted small">{t("auth.help.variables")}</p>
  </section>
</div>

<dialog bind:this={dialog} class="manage" aria-label={t("auth.oauth2.manageTitle")} onclick={(e) => e.target === dialog && dialog?.close()}>
  <header>
    <strong>{t("auth.oauth2.manageTitle")}</strong>
    <button class="ghost" aria-label={t("vars.close")} onclick={() => dialog?.close()}>✕</button>
  </header>
  <p class="muted small">{t("auth.oauth2.storeHint")}</p>
  {#if all.length === 0}
    <p class="muted">{t("auth.oauth2.noTokens")}</p>
  {:else}
    <ul class="tok-list">
      {#each all as tok (tok.id)}
        {@const exp = expiryText(tok)}
        <li>
          <div class="tok-row">
            <span class="tok-name">{tok.token_name}</span>
            <span class="muted small">{t("auth.oauth2.created", { date: dateOf(tok.created_at) })}</span>
            <span class="small" class:bad={exp.bad}>{exp.text}</span>
            <span class="spacer"></span>
            <button class="ghost small" onclick={() => (rawId = rawId === tok.id ? null : tok.id)}>{t("auth.oauth2.viewRaw")}</button>
            <button class="ghost small danger" onclick={() => remove([tok.id])}>{t("auth.oauth2.delete")}</button>
          </div>
          {#if rawId === tok.id}<pre class="raw mono">{JSON.stringify(tok.raw, null, 2)}</pre>{/if}
        </li>
      {/each}
    </ul>
    <div class="dlg-actions">
      <button class="danger small" onclick={() => remove(all.map((x) => x.id))}>{t("auth.oauth2.deleteAll")}</button>
    </div>
  {/if}
</dialog>

<style>
  .o2 {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  section {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .token {
    padding: 10px 12px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--surface-2);
  }
  .sec-title {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--muted);
  }
  .row {
    display: grid;
    grid-template-columns: 170px 1fr;
    align-items: center;
    gap: 12px;
  }
  .row.top {
    align-items: start;
  }
  .lbl {
    font-size: 13px;
    color: var(--muted);
  }
  .ctl {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }
  .ctl.inline {
    flex-direction: row;
    align-items: center;
  }
  .ctl.inline select {
    flex: 1;
    min-width: 0;
  }
  input:not([type="checkbox"]),
  select {
    font-size: 13px;
    padding: 5px 8px;
    min-width: 0;
  }
  .small {
    font-size: 12px;
    padding: 4px 10px;
  }
  .muted {
    color: var(--muted);
    margin: 0;
  }
  p.small {
    padding: 0;
  }
  .details {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 14px;
    font-size: 12px;
    color: var(--muted);
    padding-left: 182px;
  }
  .ok {
    color: var(--ok);
  }
  .bad {
    color: var(--err);
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  .waiting {
    font-size: 13px;
    color: var(--warn);
  }
  .err-text {
    color: var(--err);
    font-size: 12px;
    margin: 0;
    word-break: break-word;
  }
  .ok-text {
    color: var(--ok);
    font-size: 12px;
    margin: 0;
  }
  .check-row {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
  }
  .check-row input {
    accent-color: var(--accent);
    margin: 0;
  }
  .adv-toggle {
    align-self: flex-start;
    border: none;
    background: none;
    color: var(--muted);
    font-size: 12px;
    padding: 4px 0;
  }
  .adv-toggle:hover {
    color: var(--text);
  }
  .chev {
    display: inline-block;
    width: 12px;
  }
  .kv {
    display: grid;
    grid-template-columns: 1fr 1fr 28px;
    gap: 4px;
  }
  .x {
    padding: 0 6px;
  }
  .add {
    align-self: flex-start;
  }
  .manage {
    width: min(640px, calc(100vw - 32px));
    max-height: calc(100vh - 80px);
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--surface);
    color: var(--text);
    padding: 14px 16px;
  }
  .manage::backdrop {
    background: color-mix(in srgb, var(--bg) 60%, transparent);
  }
  .manage header {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }
  .tok-list {
    list-style: none;
    margin: 10px 0;
    padding: 0;
  }
  .tok-list li {
    border-bottom: 1px solid var(--border);
    padding: 6px 0;
  }
  .tok-row {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }
  .tok-name {
    font-weight: 600;
    font-size: 13px;
  }
  .spacer {
    flex: 1;
  }
  .danger {
    color: var(--err);
  }
  .raw {
    margin: 6px 0 0;
    padding: 8px;
    max-height: 240px;
    overflow: auto;
    font-size: 11px;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: 6px;
    white-space: pre-wrap;
    word-break: break-all;
  }
  .dlg-actions {
    display: flex;
    justify-content: flex-end;
  }
</style>
