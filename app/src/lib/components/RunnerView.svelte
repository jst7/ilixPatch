<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import type { RequestRunResult } from "$lib/api";
  import { t } from "$lib/i18n/index.svelte";
  import { ws } from "$lib/stores/workspace.svelte";
  import { resultFailed, runnerSession } from "$lib/stores/runs.svelte";
  import { methodColor, methodLabel } from "./methods";
  import EnvSelect from "./EnvSelect.svelte";
  import { formatDuration, formatSize, resolveTarget } from "./target";
  import { describeRequestError } from "$lib/requestError";

  interface Props {
    target: string;
  }

  let { target }: Props = $props();

  const session = $derived(runnerSession(target));
  const info = $derived(resolveTarget(ws.entries, target));
  const config = $derived(session.config);

  // Follow the environment chosen in the request editor until the user picks one here.
  $effect(() => {
    if (info && !session.envTouched) session.config.env = ws.resolveEnv(info.collection)?.choice ?? null;
  });

  type Filter = "all" | "passed" | "failed";
  const PAGE = 300;
  let filter = $state<Filter>("all");
  let limit = $state(PAGE);
  let expanded = $state<Record<number, boolean>>({});

  async function pickDataFile() {
    try {
      const file = await open({
        multiple: false,
        directory: false,
        filters: [{ name: t("runner.dataFileFilter"), extensions: ["csv", "json"] }],
      });
      if (typeof file === "string") session.config.data_file = file;
    } catch (e) {
      ws.fail(e);
    }
  }

  function run() {
    expanded = {};
    limit = PAGE;
    session.run();
  }

  function fileName(p: string): string {
    return p.split(/[\\/]/).pop() ?? p;
  }

  interface Row {
    i: number;
    r: RequestRunResult;
    /** Iteration header shown before this row. */
    header: number | null;
  }

  // O(visible rows) per update: reads only the first `limit` indices of the chosen list.
  const view = $derived.by(() => {
    void session.version;
    const all = session.results;
    const idx = filter === "passed" ? session.passedIdx : filter === "failed" ? session.failedIdx : null;
    const n = idx ? idx.length : all.length;
    const count = Math.min(n, limit);
    const rows: Row[] = [];
    let prevIter = -1;
    for (let k = 0; k < count; k++) {
      const i = idx ? idx[k] : k;
      const r = all[i];
      rows.push({ i, r, header: r.iteration !== prevIter ? r.iteration : null });
      prevIter = r.iteration;
    }
    return { rows, remaining: n - count, total: n };
  });

  const progressTotal = $derived(
    session.total >= session.done || session.iterations <= 1 ? session.total : session.total * session.iterations,
  );
  const progress = $derived(progressTotal > 0 ? Math.min(1, session.done / progressTotal) : 0);
  const passedCount = $derived(session.done - session.failedCount);

  /** Localized title + cause for a failed request (null when there is no error kind). */
  function errorInfo(r: RequestRunResult) {
    if (!r.error_kind || !r.error) return null;
    return describeRequestError({ kind: r.error_kind, message: r.error, detail: r.error, url: r.url });
  }

  function testStats(r: RequestRunResult) {
    let passed = 0;
    let failed = 0;
    for (const x of r.tests) {
      if (x.skipped) continue;
      if (x.passed) passed++;
      else failed++;
    }
    return { passed, failed, total: r.tests.length };
  }

  const summary = $derived(session.summary);
</script>

