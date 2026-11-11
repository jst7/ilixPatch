<script lang="ts">
  // Settings → "HTTPS & certificates" / "Proxy" / "Requests" (one concept per vertical tab).
  import { onMount } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import type { ClientCertificate } from "$lib/api";
  import { t } from "$lib/i18n/index.svelte";
  import { network } from "$lib/stores/network.svelte";

  interface Props {
    part: "https" | "proxy" | "requests";
  }

  let { part }: Props = $props();

  onMount(() => {
    void network.load();
  });

  const s = $derived(network.settings);

  function changed() {
    network.changed();
  }

  async function pickFile(extensions: string[], filterName: string): Promise<string | null> {
    try {
      const file = await open({ multiple: false, directory: false, filters: [{ name: filterName, extensions }] });
      return typeof file === "string" ? file : null;
    } catch (e) {
      network.saveError = String(e);
      return null;
    }
  }

  function baseName(p: string | null): string {
    return p ? (p.split(/[\\/]/).pop() ?? p) : "";
  }

  // ----- CA certificates -----
  async function addCa() {
    const f = await pickFile(["pem", "crt", "cer"], t("net.filter.certificates"));
    if (f && s && !s.ca_certificates.includes(f)) {
      s.ca_certificates.push(f);
      changed();
    }
  }

  function removeCa(i: number) {
    s?.ca_certificates.splice(i, 1);
    changed();
  }

  // ----- Client certificates -----
  interface Editing {
    index: number | null;
    kind: "pem" | "pfx";
    cert: ClientCertificate;
  }
  let editing = $state<Editing | null>(null);

  function newCert() {
    editing = {
      index: null,
      kind: "pem",
      cert: { host: "", cert_path: null, key_path: null, pfx_path: null, passphrase: null, enabled: true },
    };
  }

  function editCert(i: number) {
    if (!s) return;
    const c = s.client_certificates[i];
    editing = { index: i, kind: c.pfx_path ? "pfx" : "pem", cert: { ...c } };
  }

  function removeCert(i: number) {
    s?.client_certificates.splice(i, 1);
    if (editing?.index === i) editing = null;
    changed();
  }

  const editValid = $derived(
    !!editing &&
      editing.cert.host.trim() !== "" &&
      (editing.kind === "pfx" ? !!editing.cert.pfx_path : !!editing.cert.cert_path && !!editing.cert.key_path),
  );

  function commitCert() {
    if (!s || !editing || !editValid) return;
    const c = editing.cert;
    const cert: ClientCertificate = {
      host: c.host.trim(),
      enabled: c.enabled,
      cert_path: editing.kind === "pem" ? c.cert_path : null,
      key_path: editing.kind === "pem" ? c.key_path : null,
      pfx_path: editing.kind === "pfx" ? c.pfx_path : null,
      passphrase: c.passphrase ? c.passphrase : null,
    };
    if (editing.index === null) s.client_certificates.push(cert);
    else s.client_certificates[editing.index] = cert;
    editing = null;
    changed();
  }

  async function pickCertFile(field: "cert_path" | "key_path" | "pfx_path") {
    if (!editing) return;
    const exts = field === "pfx_path" ? ["pfx", "p12"] : field === "key_path" ? ["pem", "key"] : ["pem", "crt", "cer"];
    const f = await pickFile(exts, field === "pfx_path" ? t("net.filter.pfx") : t("net.filter.certificates"));
    if (f && editing) editing.cert[field] = f;
  }

  // ----- Proxy bypass (kept as text so typing separators isn't reformatted) -----
  let bypassText = $state("");
  let bypassLoaded = false;
  $effect(() => {
    if (s && !bypassLoaded) {
      bypassLoaded = true;
      bypassText = s.proxy.bypass.join(", ");
    }
  });

  function onBypass(text: string) {
    bypassText = text;
    if (!s) return;
    s.proxy.bypass = text
      .split(/[,\n]/)
      .map((x) => x.trim())
      .filter(Boolean);
    changed();
  }

  const nullIfEmpty = (v: string) => (v.trim() ? v : null);
  const nonNeg = (v: string) => Math.max(0, Math.floor(Number(v) || 0));
</script>

