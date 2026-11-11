<script lang="ts">
  // Per-request overrides of the global network settings (D29). null = use the global value.
  import { onMount } from "svelte";
  import type { HttpVersionPref } from "$lib/api";
  import { t } from "$lib/i18n/index.svelte";
  import { network } from "$lib/stores/network.svelte";
  import type { Draft } from "$lib/stores/workspace.svelte";

  interface Props {
    draft: Draft;
  }

  let { draft }: Props = $props();

  onMount(() => {
    void network.load();
  });

  const g = $derived(network.settings);
  const st = $derived(draft.settings);

  const onOff = (v: boolean | undefined) => (v === undefined ? "…" : v ? t("reqset.on") : t("reqset.off"));
  const httpLabel = (v: HttpVersionPref | undefined) =>
    v === "http1" ? "HTTP/1.1" : v === "http2" ? "HTTP/2" : v === "auto" ? t("net.auto") : "…";
  const timeoutLabel = (v: number | undefined) =>
    v === undefined ? "…" : v === 0 ? t("reqset.noTimeout") : `${v} ms`;

  function triValue(v: boolean | null): string {
    return v === null ? "" : v ? "on" : "off";
  }
  function parseTri(v: string): boolean | null {
    return v === "on" ? true : v === "off" ? false : null;
  }
  function parseNum(v: string): number | null {
    if (v.trim() === "") return null;
    const n = Math.floor(Number(v));
    return Number.isFinite(n) && n >= 0 ? n : null;
  }
</script>

<div class="settings">
  <p class="hint">{t("reqset.hint")}</p>

  <label class="row">
    <span>{t("net.verifySsl")}</span>
    <select value={triValue(st.verify_ssl)} onchange={(e) => (draft.settings.verify_ssl = parseTri(e.currentTarget.value))}>
      <option value="">{t("reqset.default", { value: onOff(g?.verify_ssl) })}</option>
      <option value="on">{t("reqset.on")}</option>
      <option value="off">{t("reqset.off")}</option>
    </select>
  </label>
  {#if st.verify_ssl === false}
    <p class="warn">{t("reqset.sslOffWarning")}</p>
  {/if}

  <label class="row">
    <span>{t("net.followRedirects")}</span>
    <select
      value={triValue(st.follow_redirects)}
      onchange={(e) => (draft.settings.follow_redirects = parseTri(e.currentTarget.value))}
    >
      <option value="">{t("reqset.default", { value: onOff(g?.follow_redirects) })}</option>
      <option value="on">{t("reqset.on")}</option>
      <option value="off">{t("reqset.off")}</option>
    </select>
  </label>

  <label class="row">
    <span>{t("net.maxRedirects")}</span>
    <input
      type="number"
      min="0"
      value={st.max_redirects ?? ""}
      placeholder={t("reqset.default", { value: g ? String(g.max_redirects) : "…" })}
      onchange={(e) => (draft.settings.max_redirects = parseNum(e.currentTarget.value))}
    />
  </label>

  <label class="row">
    <span>{t("reqset.timeout")}</span>
    <input
      type="number"
      min="0"
      step="1000"
      value={st.timeout_ms ?? ""}
      placeholder={t("reqset.default", { value: timeoutLabel(g?.timeout_ms) })}
      onchange={(e) => (draft.settings.timeout_ms = parseNum(e.currentTarget.value))}
    />
  </label>

  <label class="row">
    <span>{t("net.httpVersion")}</span>
    <select
      value={st.http_version ?? ""}
      onchange={(e) => {
        const v = e.currentTarget.value;
        draft.settings.http_version = v === "auto" || v === "http1" || v === "http2" ? v : null;
      }}
    >
      <option value="">{t("reqset.default", { value: httpLabel(g?.http_version) })}</option>
      <option value="auto">{t("net.auto")}</option>
      <option value="http1">HTTP/1.1</option>
      <option value="http2">HTTP/2</option>
    </select>
  </label>
</div>

<style>
  .settings {
    flex: 1;
    overflow: auto;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px 2px;
    max-width: 560px;
  }
  .hint {
    color: var(--muted);
    font-size: 12px;
    margin: 0 0 4px;
  }
  .row {
    display: grid;
    grid-template-columns: 200px 1fr;
    align-items: center;
    gap: 12px;
    font-size: 13px;
  }
  select,
  input {
    font-size: 13px;
    padding: 5px 8px;
  }
  .warn {
    margin: 0;
    color: var(--warn);
    border: 1px solid var(--warn);
    background: color-mix(in srgb, var(--warn) 10%, transparent);
    border-radius: 6px;
    padding: 5px 10px;
    font-size: 12px;
  }
</style>
