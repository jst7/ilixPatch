<script lang="ts">
  import type { LoadProfile, LoadSnapshot } from "$lib/api";
  import { t, type MessageKey } from "$lib/i18n/index.svelte";
  import { ws } from "$lib/stores/workspace.svelte";
  import { loadSession, vuAt } from "$lib/stores/runs.svelte";
  import LoadChart from "./LoadChart.svelte";
  import { methodColor, methodLabel } from "./methods";
  import EnvSelect from "./EnvSelect.svelte";
  import { formatDuration, resolveTarget } from "./target";

  interface Props {
    target: string;
  }

  let { target }: Props = $props();

  const session = $derived(loadSession(target));
  const info = $derived(resolveTarget(ws.entries, target));
  const config = $derived(session.config);

  $effect(() => {
    if (info && !session.envTouched) session.config.env = ws.resolveEnv(info.collection)?.choice ?? null;
  });

  const PROFILES: { id: LoadProfile; label: MessageKey }[] = [
    { id: "fixed", label: "load.profile.fixed" },
    { id: "ramp_up", label: "load.profile.ramp_up" },
    { id: "spike", label: "load.profile.spike" },
    { id: "peak", label: "load.profile.peak" },
  ];
  const usesRamp = $derived(config.profile === "ramp_up" || config.profile === "peak");

  // ----- VU curve preview -----
  const PW = 220;
  const PH = 56;
  const preview = $derived.by(() => {
    const d = Math.max(1, Number(config.duration_secs) || 1);
    const v = Math.max(1, Number(config.virtual_users) || 1);
    const r = Number(config.ramp_up_secs) || 0;
    const n = 80;
    let path = "";
    for (let i = 0; i <= n; i++) {
      const time = (d * i) / n;
      const y = vuAt(config.profile, time, v, d, r);
      const px = 2 + (i / n) * (PW - 4);
      const py = PH - 2 - (y / v) * (PH - 6);
      path += `${i ? "L" : "M"}${px.toFixed(1)},${py.toFixed(1)}`;
    }
    return { line: path, area: `${path}L${PW - 2},${PH - 2}L2,${PH - 2}Z` };
  });

  // ----- live data -----
  const snap = $derived<LoadSnapshot | null>(session.final ?? session.latest);
  const MAX_POINTS = 600;

  const series = $derived.by(() => {
    void session.version;
    const all = session.snapshots;
    const stride = Math.max(1, Math.ceil(all.length / MAX_POINTS));
    const rps: [number, number][] = [];
    const p95: [number, number][] = [];
    const vus: [number, number][] = [];
    for (let i = 0; i < all.length; i += stride) {
      const s = all[i];
      rps.push([s.elapsed_secs, s.rps]);
      p95.push([s.elapsed_secs, s.p95_ms]);
      vus.push([s.elapsed_secs, s.active_vus]);
    }
    const last = all[all.length - 1];
    if (last && rps[rps.length - 1]?.[0] !== last.elapsed_secs) {
      rps.push([last.elapsed_secs, last.rps]);
      p95.push([last.elapsed_secs, last.p95_ms]);
      vus.push([last.elapsed_secs, last.active_vus]);
    }
    const xMax = Math.max(session.runDuration, last?.elapsed_secs ?? 0, 1);
    return { rps, p95, vus, xMax };
  });

  let hoverX = $state<number | null>(null);

  const perRequest = $derived(snap ? [...snap.per_request].sort((a, b) => b.count - a.count) : []);

  const num = (v: number) => (v >= 100 ? Math.round(v).toLocaleString() : (Math.round(v * 10) / 10).toLocaleString());
  const ms = (v: number) => formatDuration(v);
  const pct = (v: number) => `${(v * 100).toFixed(v > 0 && v < 0.01 ? 2 : 1)}%`;
</script>

