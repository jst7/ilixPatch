<script lang="ts">
  // Postman-style Authorization editor (D39), shared by requests, folders and collections.
  import type { Auth, AuthType, Collection } from "$lib/api";
  import { AUTH_FIELDS, defaultAuth, isLiteralSecret, type AuthField, type EffectiveAuth } from "$lib/auth";
  import { t, type MessageKey } from "$lib/i18n/index.svelte";
  import { ws } from "$lib/stores/workspace.svelte";
  import CodeEditor from "../CodeEditor.svelte";
  import AuthTypeSelect from "./AuthTypeSelect.svelte";
  import OAuth2Panel from "./OAuth2Panel.svelte";
  import SecretInput from "./SecretInput.svelte";

  interface Props {
    auth: Auth;
    scope: "request" | "folder" | "collection";
    /** What a parent provides (shown when `auth` is inherit). */
    inherited?: EffectiveAuth | null;
    collection?: Collection | null;
    projectPath?: string | null;
  }

  let { auth = $bindable(), scope, inherited = null, collection = null, projectPath = null }: Props = $props();

  const rec = $derived(auth as unknown as Record<string, unknown>);
  const fields = $derived(AUTH_FIELDS[auth.type] ?? []);
  const visible = $derived(fields.filter((f) => !f.show || f.show(rec)));
  const basic = $derived(visible.filter((f) => !f.advanced));
  const advanced = $derived(visible.filter((f) => f.advanced));

  const inGit = $derived(!!projectPath && (ws.repos[projectPath]?.length ?? 0) > 0);

  function setType(type: AuthType) {
    auth = defaultAuth(type);
  }

  function set(key: string, value: unknown) {
    (auth as unknown as Record<string, unknown>)[key] = value;
  }

  const str = (v: unknown) => (v === null || v === undefined ? "" : String(v));
  const label = (key: string) => t(`auth.field.${key}` as MessageKey);
  const typeLabel = (type: AuthType) => t(`auth.type.${type}` as MessageKey);

  function warningFor(f: AuthField): string | null {
    if (!inGit || (f.kind !== "secret" && f.kind !== "pem")) return null;
    return isLiteralSecret(rec[f.key]) ? t("auth.literalSecret", { name: f.key }) : null;
  }

  function jsonError(v: unknown): boolean {
    const s = str(v).trim();
    if (!s || s.includes("{{")) return false;
    try {
      JSON.parse(s);
      return false;
    } catch {
      return true;
    }
  }

  const optionLabel = (key: string, v: string) =>
    key === "in" || key === "add_to" ? t(`auth.addTo.${v}` as MessageKey) : v;

  let showAdvanced = $state(false);
</script>

