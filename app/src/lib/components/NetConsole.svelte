<script lang="ts">
  // Network console (D46): every exchange the app sent — requests as they went on the wire,
  // redirect hops, Digest/NTLM rounds and OAuth 2.0 token requests. Secrets are masked by default.
  import type { NetBodyPreview, NetEntry, NetTimings } from "$lib/api";
  import { t, type MessageKey } from "$lib/i18n/index.svelte";
  import { formatBytes } from "$lib/largebody/format";
  import { entryToCurl, maskValue, isSecretHeader, statusClass, timeOf } from "$lib/netlog/format";
  import { netlog } from "$lib/stores/netlog.svelte";
  import { ws } from "$lib/stores/workspace.svelte";
  import { suppressTextSelection } from "$lib/dragSelect";

  let filter = $state("");
  let onlyErrors = $state(false);
  let reveal = $state(false);
  let copied = $state(false);
  /** Row whose cURL was just copied (feedback on the row button). */
  let copiedId = $state<number | null>(null);

  const shown = $derived.by(() => {
    const f = filter.trim().toLowerCase();
    return netlog.entries.filter((e) => {
      if (onlyErrors && statusClass(e) !== "err" && statusClass(e) !== "warn") return false;
      if (!f) return true;
      return (
        e.url.toLowerCase().includes(f) ||
        e.label.toLowerCase().includes(f) ||
        e.method.toLowerCase().includes(f) ||
        String(e.status ?? "").includes(f)
      );
    });
  });
  const current = $derived(netlog.entries.find((e) => e.id === netlog.selected) ?? null);
  const errors = $derived(netlog.entries.filter((e) => statusClass(e) === "err").length);

  const PHASES: Record<string, MessageKey> = {
    redirect: "netlog.phase.redirect",
    "digest-response": "netlog.phase.digest",
    "ntlm-negotiate": "netlog.phase.ntlm1",
    "ntlm-authenticate": "netlog.phase.ntlm2",
    "oauth2-token": "netlog.phase.oauth2",
  };
  const phaseLabel = (p: string) => (PHASES[p] ? t(PHASES[p]) : p);

  // Keep the newest row in view while the list is scrolled to the bottom.
  let list = $state<HTMLDivElement>();
  let stick = true;
  $effect.pre(() => {
    void shown.length;
    if (list) stick = list.scrollTop + list.clientHeight >= list.scrollHeight - 4;
  });
  $effect(() => {
    void shown.length;
    if (list && stick) list.scrollTop = list.scrollHeight;
  });

  /** Phases for the waterfall bar (only the ones that happened). */
  function phases(tm: NetTimings): { key: string; label: string; ms: number }[] {
    const out = [
      { key: "dns", label: t("netlog.t.dns"), ms: tm.dns_ms },
      { key: "connect", label: t("netlog.t.connect"), ms: tm.connect_ms },
      { key: "wait", label: t("netlog.t.wait"), ms: tm.wait_ms },
      { key: "download", label: t("netlog.t.download"), ms: tm.download_ms },
    ];
    return out.filter((p): p is { key: string; label: string; ms: number } => p.ms !== null);
  }

  function value(name: string, v: string) {
    return reveal ? v : maskValue(name, v);
  }

  async function copyCurl(e: NetEntry) {
    try {
      await navigator.clipboard.writeText(entryToCurl(e));
      copied = true;
      copiedId = e.id;
      setTimeout(() => {
        copied = false;
        copiedId = null;
      }, 1500);
    } catch {
      // ignore
    }
  }

  function resize(e: PointerEvent) {
    // No text selection while resizing.
    e.preventDefault();
    const restoreSelection = suppressTextSelection();
    const startY = e.clientY;
    const startH = netlog.height;
    const move = (ev: PointerEvent) => {
      netlog.height = Math.min(Math.max(startH - (ev.clientY - startY), 120), window.innerHeight - 160);
    };
    const up = () => {
      restoreSelection();
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      netlog.save();
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }
</script>

{#snippet headers(list: [string, string][])}
  {#if list.length}
    <table class="kv">
      <tbody>
        {#each list as [k, v], i (i)}
          <tr>
            <th>{k}</th>
            <td class:secret={isSecretHeader(k) && !reveal}>{value(k, v)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {:else}
    <p class="muted">—</p>
  {/if}
{/snippet}

{#snippet bodyView(b: NetBodyPreview | null)}
  {#if !b}
    <p class="muted">—</p>
  {:else if b.binary}
    <p class="muted">{t("netlog.binaryBody", { size: formatBytes(b.size) })}</p>
  {:else}
    <pre class="body">{b.text}</pre>
    {#if b.truncated}<p class="muted">{t("netlog.bodyCut", { size: formatBytes(b.size) })}</p>{/if}
  {/if}
{/snippet}

<section class="console" style:height="{netlog.height}px" aria-label={t("netlog.title")}>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="grip" onpointerdown={resize}></div>
  <header>
    <strong>{t("netlog.title")}</strong>
    <span class="muted">{netlog.entries.length}{#if errors}<span class="errcount"> · {t("netlog.errors", { n: errors })}</span>{/if}</span>
    <input class="filter" type="search" bind:value={filter} placeholder={t("netlog.filter")} spellcheck="false" />
    <label class="opt"><input type="checkbox" bind:checked={onlyErrors} /> {t("netlog.onlyErrors")}</label>
    <label class="opt"><input type="checkbox" bind:checked={reveal} /> {t("netlog.reveal")}</label>
    <button class="ghost small" onclick={() => netlog.clear()}>{t("netlog.clear")}</button>
    <button class="ghost small" aria-label={t("netlog.close")} title={t("netlog.close")} onclick={() => netlog.toggle()}>×</button>
  </header>

  <div class="split">
    <div class="list" bind:this={list} role="listbox" aria-label={t("netlog.title")}>
      {#if !shown.length}
        <p class="muted pad">{netlog.entries.length ? t("netlog.noMatches") : t("netlog.empty")}</p>
      {/if}
      {#each shown as e (e.id)}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <div
          class="row"
          class:sel={netlog.selected === e.id}
          class:sub={e.phase !== null}
          role="option"
          tabindex="-1"
          aria-selected={netlog.selected === e.id}
          onclick={() => (netlog.selected = e.id)}
          oncontextmenu={(ev) => {
            ev.preventDefault();
            netlog.selected = e.id;
            ws.menu = {
              x: ev.clientX,
              y: ev.clientY,
              items: [
                { label: t("netlog.copyCurl"), action: () => copyCurl(e) },
                { label: t("netlog.copyUrl"), action: () => void navigator.clipboard.writeText(e.url).catch(() => {}) },
              ],
            };
          }}
        >
          <span class="time">{timeOf(e.timestamp_ms)}</span>
          <span class="method">{e.method}</span>
          <span class="status {statusClass(e)}">{e.error ? "ERR" : (e.status ?? "—")}</span>
          {#if e.phase}<span class="phase">{phaseLabel(e.phase)}</span>{/if}
          <span class="url" title={e.url}>{e.url}</span>
          <span class="label" title={e.label}>{e.label}</span>
          <span class="ms">{Math.round(e.elapsed_ms)} ms</span>
          <button
            class="rowcurl ghost"
            title={t("netlog.copyCurl")}
            aria-label={t("netlog.copyCurl")}
            onclick={(ev) => {
              ev.stopPropagation();
              void copyCurl(e);
            }}>{copiedId === e.id ? "✓" : "cURL"}</button
          >
        </div>
      {/each}
    </div>

    <div class="detail">
      {#if current}
        <div class="dhead">
          <span class="method">{current.method}</span>
          <span class="url sel-url">{current.url}</span>
          <button class="ghost small" onclick={() => copyCurl(current)}>{copied ? t("netlog.copied") : t("netlog.copyCurl")}</button>
        </div>
        {#if current.error}<p class="error">{current.error}</p>{/if}
        <h4>{t("netlog.general")}</h4>
        <table class="kv">
          <tbody>
            <tr><th>{t("netlog.source")}</th><td>{current.label}{#if current.phase} · {phaseLabel(current.phase)}{/if}</td></tr>
            <tr><th>{t("netlog.status")}</th><td>{current.status ?? "—"} {current.status_text ?? ""}</td></tr>
            <tr><th>{t("netlog.time")}</th><td>{timeOf(current.timestamp_ms)} · {current.elapsed_ms.toFixed(1)} ms</td></tr>
            {#if current.http_version}<tr><th>HTTP</th><td>{current.http_version}</td></tr>{/if}
            {#if current.remote_addr}<tr><th>{t("netlog.remote")}</th><td>{current.remote_addr}</td></tr>{/if}
            {#if current.proxy}<tr><th>Proxy</th><td>{current.proxy}</td></tr>{/if}
          </tbody>
        </table>
        {#if current.timings}
          {@const ph = phases(current.timings)}
          {@const total = ph.reduce((a, p) => a + p.ms, 0) || 1}
          <h4>{t("netlog.timings")}</h4>
          <div class="waterfall" role="img" aria-label={t("netlog.timings")}>
            {#each ph as p (p.key)}
              <span class="seg {p.key}" style:flex-grow={Math.max(p.ms / total, 0.01)} title="{p.label}: {p.ms.toFixed(1)} ms"></span>
            {/each}
          </div>
          <table class="kv">
            <tbody>
              {#each ph as p (p.key)}
                <tr><th><span class="dot {p.key}"></span>{p.label}</th><td>{p.ms.toFixed(1)} ms</td></tr>
              {/each}
              {#if current.timings.reused}
                <tr><th></th><td class="muted">{t("netlog.t.reused")}</td></tr>
              {/if}
            </tbody>
          </table>
        {/if}
        <h4>{t("netlog.requestHeaders")}</h4>
        {@render headers(current.request_headers)}
        <h4>{t("netlog.requestBody")}</h4>
        {@render bodyView(current.request_body)}
        <h4>{t("netlog.responseHeaders")}</h4>
        {@render headers(current.response_headers)}
        <h4>{t("netlog.responseBody")}</h4>
        {@render bodyView(current.response_body)}
      {:else}
        <p class="muted pad">{t("netlog.pick")}</p>
      {/if}
    </div>
  </div>
</section>

<style>
  .console {
    position: relative;
    flex: none;
    display: flex;
    flex-direction: column;
    min-height: 0;
    border-top: 1px solid var(--border);
    background: var(--surface);
    font-size: 12px;
  }
  .grip {
    position: absolute;
    top: -3px;
    left: 0;
    right: 0;
    height: 6px;
    cursor: ns-resize;
    z-index: 1;
  }
  header {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 4px 8px;
    border-bottom: 1px solid var(--border);
    flex-wrap: wrap;
  }
  .filter {
    flex: 1;
    min-width: 120px;
    max-width: 320px;
    padding: 2px 8px;
    font-size: 12px;
  }
  .opt {
    display: flex;
    align-items: center;
    gap: 4px;
    color: var(--muted);
    white-space: nowrap;
  }
  .muted {
    color: var(--muted);
  }
  .errcount {
    color: var(--err);
  }
  .pad {
    padding: 10px;
    margin: 0;
  }
  .split {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(0, 3fr) minmax(0, 2fr);
  }
  .list {
    overflow: auto;
    border-right: 1px solid var(--border);
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    border: none;
    border-radius: 0;
    background: none;
    padding: 2px 8px;
    text-align: left;
    font-family: var(--mono);
    font-size: 11.5px;
    white-space: nowrap;
  }
  .row:hover {
    background: var(--hover);
  }
  .row {
    cursor: default;
  }
  .rowcurl {
    flex: none;
    visibility: hidden;
    padding: 0 6px;
    font-size: 10.5px;
    font-family: var(--mono);
    border: 1px solid var(--border);
    border-radius: 4px;
  }
  .row:hover .rowcurl,
  .row.sel .rowcurl,
  .rowcurl:focus-visible {
    visibility: visible;
  }
  .row.sel {
    background: var(--selected);
  }
  .row.sub {
    padding-left: 20px;
  }
  .time {
    color: var(--muted);
    flex: none;
  }
  .method {
    flex: none;
    font-weight: 600;
    min-width: 44px;
  }
  .status {
    flex: none;
    min-width: 30px;
  }
  .status.ok {
    color: var(--ok);
  }
  .status.redirect {
    color: var(--accent);
  }
  .status.warn {
    color: var(--warn);
  }
  .status.err {
    color: var(--err);
  }
  .phase {
    flex: none;
    font-family: inherit;
    font-size: 10.5px;
    padding: 0 6px;
    border-radius: 8px;
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 14%, transparent);
  }
  .url {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .label {
    flex: none;
    max-width: 180px;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--muted);
    font-family: system-ui, sans-serif;
  }
  .ms {
    flex: none;
    color: var(--muted);
    min-width: 54px;
    text-align: right;
  }
  .detail {
    overflow: auto;
    padding: 6px 10px 12px;
  }
  .dhead {
    display: flex;
    align-items: center;
    gap: 8px;
    font-family: var(--mono);
  }
  .sel-url {
    word-break: break-all;
    white-space: normal;
  }
  .dhead button {
    margin-left: auto;
    flex: none;
  }
  h4 {
    margin: 10px 0 4px;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
  }
  .kv {
    border-collapse: collapse;
    width: 100%;
    font-family: var(--mono);
    font-size: 11.5px;
  }
  .kv th {
    text-align: left;
    font-weight: 600;
    padding: 1px 10px 1px 0;
    vertical-align: top;
    white-space: nowrap;
    width: 1%;
  }
  .kv td {
    word-break: break-all;
    padding: 1px 0;
  }
  .kv td.secret {
    color: var(--muted);
  }
  .body {
    margin: 0;
    padding: 6px 8px;
    max-height: 240px;
    overflow: auto;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 4px;
    font-family: var(--mono);
    font-size: 11.5px;
    white-space: pre-wrap;
    word-break: break-all;
  }
  .waterfall {
    display: flex;
    height: 10px;
    border-radius: 3px;
    overflow: hidden;
    gap: 1px;
    margin-bottom: 4px;
    background: var(--border);
  }
  .seg {
    min-width: 2px;
  }
  .dot {
    display: inline-block;
    width: 8px;
    height: 8px;
    border-radius: 2px;
    margin-right: 6px;
  }
  .dns {
    background: var(--syntax-keyword);
  }
  .connect {
    background: var(--warn);
  }
  .wait {
    background: var(--accent);
  }
  .download {
    background: var(--ok);
  }
  .error {
    color: var(--err);
    margin: 6px 0;
    word-break: break-word;
  }
</style>
