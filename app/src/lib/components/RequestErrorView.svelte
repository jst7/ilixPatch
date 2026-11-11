<script lang="ts">
  // Descriptive view of a failed send (D36): localized title, likely cause, suggestions,
  // quick actions and the raw technical details.
  import type { RequestError } from "$lib/api";
  import { t } from "$lib/i18n/index.svelte";
  import { describeRequestError } from "$lib/requestError";
  import { ws } from "$lib/stores/workspace.svelte";
  import { ui, type SettingsSection } from "$lib/stores/ui.svelte";

  interface Props {
    error: RequestError;
    /** Response key (request path or history key); used to find the draft and its collection. */
    path: string;
  }

  let { error, path }: Props = $props();

  const draft = $derived(ws.drafts[path]);
  const loc = $derived(ws.index.get(path) ?? null);
  const kind = $derived(error.kind);
  const isTls = $derived(kind.startsWith("tls_"));

  const envText = $derived.by(() => {
    const name = loc ? ws.envFor(loc.collection) : "";
    return name ? t("reqerr.envNamed", { name }) : t("reqerr.noEnvironment");
  });

  const desc = $derived(describeRequestError(error, envText));
  const title = $derived(desc.title);
  const cause = $derived(desc.cause);
  const tips = $derived(desc.tips);

  // ----- actions -----
  let done = $state<string | null>(null);

  function disableSsl() {
    if (!draft) return;
    draft.settings.verify_ssl = false;
    done = t("reqerr.action.sslDisabled");
  }

  function increaseTimeout() {
    if (!draft) return;
    const current = draft.settings.timeout_ms ?? error.timeout_ms ?? 30_000;
    const ms = Math.max(current * 2, 60_000);
    draft.settings.timeout_ms = ms;
    done = t("reqerr.action.timeoutSet", { ms });
  }

  function openSettings(section: SettingsSection) {
    ui.openSettings(section);
  }

  // Reset the confirmation when a new error arrives.
  $effect(() => {
    void error;
    done = null;
  });

  // ----- details -----
  let copied = $state(false);
  async function copyDetails() {
    try {
      await navigator.clipboard.writeText(error.detail || error.message);
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch (e) {
      ws.fail(e);
    }
  }
</script>

<div class="reqerr" role="alert">
  <div class="title-row">
    <svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      {#if isTls}
        <rect x="5" y="11" width="14" height="10" rx="2" /><path d="M8 11V7a4 4 0 018 0v4M12 15v2" />
      {:else if kind === "timeout"}
        <circle cx="12" cy="12" r="9" /><path d="M12 7v5l3 2" />
      {:else if kind === "connection_refused" || kind === "connection_reset" || kind === "dns"}
        <path d="M9 7V3M15 7V3M6 7h12v4a6 6 0 01-12 0zM12 17v4" />
      {:else}
        <path d="M12 3l9.5 17h-19zM12 10v4M12 17h.01" />
      {/if}
    </svg>
    <h4>{title}</h4>
  </div>

  <div class="block">
    <div class="label">{t("reqerr.likelyCause")}</div>
    <p>{cause}</p>
  </div>

  {#if kind === "invalid_url" && error.url}
    <div class="block">
      <div class="label">{t("reqerr.url")}</div>
      <code class="mono url">{error.url}</code>
    </div>
  {/if}

  {#if kind === "undefined_variables" && error.variables?.length}
    <div class="block">
      <div class="label">{t("reqerr.variables")}</div>
      <div class="vars">
        {#each error.variables as v (v)}<code class="mono var">{`{{${v}}}`}</code>{/each}
      </div>
    </div>
  {/if}

  {#if tips.length}
    <div class="block">
      <div class="label">{t("reqerr.tryThis")}</div>
      <ul>
        {#each tips as tip, i (i)}<li>{tip}</li>{/each}
      </ul>
    </div>
  {/if}

  <div class="actions">
    {#if isTls && kind !== "tls_client_certificate" && draft && draft.settings.verify_ssl !== false}
      <button onclick={disableSsl}>{t("reqerr.action.disableSsl")}</button>
    {/if}
    {#if isTls && kind !== "tls_client_certificate"}
      <button onclick={() => openSettings("https")}>{t("reqerr.action.openHttps")}</button>
    {/if}
    {#if kind === "tls_client_certificate"}
      <button onclick={() => openSettings("https")}>{t("reqerr.action.openCertificates")}</button>
    {/if}
    {#if kind === "proxy" || (error.proxy && (kind === "timeout" || kind === "connection_refused"))}
      <button onclick={() => openSettings("proxy")}>{t("reqerr.action.openProxy")}</button>
    {/if}
    {#if kind === "timeout" && draft}
      <button onclick={increaseTimeout}>{t("reqerr.action.increaseTimeout")}</button>
    {/if}
    {#if kind === "too_many_redirects"}
      <button onclick={() => openSettings("requests")}>{t("reqerr.action.openRequests")}</button>
    {/if}
    {#if kind === "undefined_variables"}
      <button onclick={() => ws.openEnvironments(loc?.projectPath ?? null)}>{t("reqerr.action.openEnvironments")}</button>
    {/if}
  </div>
  {#if done}
    <p class="done">{done} {t("reqerr.sendAgain")}</p>
  {/if}

  <details>
    <summary>{t("reqerr.details")}</summary>
    <div class="detail-box">
      <button class="ghost copy" onclick={copyDetails}>{copied ? t("reqerr.copied") : t("reqerr.copy")}</button>
      <pre class="mono">{error.detail || error.message}</pre>
    </div>
  </details>
</div>

<style>
  .reqerr {
    border: 1px solid var(--err);
    border-left-width: 3px;
    border-radius: 6px;
    padding: 10px 14px;
    overflow: auto;
    display: flex;
    flex-direction: column;
    gap: 8px;
    background: color-mix(in srgb, var(--err) 5%, var(--surface));
    font-size: 13px;
    min-height: 0;
  }
  .title-row {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--err);
  }
  h4 {
    margin: 0;
    font-size: 15px;
  }
  .label {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--muted);
    margin-bottom: 2px;
  }
  p {
    margin: 0;
    word-break: break-word;
  }
  ul {
    margin: 0;
    padding-left: 18px;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .mono {
    font-family: var(--mono);
  }
  .url {
    display: block;
    font-size: 12px;
    padding: 4px 8px;
    border-radius: 4px;
    background: var(--surface-2);
    border: 1px solid var(--border);
    word-break: break-all;
  }
  .vars {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .var {
    font-size: 12px;
    padding: 1px 6px;
    border-radius: 4px;
    color: var(--err);
    background: color-mix(in srgb, var(--err) 10%, transparent);
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .actions:empty {
    display: none;
  }
  .actions button {
    font-size: 12px;
    padding: 4px 10px;
  }
  .done {
    color: var(--ok);
    font-size: 12px;
  }
  details summary {
    cursor: pointer;
    color: var(--muted);
    font-size: 12px;
  }
  .detail-box {
    position: relative;
    margin-top: 6px;
  }
  .detail-box pre {
    margin: 0;
    padding: 8px 10px;
    padding-right: 70px;
    border-radius: 4px;
    background: var(--surface-2);
    border: 1px solid var(--border);
    white-space: pre-wrap;
    word-break: break-word;
    font-size: 12px;
    color: var(--text);
  }
  .copy {
    position: absolute;
    top: 4px;
    right: 4px;
    font-size: 11px;
    padding: 2px 8px;
  }
</style>