{#snippet field(f: AuthField)}
  {@const warning = warningFor(f)}
  <div class="row" class:top={f.kind === "pem" || f.kind === "json"}>
    <label class="lbl" for="auth-{f.key}">{label(f.key)}</label>
    <div class="ctl">
      {#if f.kind === "text"}
        <input
          id="auth-{f.key}"
          class="mono"
          value={str(rec[f.key])}
          placeholder={f.placeholder ?? ""}
          spellcheck="false"
          autocomplete="off"
          oninput={(e) => set(f.key, e.currentTarget.value)}
        />
      {:else if f.kind === "secret"}
        <SecretInput
          value={str(rec[f.key])}
          label={label(f.key)}
          placeholder={f.placeholder ?? ""}
          {warning}
          oninput={(v) => set(f.key, v)}
        />
      {:else if f.kind === "pem"}
        <div class="pem">
          <textarea
            id="auth-{f.key}"
            class="mono"
            rows="6"
            spellcheck="false"
            placeholder="-----BEGIN PRIVATE KEY-----"
            value={str(rec[f.key])}
            oninput={(e) => set(f.key, e.currentTarget.value)}
          ></textarea>
          {#if warning}<span class="warn-text">{warning}</span>{/if}
        </div>
      {:else if f.kind === "json"}
        <div class="json" class:bad={jsonError(rec[f.key])}>
          <CodeEditor value={str(rec[f.key])} language="json" onchange={(v) => set(f.key, v)} />
        </div>
        {#if jsonError(rec[f.key])}<span class="err-text">{t("auth.invalidJson")}</span>{/if}
      {:else if f.kind === "select"}
        <select
          id="auth-{f.key}"
          value={str(rec[f.key])}
          onchange={(e) => set(f.key, e.currentTarget.value === "" ? null : e.currentTarget.value)}
        >
          {#if f.optional}<option value="">{t("auth.default")}</option>{/if}
          {#each f.options ?? [] as o (o)}<option value={o}>{optionLabel(f.key, o)}</option>{/each}
        </select>
      {:else if f.kind === "bool"}
        <input
          id="auth-{f.key}"
          class="check"
          type="checkbox"
          checked={!!rec[f.key]}
          onchange={(e) => set(f.key, e.currentTarget.checked)}
        />
      {:else if f.kind === "number"}
        <input
          id="auth-{f.key}"
          class="num"
          type="number"
          min="0"
          value={str(rec[f.key])}
          placeholder={f.placeholder ?? ""}
          onchange={(e) => set(f.key, e.currentTarget.value === "" ? null : Math.max(0, Math.floor(Number(e.currentTarget.value) || 0)))}
        />
      {/if}
    </div>
  </div>
{/snippet}

<div class="auth">
  <aside class="help">
    <div class="lbl-top">{t("auth.typeLabel")}</div>
    <AuthTypeSelect value={auth.type} exclude={scope === "collection" ? ["inherit"] : []} onchange={setType} />
    <p>{t(`auth.help.${auth.type}` as MessageKey)}</p>
    {#if auth.type !== "inherit" && auth.type !== "no_auth"}
      <p class="muted">{t("auth.help.generated")}</p>
      <p class="muted">{t("auth.help.variables")}</p>
    {/if}
  </aside>

  <div class="form">
    {#if auth.type === "inherit"}
      {#if inherited?.source}
        <p class="info">
          {t(scope === "folder" ? "auth.inherit.folder" : "auth.inherit.request", {
            type: typeLabel(inherited.auth.type),
            kind: t(inherited.source.kind === "folder" ? "auth.source.folder" : "auth.source.collection"),
            name: inherited.source.name,
          })}
        </p>
        <button class="link" onclick={() => inherited?.source && ws.openContainer(inherited.source.path)}>
          {t("auth.inherit.edit", { name: inherited.source.name })}
        </button>
      {:else}
        <p class="info">{t("auth.inherit.none")}</p>
        {#if collection}
          <button class="link" onclick={() => collection && ws.openContainer(collection.path)}>
            {t("auth.inherit.edit", { name: collection.name })}
          </button>
        {/if}
      {/if}
    {:else if auth.type === "no_auth"}
      <p class="info">{t(scope === "collection" ? "auth.noAuth.collection" : "auth.noAuth.request")}</p>
    {:else if auth.type === "oauth2"}
      <OAuth2Panel bind:auth {collection} {inGit} />
    {:else}
      {#each basic as f (f.key)}{@render field(f)}{/each}
      {#if advanced.length}
        <button class="adv-toggle" aria-expanded={showAdvanced} onclick={() => (showAdvanced = !showAdvanced)}>
          <span class="chev">{showAdvanced ? "▾" : "▸"}</span>
          {t("auth.advanced")}
        </button>
        {#if showAdvanced}
          <p class="muted small">{t("auth.advancedHint")}</p>
          {#each advanced as f (f.key)}{@render field(f)}{/each}
        {/if}
      {/if}
    {/if}
  </div>
</div>

<style>
  .auth {
    flex: 1;
    min-height: 0;
    display: flex;
    gap: 16px;
    padding: 10px 2px;
    overflow: auto;
  }
  .help {
    width: 260px;
    flex: none;
    display: flex;
    flex-direction: column;
    gap: 8px;
    font-size: 12px;
    padding-right: 14px;
    border-right: 1px solid var(--border);
  }
  .help p {
    margin: 0;
    line-height: 1.45;
  }
  .lbl-top {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--muted);
  }
  .muted {
    color: var(--muted);
  }
  .small {
    font-size: 12px;
    margin: 0;
  }
  .form {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-width: 720px;
  }
  .info {
    margin: 0;
    font-size: 13px;
  }
  .link {
    align-self: flex-start;
    border: none;
    background: none;
    padding: 0;
    color: var(--accent);
    font-size: 13px;
    cursor: pointer;
  }
  .link:hover {
    text-decoration: underline;
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
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .ctl > input:not(.check),
  .ctl select,
  textarea {
    font-size: 13px;
    padding: 5px 8px;
    min-width: 0;
  }
  .check {
    justify-self: start;
    align-self: flex-start;
    width: 16px;
    height: 16px;
    padding: 0;
    accent-color: var(--accent);
  }
  .num {
    max-width: 140px;
  }
  .pem {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  textarea {
    resize: vertical;
    font-size: 12px;
  }
  .json {
    height: 110px;
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow: hidden;
  }
  .json.bad {
    border-color: var(--err);
  }
  .err-text {
    color: var(--err);
    font-size: 12px;
  }
  .warn-text {
    color: var(--warn);
    font-size: 12px;
  }
  .adv-toggle {
    align-self: flex-start;
    border: none;
    background: none;
    color: var(--muted);
    font-size: 12px;
    padding: 4px 0;
    margin-top: 4px;
  }
  .adv-toggle:hover {
    color: var(--text);
  }
  .chev {
    display: inline-block;
    width: 12px;
  }
  @media (max-width: 760px) {
    .auth {
      flex-direction: column;
    }
    .help {
      width: auto;
      border-right: none;
      padding-right: 0;
    }
  }
</style>