<section class="net">
  <div class="status" aria-live="polite">
    {#if network.saveError}
      <div class="err-box" role="alert">
        <strong>{t("net.saveError")}</strong>
        <span>{network.saveError}</span>
      </div>
    {:else if network.saving}
      <span class="muted">{t("net.saving")}</span>
    {:else if network.saved}
      <span class="ok">{t("net.saved")}</span>
    {/if}
  </div>

  {#if network.loadError}
    <div class="err-box" role="alert">
      <span>{network.loadError}</span>
      <button onclick={() => network.load(true)}>{t("net.retry")}</button>
    </div>
  {:else if !s}
    <p class="muted">{t("common.loading")}</p>
  {:else if part === "https"}
    <p class="muted note">{t("net.corsNote", { app: t("app.name") })}</p>

    <label class="row">
      <span>{t("net.verifySsl")}</span>
      <input class="toggle" type="checkbox" bind:checked={s.verify_ssl} onchange={changed} />
    </label>
    {#if !s.verify_ssl}
      <p class="warn-box">{t("net.verifySslOffWarning")}</p>
    {/if}
    <label class="row">
      <span>{t("net.systemCa")}</span>
      <input class="toggle" type="checkbox" bind:checked={s.use_system_ca} onchange={changed} />
    </label>
    <label class="row">
      <span>{t("net.minTls")}</span>
      <select
        value={s.min_tls_version ?? ""}
        onchange={(e) => {
          if (!s) return;
          const v = e.currentTarget.value;
          s.min_tls_version = v === "1.2" || v === "1.3" ? v : null;
          changed();
        }}
      >
        <option value="">{t("net.auto")}</option>
        <option value="1.2">TLS 1.2</option>
        <option value="1.3">TLS 1.3</option>
      </select>
    </label>

    <div class="group">
      <div class="group-head">
        <div>
          <div class="group-title">{t("net.caTitle")}</div>
          <p class="muted small">{t("net.caHint")}</p>
        </div>
        <button onclick={addCa}>{t("net.add")}</button>
      </div>
      {#if s.ca_certificates.length === 0}
        <p class="muted small">{t("net.caEmpty")}</p>
      {:else}
        <ul class="files">
          {#each s.ca_certificates as f, i (f)}
            <li>
              <span class="mono path" title={f}>{f}</span>
              <button class="ghost" title={t("net.remove")} aria-label={t("net.remove")} onclick={() => removeCa(i)}>×</button>
            </li>
          {/each}
        </ul>
      {/if}
    </div>

    <div class="group">
      <div class="group-head">
        <div>
          <div class="group-title">{t("net.clientTitle")}</div>
          <p class="muted small">{t("net.clientHint")}</p>
        </div>
        <button onclick={newCert} disabled={editing !== null}>{t("net.add")}</button>
      </div>
      {#if s.client_certificates.length === 0 && !editing}
        <p class="muted small">{t("net.clientEmpty")}</p>
      {:else if s.client_certificates.length}
        <table>
          <thead>
            <tr>
              <th>{t("net.client.host")}</th>
              <th>{t("net.client.files")}</th>
              <th class="c">{t("net.client.enabled")}</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            {#each s.client_certificates as c, i (i)}
              <tr class:off={!c.enabled}>
                <td class="mono">{c.host}</td>
                <td class="mono small" title={[c.pfx_path, c.cert_path, c.key_path].filter(Boolean).join("\n")}>
                  {c.pfx_path ? baseName(c.pfx_path) : `${baseName(c.cert_path)} + ${baseName(c.key_path)}`}
                </td>
                <td class="c">
                  <input
                    class="toggle"
                    type="checkbox"
                    bind:checked={c.enabled}
                    onchange={changed}
                    aria-label={t("net.client.enabled")}
                  />
                </td>
                <td class="acts">
                  <button class="ghost" onclick={() => editCert(i)}>{t("net.edit")}</button>
                  <button class="ghost danger" onclick={() => removeCert(i)}>{t("net.remove")}</button>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}

      {#if editing}
        <div class="editor">
          <label class="row">
            <span>{t("net.client.host")}</span>
            <input
              class="mono"
              bind:value={editing.cert.host}
              placeholder={t("net.client.hostPlaceholder")}
              spellcheck="false"
              autocomplete="off"
            />
          </label>
          <div class="row">
            <span>{t("net.client.type")}</span>
            <span class="seg" role="radiogroup" aria-label={t("net.client.type")}>
              <button role="radio" aria-checked={editing.kind === "pem"} class:active={editing.kind === "pem"} onclick={() => editing && (editing.kind = "pem")}>
                {t("net.client.pem")}
              </button>
              <button role="radio" aria-checked={editing.kind === "pfx"} class:active={editing.kind === "pfx"} onclick={() => editing && (editing.kind = "pfx")}>
                {t("net.client.pfx")}
              </button>
            </span>
          </div>
          {#if editing.kind === "pem"}
            <div class="row">
              <span>{t("net.client.cert")}</span>
              <span class="pick">
                <span class="mono path" title={editing.cert.cert_path ?? ""}>{editing.cert.cert_path ?? t("net.noFile")}</span>
                <button onclick={() => pickCertFile("cert_path")}>{t("net.choose")}</button>
              </span>
            </div>
            <div class="row">
              <span>{t("net.client.key")}</span>
              <span class="pick">
                <span class="mono path" title={editing.cert.key_path ?? ""}>{editing.cert.key_path ?? t("net.noFile")}</span>
                <button onclick={() => pickCertFile("key_path")}>{t("net.choose")}</button>
              </span>
            </div>
          {:else}
            <div class="row">
              <span>{t("net.client.pfxFile")}</span>
              <span class="pick">
                <span class="mono path" title={editing.cert.pfx_path ?? ""}>{editing.cert.pfx_path ?? t("net.noFile")}</span>
                <button onclick={() => pickCertFile("pfx_path")}>{t("net.choose")}</button>
              </span>
            </div>
          {/if}
          <label class="row">
            <span>{t("net.client.passphrase")}</span>
            <input
              type="password"
              value={editing.cert.passphrase ?? ""}
              oninput={(e) => editing && (editing.cert.passphrase = e.currentTarget.value)}
              autocomplete="off"
            />
          </label>
          <p class="muted small">{t("net.client.passphraseHint")}</p>
          <label class="row">
            <span>{t("net.client.enabled")}</span>
            <input class="toggle" type="checkbox" bind:checked={editing.cert.enabled} />
          </label>
          <div class="editor-actions">
            <button class="ghost" onclick={() => (editing = null)}>{t("net.cancel")}</button>
            <button class="primary" disabled={!editValid} onclick={commitCert}>
              {editing.index === null ? t("net.add") : t("net.apply")}
            </button>
          </div>
        </div>
      {/if}
    </div>
  {:else if part === "proxy"}
    <div class="row">
      <span>{t("net.proxy.mode")}</span>
      <span class="seg" role="radiogroup" aria-label={t("net.proxy.mode")}>
        {#each [["system", "net.proxy.system"], ["none", "net.proxy.none"], ["custom", "net.proxy.custom"]] as const as [mode, label] (mode)}
          <button
            role="radio"
            aria-checked={s.proxy.mode === mode}
            class:active={s.proxy.mode === mode}
            onclick={() => {
              if (!s) return;
              s.proxy.mode = mode;
              changed();
            }}>{t(label)}</button
          >
        {/each}
      </span>
    </div>
    <p class="muted small">
      {s.proxy.mode === "system"
        ? t("net.proxy.systemHint")
        : s.proxy.mode === "none"
          ? t("net.proxy.noneHint")
          : t("net.proxy.customHint")}
    </p>
    {#if s.proxy.mode === "custom"}
      <label class="row">
        <span>{t("net.proxy.url")}</span>
        <input
          class="mono"
          value={s.proxy.url ?? ""}
          placeholder="http://proxy:8080"
          spellcheck="false"
          autocomplete="off"
          oninput={(e) => {
            if (!s) return;
            s.proxy.url = nullIfEmpty(e.currentTarget.value);
            changed();
          }}
        />
      </label>
      <label class="row">
        <span>{t("net.proxy.username")}</span>
        <input
          value={s.proxy.username ?? ""}
          autocomplete="off"
          spellcheck="false"
          oninput={(e) => {
            if (!s) return;
            s.proxy.username = nullIfEmpty(e.currentTarget.value);
            changed();
          }}
        />
      </label>
      <label class="row">
        <span>{t("net.proxy.password")}</span>
        <input
          type="password"
          value={s.proxy.password ?? ""}
          autocomplete="off"
          oninput={(e) => {
            if (!s) return;
            s.proxy.password = nullIfEmpty(e.currentTarget.value);
            changed();
          }}
        />
      </label>
    {/if}
    {#if s.proxy.mode !== "none"}
      <label class="row top">
        <span>{t("net.proxy.bypass")}</span>
        <textarea
          class="mono"
          rows="3"
          value={bypassText}
          placeholder="localhost, .internal, 10.0.0.0/8"
          spellcheck="false"
          oninput={(e) => onBypass(e.currentTarget.value)}
        ></textarea>
      </label>
      <p class="muted small">{t("net.proxy.bypassHint")}</p>
    {/if}
  {:else}
    <label class="row">
      <span>{t("net.followRedirects")}</span>
      <input class="toggle" type="checkbox" bind:checked={s.follow_redirects} onchange={changed} />
    </label>
    <label class="row">
      <span>{t("net.maxRedirects")}</span>
      <input
        class="num"
        type="number"
        min="0"
        disabled={!s.follow_redirects}
        value={s.max_redirects}
        onchange={(e) => {
          if (!s) return;
          s.max_redirects = nonNeg(e.currentTarget.value);
          changed();
        }}
      />
    </label>
    <label class="row">
      <span>{t("net.timeout")}</span>
      <span class="inline">
        <input
          class="num"
          type="number"
          min="0"
          step="1000"
          value={s.timeout_ms}
          onchange={(e) => {
            if (!s) return;
            s.timeout_ms = nonNeg(e.currentTarget.value);
            changed();
          }}
        />
        <span class="muted small">{t("net.timeoutHint")}</span>
      </span>
    </label>
    <label class="row">
      <span>{t("net.httpVersion")}</span>
      <select bind:value={s.http_version} onchange={changed}>
        <option value="auto">{t("net.auto")}</option>
        <option value="http1">HTTP/1.1</option>
        <option value="http2">HTTP/2</option>
      </select>
    </label>
    <p class="muted note">{t("net.corsNote", { app: t("app.name") })}</p>
  {/if}
</section>

<style>
  .net {
    display: flex;
    flex-direction: column;
    gap: 10px;
    font-size: 14px;
  }
  .status {
    min-height: 0;
  }
  .muted {
    color: var(--muted);
    margin: 0;
  }
  .small {
    font-size: 12px;
  }
  .note {
    font-size: 12px;
    border-left: 2px solid var(--border);
    padding-left: 8px;
  }
  .ok {
    color: var(--ok);
    font-size: 12px;
  }
  .err-box,
  .warn-box {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 10px;
    align-items: center;
    border-radius: 6px;
    padding: 6px 10px;
    font-size: 13px;
    margin: 0;
    word-break: break-word;
  }
  .err-box {
    color: var(--err);
    border: 1px solid var(--err);
    background: color-mix(in srgb, var(--err) 8%, transparent);
  }
  .warn-box {
    color: var(--warn);
    border: 1px solid var(--warn);
    background: color-mix(in srgb, var(--warn) 10%, transparent);
  }
  .row {
    display: grid;
    grid-template-columns: 180px 1fr;
    align-items: center;
    gap: 12px;
  }
  .row.top {
    align-items: start;
  }
  .row > span:first-child {
    font-size: 13px;
  }
  input,
  select,
  textarea,
  button {
    font: inherit;
    font-size: 13px;
    color: var(--text);
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 5px 9px;
  }
  textarea {
    resize: vertical;
    font-size: 12px;
  }
  .mono {
    font-family: var(--mono);
  }
  button {
    cursor: pointer;
  }
  button:disabled,
  input:disabled {
    opacity: 0.55;
    cursor: default;
  }
  button.primary {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--accent-text);
  }
  button.ghost {
    background: none;
    border-color: transparent;
    color: var(--muted);
  }
  button.ghost:hover {
    color: var(--text);
  }
  button.danger {
    color: var(--err);
  }
  input.toggle {
    justify-self: start;
    width: 16px;
    height: 16px;
    padding: 0;
    accent-color: var(--accent);
  }
  .num {
    width: 110px;
  }
  .inline {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .group {
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 10px 12px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .group-head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
  }
  .group-title {
    font-weight: 600;
    font-size: 13px;
  }
  .files {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .files li {
    display: flex;
    align-items: center;
    gap: 6px;
    border-bottom: 1px solid var(--border);
    padding: 2px 0;
  }
  .path {
    flex: 1;
    min-width: 0;
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 13px;
    table-layout: fixed;
  }
  th {
    text-align: left;
    font-weight: 500;
    font-size: 12px;
    color: var(--muted);
    padding: 4px 6px;
    border-bottom: 1px solid var(--border);
  }
  td {
    padding: 4px 6px;
    border-bottom: 1px solid var(--border);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  tr.off td {
    color: var(--muted);
  }
  .c {
    text-align: center;
    width: 70px;
  }
  td.c input {
    display: inline-block;
  }
  .acts {
    width: 150px;
    text-align: right;
  }
  .acts button {
    padding: 2px 6px;
  }
  .editor {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px;
    border-radius: 6px;
    background: var(--surface-2);
    border: 1px solid var(--border);
  }
  .editor input:not(.toggle),
  .editor button:not(.ghost):not(.primary) {
    background: var(--surface);
  }
  .pick {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .seg {
    display: inline-flex;
    gap: 4px;
    flex-wrap: wrap;
  }
  .seg button {
    color: var(--muted);
  }
  .seg button.active {
    color: var(--text);
    border-color: var(--accent);
    background: var(--selection);
  }
  .editor-actions {
    display: flex;
    justify-content: flex-end;
    gap: 6px;
  }
</style>