<section class="runner">
  <header class="head">
    <div class="title">
      <span class="kind">{t("runner.title")}</span>
      {#if info}
        <span class="crumbs">{info.projectName} / {info.crumbs.join(" / ")}</span>
      {/if}
    </div>
    <div class="actions">
      {#if session.running}
        <button onclick={() => session.cancel()}>{t("runner.cancel")}</button>
      {/if}
      <button class="primary" disabled={session.running || !info} onclick={run}>
        {session.running ? t("runner.running") : t("runner.run")}
      </button>
    </div>
  </header>

  {#if !info}
    <p class="empty">{t("runner.notFound")}</p>
  {:else}
    <div class="body">
      <aside class="config" aria-label={t("runner.config")}>
        <div class="section-title">{t("runner.config")}</div>
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
        <label>
          <span>{t("runner.iterations")}</span>
          <input type="number" min="1" bind:value={session.config.iterations} disabled={session.running} />
        </label>
        <label>
          <span>{t("runner.delay")}</span>
          <input type="number" min="0" step="50" bind:value={session.config.delay_ms} disabled={session.running} />
        </label>
        <label>
          <span>{t("runner.timeout")}</span>
          <input type="number" min="0" step="500" bind:value={session.config.timeout_ms} disabled={session.running} />
        </label>
        <div class="field">
          <span>{t("runner.dataFile")}</span>
          <div class="file-row">
            <span class="file mono" title={config.data_file ?? ""}>
              {config.data_file ? fileName(config.data_file) : t("runner.dataFileNone")}
            </span>
            {#if config.data_file}
              <button
                class="ghost small"
                disabled={session.running}
                title={t("runner.dataFileClear")}
                aria-label={t("runner.dataFileClear")}
                onclick={() => (session.config.data_file = null)}>×</button
              >
            {/if}
            <button class="small" disabled={session.running} onclick={pickDataFile}>{t("runner.dataFilePick")}</button>
          </div>
        </div>
        <label class="check">
          <input type="checkbox" bind:checked={session.config.bail} disabled={session.running} />
          <span>{t("runner.bail")}</span>
        </label>
        <label class="check">
          <input type="checkbox" bind:checked={session.config.persist_variables} disabled={session.running} />
          <span>{t("runner.persist")}</span>
        </label>
      </aside>

      <div class="results">
        {#if !session.started}
          <p class="empty">{t("runner.empty")}</p>
        {:else}
          <div class="progress-row">
            <div class="bar" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(progress * 100)}>
              <div class="fill" class:has-fail={session.failedCount > 0} style:width="{progress * 100}%"></div>
            </div>
            <span class="progress-text">
              {t("runner.progress", { done: session.done, total: progressTotal || "…" })}
            </span>
          </div>

          {#if session.error}
            <div class="error" role="alert"><pre>{session.error}</pre></div>
          {/if}

          {#if summary}
            <div class="summary-head">
              {summary.cancelled ? t("runner.cancelled") : t("runner.finished")}
            </div>
            <div class="cards">
              <div class="card"><span>{t("runner.summary.iterations")}</span><strong>{summary.iterations}</strong></div>
              <div class="card"><span>{t("runner.summary.requests")}</span><strong>{summary.requests}</strong></div>
              <div class="card" class:bad={summary.failed_requests > 0}>
                <span>{t("runner.summary.failedRequests")}</span><strong>{summary.failed_requests}</strong>
              </div>
              <div class="card good">
                <span>{t("runner.summary.testsPassed")}</span>
                <strong>{summary.tests - summary.failed_tests - summary.skipped_tests}</strong>
              </div>
              <div class="card" class:bad={summary.failed_tests > 0}>
                <span>{t("runner.summary.testsFailed")}</span><strong>{summary.failed_tests}</strong>
              </div>
              <div class="card"><span>{t("runner.summary.testsSkipped")}</span><strong>{summary.skipped_tests}</strong></div>
              <div class="card"><span>{t("runner.summary.totalTime")}</span><strong>{formatDuration(summary.total_ms)}</strong></div>
              <div class="card">
                <span>{t("runner.summary.avgResponse")}</span><strong>{formatDuration(summary.avg_response_ms)}</strong>
              </div>
            </div>
          {/if}

          <div class="filters" role="tablist">
            <button role="tab" aria-selected={filter === "all"} class:active={filter === "all"} onclick={() => ((filter = "all"), (limit = PAGE))}>
              {t("runner.filter.all")}<span class="n">{session.done}</span>
            </button>
            <button role="tab" aria-selected={filter === "passed"} class:active={filter === "passed"} onclick={() => ((filter = "passed"), (limit = PAGE))}>
              {t("runner.filter.passed")}<span class="n ok">{passedCount}</span>
            </button>
            <button role="tab" aria-selected={filter === "failed"} class:active={filter === "failed"} onclick={() => ((filter = "failed"), (limit = PAGE))}>
              {t("runner.filter.failed")}<span class="n err">{session.failedCount}</span>
            </button>
          </div>

          <div class="list">
            {#if view.total === 0 && !session.running}
              <p class="empty small-empty">{t("runner.noResults")}</p>
            {/if}
            {#each view.rows as row (row.i)}
              {#if row.header !== null}
                <div class="iter">{t("runner.iteration", { n: row.header + 1 })}</div>
              {/if}
              {@const r = row.r}
              {@const ts = testStats(r)}
              {@const failed = resultFailed(r)}
              <div class="item" class:failed>
                <button class="row" onclick={() => (expanded[row.i] = !expanded[row.i])} aria-expanded={!!expanded[row.i]}>
                  <span class="chev">{expanded[row.i] ? "▾" : "▸"}</span>
                  <span class="method mono" style:color={methodColor(r.method)}>{methodLabel(r.method)}</span>
                  <span class="name">{r.name}</span>
                  <span class="url mono">{r.url}</span>
                  {#if r.status !== null}
                    <span class="status mono" class:ok={r.status < 400}>{r.status}</span>
                  {:else}
                    {@const ei = errorInfo(r)}
                    <span class="status err-kind" title={ei ? ei.cause : (r.error ?? "")}>{ei ? ei.title : t("runner.error")}</span>
                  {/if}
                  <span class="meta">{formatDuration(r.elapsed_ms)}</span>
                  <span class="meta">{formatSize(r.size_bytes)}</span>
                  <span class="tests" class:ok={ts.failed === 0 && ts.total > 0} class:err={ts.failed > 0}>
                    {ts.total > 0 ? `${ts.passed}/${ts.total}` : "–"}
                  </span>
                </button>
                {#if expanded[row.i]}
                  <div class="detail">
                    {#if r.error}
                      {@const ei = errorInfo(r)}
                      {#if ei}
                        <div class="err-desc">
                          <strong>{ei.title}</strong>
                          <span>{ei.cause}</span>
                        </div>
                      {/if}
                      <pre class="err-text">{r.error}</pre>
                    {/if}
                    {#if r.tests.length === 0}
                      <p class="muted">{t("runner.noTests")}</p>
                    {:else}
                      <ul class="test-list">
                        {#each r.tests as x, k (k)}
                          {@const st = x.skipped ? "skip" : x.passed ? "pass" : "fail"}
                          <li class={st}>
                            <span class="badge">
                              {st === "skip"
                                ? t("response.tests.skipped")
                                : st === "pass"
                                  ? t("response.tests.passed")
                                  : t("response.tests.failed")}
                            </span>
                            <span>{x.name}</span>
                            {#if x.error}<pre class="err-text">{x.error}</pre>{/if}
                          </li>
                        {/each}
                      </ul>
                    {/if}
                    {#if r.logs.length}
                      <div class="logs-title">{t("response.tab.console")}</div>
                      {#each r.logs as line, k (k)}<pre class="log mono">{line}</pre>{/each}
                    {/if}
                  </div>
                {/if}
              </div>
            {/each}
            {#if view.remaining > 0}
              <button class="more" onclick={() => (limit += 500)}>
                {t("runner.showMore", { n: Math.min(view.remaining, 500) })}
              </button>
            {/if}
          </div>
        {/if}
      </div>
    </div>
  {/if}
</section>

<style>
  .runner {
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
  .section-title,
  .summary-head {
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
  .file-row {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .file {
    flex: 1;
    min-width: 0;
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
  }
  .small {
    padding: 3px 8px;
    font-size: 12px;
  }
  .results {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-height: 0;
  }
  .empty {
    color: var(--muted);
    margin: auto;
    text-align: center;
  }
  .small-empty {
    margin: 16px auto;
  }
  .progress-row {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .bar {
    flex: 1;
    height: 8px;
    border-radius: 4px;
    background: var(--surface-2);
    border: 1px solid var(--border);
    overflow: hidden;
  }
  .fill {
    height: 100%;
    background: var(--ok);
    transition: width 0.2s;
  }
  .fill.has-fail {
    background: var(--warn);
  }
  .progress-text {
    font-size: 12px;
    color: var(--muted);
    white-space: nowrap;
  }
  .error {
    color: var(--err);
    border: 1px solid var(--err);
    border-radius: 6px;
    padding: 6px 10px;
  }
  .error pre,
  .err-kind {
    font-size: 12px;
    max-width: 200px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .err-desc {
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: 12px;
  }
  .err-desc strong {
    color: var(--err);
  }
  .err-text {
    margin: 0;
    white-space: pre-wrap;
    word-break: break-word;
    font-family: var(--mono);
    font-size: 12px;
    color: var(--err);
  }
  .cards {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(130px, 1fr));
    gap: 8px;
  }
  .card {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 8px 10px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--surface);
  }
  .card span {
    font-size: 11px;
    color: var(--muted);
  }
  .card strong {
    font-size: 18px;
    font-variant-numeric: tabular-nums;
  }
  .card.good strong {
    color: var(--ok);
  }
  .card.bad strong {
    color: var(--err);
  }
  .filters {
    display: flex;
    gap: 4px;
  }
  .filters button {
    padding: 3px 10px;
    font-size: 12px;
    border-radius: 999px;
    background: none;
    color: var(--muted);
  }
  .filters button.active {
    color: var(--text);
    border-color: var(--accent);
    background: var(--surface-2);
  }
  .n {
    margin-left: 6px;
    font-variant-numeric: tabular-nums;
  }
  .n.ok {
    color: var(--ok);
  }
  .n.err {
    color: var(--err);
  }
  .list {
    flex: 1;
    min-height: 0;
    overflow: auto;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--surface);
  }
  .iter {
    position: sticky;
    top: 0;
    z-index: 1;
    background: var(--surface-2);
    color: var(--muted);
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    padding: 4px 10px;
    border-bottom: 1px solid var(--border);
  }
  .item {
    border-bottom: 1px solid var(--border);
  }
  .item.failed {
    box-shadow: inset 2px 0 0 var(--err);
  }
  .row {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 10px;
    border: none;
    border-radius: 0;
    background: none;
    padding: 5px 10px;
    text-align: left;
    font-size: 13px;
  }
  .row:hover {
    background: var(--hover);
  }
  .chev {
    color: var(--muted);
    font-size: 10px;
    width: 10px;
    flex: none;
  }
  .method {
    font-weight: 700;
    font-size: 11px;
    width: 38px;
    flex: none;
  }
  .name {
    flex: 0 1 auto;
    min-width: 60px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 500;
  }
  .url {
    flex: 1;
    min-width: 0;
    color: var(--muted);
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .status {
    color: var(--err);
    font-weight: 700;
    flex: none;
  }
  .status.ok {
    color: var(--ok);
  }
  .meta {
    color: var(--muted);
    font-size: 12px;
    flex: none;
    min-width: 56px;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .tests {
    flex: none;
    min-width: 40px;
    text-align: right;
    font-size: 12px;
    color: var(--muted);
    font-weight: 600;
  }
  .tests.ok {
    color: var(--ok);
  }
  .tests.err {
    color: var(--err);
  }
  .detail {
    padding: 4px 10px 10px 40px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .muted {
    color: var(--muted);
    font-size: 12px;
    margin: 0;
  }
  .test-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
    font-size: 12px;
  }
  .test-list li {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: baseline;
  }
  .test-list .err-text {
    flex-basis: 100%;
    margin-left: 80px;
  }
  .badge {
    font-size: 10px;
    font-weight: 700;
    text-transform: uppercase;
    padding: 1px 6px;
    border-radius: 4px;
    min-width: 72px;
    text-align: center;
  }
  .pass .badge {
    color: var(--ok);
    background: color-mix(in srgb, var(--ok) 14%, transparent);
  }
  .fail .badge {
    color: var(--err);
    background: color-mix(in srgb, var(--err) 14%, transparent);
  }
  .skip .badge {
    color: var(--muted);
    background: var(--surface-2);
  }
  .logs-title {
    font-size: 11px;
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
  .log {
    margin: 0;
    font-size: 12px;
    white-space: pre-wrap;
    word-break: break-word;
  }
  .more {
    display: block;
    margin: 8px auto;
    font-size: 12px;
  }
</style>
