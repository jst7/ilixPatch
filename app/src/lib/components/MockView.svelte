<script lang="ts">
  // D62: mock server of a project or collection: start/stop, routes and the requests it served.
  import { onMount } from "svelte";
  import { t } from "$lib/i18n/index.svelte";
  import { ws } from "$lib/stores/workspace.svelte";
  import { mocks } from "$lib/stores/mock.svelte";
  import { methodColor } from "./methods";

  interface Props {
    target: string;
  }
  let { target }: Props = $props();

  onMount(() => void mocks.init());

  const status = $derived(mocks.running[target] ?? null);
  // Created here, at init: writing state inside $derived is not allowed (it crashed the view).
  // The view is keyed by target in +page.svelte, so the initial value is the only one.
  // svelte-ignore state_referenced_locally
  mocks.ensure(target);
  const config = $derived(mocks.config[target]);
  const log = $derived(mocks.logs[target] ?? []);
  const name = $derived(ws.nodes.get(target)?.name ?? target.split(/[\\/]/).filter(Boolean).pop() ?? target);
  const time = (ms: number) => new Date(ms).toLocaleTimeString();
</script>

<section class="mock">
  <header>
    <h2>{t("mock.title", { name })}</h2>
    <p class="hint">{t("mock.intro")}</p>
  </header>

  <div class="controls">
    <label>{t("mock.port")}<input type="number" min="0" max="65535" bind:value={config.port} disabled={!!status} /></label>
    <label title={t("mock.delayHint")}>{t("mock.delay")}<input type="number" min="0" step="100" bind:value={config.delay_ms} disabled={!!status} /></label>
    {#if status}
      <button onclick={() => mocks.stop(target)}>{t("mock.stop")}</button>
      <span class="url">
        <span class="dot"></span>
        <code>{status.url}</code>
        <button class="ghost small" onclick={() => navigator.clipboard.writeText(status.url).catch(() => {})}>{t("mock.copy")}</button>
      </span>
    {:else}
      <button class="primary" disabled={mocks.busy[target]} onclick={() => mocks.start(target)}>{t("mock.start")}</button>
    {/if}
  </div>
  {#if mocks.error[target]}<p class="err" role="alert">{mocks.error[target]}</p>{/if}

  {#if status}
    <div class="panes">
      <div class="pane">
        <h3>{t("mock.routes", { count: status.routes.length })}</h3>
        {#if status.routes.length === 0}
          <p class="hint">{t("mock.noRoutes")}</p>
        {:else}
          <table>
            <tbody>
              {#each status.routes as r (r.method + r.pattern + r.name)}
                <tr>
                  <td class="method" style:color={methodColor(r.method)}>{r.method}</td>
                  <td class="mono">{r.pattern}</td>
                  <td class="muted" title={r.examples.join("\n")}>{r.name} · {r.examples.length}</td>
                </tr>
              {/each}
            </tbody>
          </table>
          <p class="hint">{t("mock.pickHint")}</p>
        {/if}
      </div>
      <div class="pane">
        <h3>
          {t("mock.log", { count: log.length })}
          {#if log.length}<button class="ghost small" onclick={() => mocks.clearLog(target)}>{t("mock.clear")}</button>{/if}
        </h3>
        {#if log.length === 0}
          <p class="hint">{t("mock.waiting", { url: status.url })}</p>
        {:else}
          <table>
            <tbody>
              {#each log as e, i (i)}
                <tr>
                  <td class="muted">{time(e.time)}</td>
                  <td class="method" style:color={methodColor(e.method)}>{e.method}</td>
                  <td class="mono">{e.path}</td>
                  <td class="status" class:ok={e.status < 400}>{e.status}</td>
                  <td class="muted">{e.route ? `${e.route} › ${e.example}` : t("mock.noMatch")}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
      </div>
    </div>
  {/if}
</section>

<style>
  .mock {
    display: flex;
    flex-direction: column;
    gap: 10px;
    height: 100%;
    min-height: 0;
    padding: 12px;
    overflow: auto;
  }
  h2 {
    margin: 0;
    font-size: var(--fs-lg);
  }
  h3 {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 0 6px;
    font-size: var(--fs-sm);
  }
  .hint {
    margin: 4px 0 0;
    color: var(--muted);
    font-size: var(--fs-sm);
  }
  .controls {
    display: flex;
    align-items: flex-end;
    gap: 10px;
    flex-wrap: wrap;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: var(--fs-xs);
    color: var(--muted);
  }
  label input {
    width: 110px;
  }
  .url {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--ok);
  }
  .err {
    color: var(--err);
    margin: 0;
  }
  .panes {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1.4fr);
    gap: 12px;
    min-height: 0;
  }
  .pane {
    min-width: 0;
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 8px 10px;
    overflow: auto;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--fs-sm);
  }
  td {
    padding: 2px 6px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 320px;
  }
  .method {
    font-family: var(--mono);
    font-weight: 600;
    font-size: var(--fs-xs);
  }
  .mono {
    font-family: var(--mono);
  }
  .muted {
    color: var(--muted);
  }
  .status {
    font-family: var(--mono);
    color: var(--err);
  }
  .status.ok {
    color: var(--ok);
  }
</style>