<section class="load">
  <header class="head">
    <div class="title">
      <span class="kind">{t("load.title")}</span>
      {#if info}<span class="crumbs">{info.projectName} / {info.crumbs.join(" / ")}</span>{/if}
    </div>
    <div class="actions">
      {#if session.running}
        <button class="danger" onclick={() => session.stop()}>{t("load.stop")}</button>
      {/if}
      <button class="primary" disabled={session.running || !info} onclick={() => session.start()}>
        {session.running ? t("load.running") : t("load.start")}
      </button>
    </div>
  </header>

  {#if !info}
    <p class="empty">{t("runner.notFound")}</p>
  {:else}
    <div class="body">
      <aside class="config" aria-label={t("runner.config")}>
        <div class="section-title">{t("runner.config")}</div>
        <label>
          <span>{t("load.vus")}</span>
          <input type="number" min="1" bind:value={session.config.virtual_users} disabled={session.running} />
        </label>
        <label>
          <span>{t("load.duration")}</span>
          <input type="number" min="1" bind:value={session.config.duration_secs} disabled={session.running} />
        </label>
        <div class="field">
          <span>{t("load.profile")}</span>
          <div class="seg" role="radiogroup" aria-label={t("load.profile")}>
            {#each PROFILES as p (p.id)}
              <button
                role="radio"
                aria-checked={config.profile === p.id}
                class:active={config.profile === p.id}
                disabled={session.running}
                onclick={() => (session.config.profile = p.id)}>{t(p.label)}</button
              >
            {/each}
          </div>
          <svg class="preview" viewBox="0 0 {PW} {PH}" role="img" aria-label={t("load.preview")}>
            <title>{t("load.preview")}</title>
            <line x1="2" x2={PW - 2} y1={PH - 2} y2={PH - 2} class="base" />
            <path d={preview.area} class="p-area" />
            <path d={preview.line} class="p-line" />
          </svg>
        </div>
        {#if usesRamp}
          <label>
            <span>{t("load.rampUp")}</span>
            <input type="number" min="0" bind:value={session.config.ramp_up_secs} disabled={session.running} />
          </label>
        {/if}
        <label>
          <span>{t("load.thinkTime")}</span>
          <input type="number" min="0" step="50" bind:value={session.config.think_time_ms} disabled={session.running} />
        </label>
        <div class="field">
          <span>{t("request.environment")}</span>
          <EnvSelect
            projectPath={ws.projectOf(info.collection)?.path ?? null}
            collection={info.collection}
            value={config.env}
            disabled={session.running}
            onchange={(c) => {
              session.envTouched = true;
              session.config.env = c;
            }}
          />
        </div>
        <label class="check">
          <input type="checkbox" bind:checked={session.config.run_scripts} disabled={session.running} />
          <span>{t("load.runScripts")}</span>
        </label>
      </aside>

      <div class="dash">
        {#if session.error}
          <div class="error" role="alert"><pre>{session.error}</pre></div>
        {/if}
        {#if !session.started}
          <p class="empty">{t("load.empty")}</p>
        {:else if !snap}
          <p class="empty">{session.running ? t("load.waiting") : ""}</p>
        {:else}
          {#if session.final}
            <div class="section-title">{t("load.summary")}</div>
          {/if}
          <div class="tiles">
            <div class="tile"><span>{t("load.stat.elapsed")}</span><strong>{t("common.seconds", { n: Math.round(snap.elapsed_secs) })}</strong></div>
            <div class="tile"><span>{t("load.stat.vus")}</span><strong>{snap.active_vus}</strong></div>
            <div class="tile"><span>{t("load.stat.requests")}</span><strong>{snap.requests.toLocaleString()}</strong></div>
            <div class="tile"><span>{t("load.stat.rps")}</span><strong>{num(snap.rps)}</strong></div>
            <div class="tile" class:bad={snap.error_rate > 0}>
              <span>{t("load.stat.errorRate")}</span><strong>{pct(snap.error_rate)}</strong>
              <small>{t("load.stat.errors")}: {snap.errors.toLocaleString()}</small>
            </div>
            <div class="tile"><span>{t("load.stat.avg")}</span><strong>{ms(snap.avg_ms)}</strong></div>
            <div class="tile"><span>{t("load.stat.p95")}</span><strong>{ms(snap.p95_ms)}</strong></div>
            <div class="tile"><span>{t("load.stat.p99")}</span><strong>{ms(snap.p99_ms)}</strong></div>
            {#if session.final}
              <div class="tile"><span>{t("load.stat.p50")}</span><strong>{ms(snap.p50_ms)}</strong></div>
              <div class="tile"><span>{t("load.stat.p90")}</span><strong>{ms(snap.p90_ms)}</strong></div>
              <div class="tile"><span>{t("load.stat.max")}</span><strong>{ms(snap.max_ms)}</strong></div>
            {/if}
          </div>

          <div class="section-title">{t("load.chart.title")}</div>
          <div class="charts">
            <LoadChart
              title={t("load.chart.rps")}
              points={series.rps}
              xMax={series.xMax}
              color="var(--accent)"
              format={num}
              {hoverX}
              onhover={(x) => (hoverX = x)}
            />
            <LoadChart
              title={t("load.chart.p95")}
              points={series.p95}
              xMax={series.xMax}
              color="var(--warn)"
              format={(v) => num(v)}
              {hoverX}
              onhover={(x) => (hoverX = x)}
            />
            <LoadChart
              title={t("load.chart.vus")}
              points={series.vus}
              xMax={series.xMax}
              color="var(--muted)"
              format={(v) => String(Math.round(v))}
              area
              {hoverX}
              onhover={(x) => (hoverX = x)}
            />
          </div>

          {#if perRequest.length}
            <table>
              <thead>
                <tr>
                  <th>{t("load.table.request")}</th>
                  <th class="r">{t("load.table.count")}</th>
                  <th class="r">{t("load.table.errors")}</th>
                  <th class="r">{t("load.table.avg")}</th>
                  <th class="r">{t("load.table.p95")}</th>
                </tr>
              </thead>
              <tbody>
                {#each perRequest as r (r.path)}
                  <tr>
                    <td>
                      <span class="method mono" style:color={methodColor(r.method)}>{methodLabel(r.method)}</span>
                      {r.name}
                    </td>
                    <td class="r">{r.count.toLocaleString()}</td>
                    <td class="r" class:err={r.errors > 0}>{r.errors.toLocaleString()}</td>
                    <td class="r">{num(r.avg_ms)}</td>
                    <td class="r">{num(r.p95_ms)}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          {/if}
        {/if}
      </div>
    </div>
  {/if}
</section>

<style>
  .load {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    padding: 12px;
    gap: 10px;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .title {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: baseline;
    gap: 10px;
  }
  .kind {
    font-weight: 600;
    font-size: 15px;
    flex: none;
  }
  .crumbs {
    color: var(--muted);
    font-size: 13px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .actions {
    display: flex;
    gap: 6px;
  }
  .danger {
    color: var(--err);
    border-color: var(--err);
  }
  .body {
    flex: 1;
    min-height: 0;
    display: flex;
    gap: 14px;
  }
  .config {
    width: 250px;
    flex: none;
    overflow: auto;
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding-right: 12px;
    border-right: 1px solid var(--border);
  }
  .section-title {
    font-weight: 600;
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--muted);
  }
  label,
  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 12px;
    color: var(--muted);
  }
  label input:not([type="checkbox"]) {
    color: var(--text);
    font-size: 13px;
    padding: 5px 8px;
  }
  label.check {
    flex-direction: row;
    align-items: center;
    gap: 8px;
    color: var(--text);
    font-size: 13px;
  }
  label.check input {
    accent-color: var(--accent);
    margin: 0;
  }
  .seg {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 4px;
  }
  .seg button {
    font-size: 12px;
    padding: 4px 6px;
    color: var(--muted);
  }
  .seg button.active {
    color: var(--text);
    border-color: var(--accent);
    background: var(--surface-2);
  }
  .preview {
    width: 100%;
    height: auto;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: 6px;
  }
  .preview .base {
    stroke: var(--border);
  }
  .p-area {
    fill: var(--accent);
    opacity: 0.15;
  }
  .p-line {
    fill: none;
    stroke: var(--accent);
    stroke-width: 2;
    stroke-linejoin: round;
  }
  .dash {
    flex: 1;
    min-width: 0;
    overflow: auto;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .empty {
    color: var(--muted);
    margin: auto;
    text-align: center;
  }
  .error {
    color: var(--err);
    border: 1px solid var(--err);
    border-radius: 6px;
    padding: 6px 10px;
  }
  .error pre {
    margin: 0;
    white-space: pre-wrap;
    word-break: break-word;
    font-family: var(--mono);
    font-size: 12px;
  }
  .tiles {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(120px, 1fr));
    gap: 8px;
  }
  .tile {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 8px 10px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--surface);
  }
  .tile span,
  .tile small {
    font-size: 11px;
    color: var(--muted);
  }
  .tile strong {
    font-size: 18px;
    font-variant-numeric: tabular-nums;
  }
  .tile.bad strong {
    color: var(--err);
  }
  .charts {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 10px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--surface);
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 13px;
  }
  th {
    text-align: left;
    font-weight: 500;
    font-size: 12px;
    color: var(--muted);
    padding: 6px 8px;
    border-bottom: 1px solid var(--border);
  }
  td {
    padding: 5px 8px;
    border-bottom: 1px solid var(--border);
    font-variant-numeric: tabular-nums;
  }
  .r {
    text-align: right;
  }
  td.err {
    color: var(--err);
    font-weight: 600;
  }
  .method {
    font-weight: 700;
    font-size: 11px;
    display: inline-block;
    width: 38px;
  }
</style>
