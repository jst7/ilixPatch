<script lang="ts">
  // Compact network details of a response: HTTP version, remote address, TLS lock + certificate,
  // redirect chain and proxy, in a popover.
  import type { ResponseNetworkInfo } from "$lib/api";
  import { t } from "$lib/i18n/index.svelte";

  interface Props {
    network: ResponseNetworkInfo;
  }

  let { network }: Props = $props();

  let open = $state(false);
  let root: HTMLSpanElement | undefined = $state();

  const tls = $derived(network.tls);
  const cert = $derived(tls?.peer_certificate ?? null);
  const hasDetails = $derived(!!tls || network.redirects.length > 0 || !!network.proxy);

  const DAY = 86_400_000;
  const expiry = $derived.by(() => {
    if (!cert) return null;
    const end = Date.parse(cert.not_after);
    if (Number.isNaN(end)) return null;
    const days = Math.floor((end - Date.now()) / DAY);
    return { days, bad: days < 14 };
  });

  function fmtDate(iso: string): string {
    const d = new Date(iso);
    return Number.isNaN(d.getTime()) ? iso : d.toLocaleString();
  }

  function onWindowPointer(e: PointerEvent) {
    if (open && root && !root.contains(e.target as Node)) open = false;
  }
  function onKey(e: KeyboardEvent) {
    if (open && e.key === "Escape") open = false;
  }
</script>

<svelte:window onpointerdown={onWindowPointer} onkeydown={onKey} />

<span class="net" bind:this={root}>
  <span class="meta">{network.http_version}</span>
  {#if network.remote_addr}<span class="meta mono">{network.remote_addr}</span>{/if}
  {#if hasDetails}
    <button
      class="trigger"
      aria-expanded={open}
      title={t("netinfo.details")}
      aria-label={t("netinfo.details")}
      onclick={() => (open = !open)}
    >
      {#if tls}
        <svg
          class="lock"
          class:verified={tls.verified}
          class:unverified={!tls.verified}
          viewBox="0 0 24 24"
          width="13"
          height="13"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          stroke-linecap="round"
          stroke-linejoin="round"
          aria-hidden="true"
        >
          <rect x="5" y="11" width="14" height="10" rx="2" />
          <path d={tls.verified ? "M8 11V7a4 4 0 018 0v4" : "M8 11V7a4 4 0 017.5-2"} />
        </svg>
      {/if}
      {#if network.redirects.length}
        <span class="redir">{t("netinfo.redirects", { n: network.redirects.length })}</span>
      {/if}
      {#if network.proxy}<span class="redir">{t("netinfo.proxyShort")}</span>{/if}
    </button>
  {/if}

  {#if open}
    <div class="pop" role="dialog" aria-label={t("netinfo.details")}>
      {#if tls}
        <div class="sec">
          <div class="sec-title" class:ok={tls.verified} class:warn={!tls.verified}>
            {tls.verified ? t("netinfo.tlsVerified") : t("netinfo.tlsUnverified")}
          </div>
          {#if cert}
            <dl>
              <dt>{t("netinfo.subject")}</dt>
              <dd class="mono">{cert.subject}</dd>
              <dt>{t("netinfo.issuer")}</dt>
              <dd class="mono">{cert.issuer}</dd>
              <dt>{t("netinfo.validFrom")}</dt>
              <dd>{fmtDate(cert.not_before)}</dd>
              <dt>{t("netinfo.validTo")}</dt>
              <dd>
                {fmtDate(cert.not_after)}
                {#if expiry}
                  <span class:bad={expiry.bad} class="exp">
                    {expiry.days < 0 ? t("netinfo.expired") : t("netinfo.expiresIn", { n: expiry.days })}
                  </span>
                {/if}
              </dd>
              <dt>{t("netinfo.sha256")}</dt>
              <dd class="mono fp">{cert.sha256}</dd>
              {#if cert.san.length}
                <dt>{t("netinfo.san")}</dt>
                <dd class="mono">{cert.san.join(", ")}</dd>
              {/if}
            </dl>
          {:else}
            <p class="muted">{t("netinfo.noCert")}</p>
          {/if}
        </div>
      {/if}
      {#if network.redirects.length}
        <div class="sec">
          <div class="sec-title">{t("netinfo.redirects", { n: network.redirects.length })}</div>
          <ol class="chain">
            {#each network.redirects as r, i (i)}
              <li><span class="code mono">{r.status}</span><span class="mono url">{r.url}</span></li>
            {/each}
            <li class="final"><span class="code mono">→</span><span class="mono url">{network.final_url}</span></li>
          </ol>
        </div>
      {/if}
      {#if network.proxy}
        <div class="sec">
          <div class="sec-title">{t("netinfo.proxy")}</div>
          <div class="mono small">{network.proxy}</div>
        </div>
      {/if}
    </div>
  {/if}
</span>

<style>
  .net {
    position: relative;
    display: inline-flex;
    align-items: baseline;
    gap: 10px;
  }
  .meta {
    color: var(--muted);
    font-size: 13px;
  }
  .mono {
    font-family: var(--mono);
  }
  .trigger {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    border: none;
    background: none;
    padding: 1px 4px;
    color: var(--muted);
    font-size: 12px;
    align-self: center;
  }
  .trigger:hover {
    color: var(--text);
    background: var(--hover);
  }
  .lock.verified {
    color: var(--ok);
  }
  .lock.unverified {
    color: var(--warn);
  }
  .pop {
    position: absolute;
    left: 0;
    top: calc(100% + 6px);
    width: min(460px, 80vw);
    max-height: 60vh;
    overflow: auto;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: var(--shadow);
    padding: 10px 12px;
    z-index: 40;
    font-size: 12px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .sec-title {
    font-weight: 600;
    font-size: 12px;
    margin-bottom: 4px;
  }
  .sec-title.ok {
    color: var(--ok);
  }
  .sec-title.warn {
    color: var(--warn);
  }
  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 3px 10px;
    margin: 0;
  }
  dt {
    color: var(--muted);
  }
  dd {
    margin: 0;
    word-break: break-word;
  }
  .fp {
    font-size: 11px;
    word-break: break-all;
  }
  .exp {
    margin-left: 6px;
    color: var(--muted);
  }
  .exp.bad {
    color: var(--err);
    font-weight: 600;
  }
  .muted {
    color: var(--muted);
    margin: 0;
  }
  .small {
    font-size: 12px;
    word-break: break-all;
  }
  .chain {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .chain li {
    display: flex;
    gap: 8px;
  }
  .code {
    flex: none;
    width: 32px;
    color: var(--warn);
    font-weight: 600;
  }
  .final .code {
    color: var(--ok);
  }
  .url {
    word-break: break-all;
  }
</style>
