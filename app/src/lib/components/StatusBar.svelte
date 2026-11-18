<script lang="ts">
  // D65: status bar — what is worth always seeing, in one line: environment, git branch, running mock servers,
  // a security warning and the network console. Each item opens where it is managed.
  import { t } from "$lib/i18n/index.svelte";
  import { ws } from "$lib/stores/workspace.svelte";
  import { mocks } from "$lib/stores/mock.svelte";
  import { vault } from "$lib/stores/vault.svelte";
  import { netlog } from "$lib/stores/netlog.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { envColor } from "$lib/env/style";

  /** Collection of the active request (or of the request's history entry), for the environment and the branch. */
  const collection = $derived(ws.selected?.collection ?? null);
  const project = $derived(collection ? ws.projectOf(collection) : null);
  const env = $derived(collection ? ws.resolveEnv(collection) : null);
  const envDoc = $derived(env && project ? project.environments.find((e) => e.name === env.name) : null);
  const repo = $derived(project ? (ws.repoByRoot.get(project.path) ?? null) : null);
  const running = $derived(Object.values(mocks.running));

  const mod = typeof navigator !== "undefined" && /Mac/.test(navigator.platform) ? "⌘" : "Ctrl+";

  function openMock() {
    const first = running[0];
    if (first) ws.openTab({ kind: "mock", target: first.target });
  }
</script>

<footer class="status" aria-label={t("status.label")}>
  {#if project}
    <button class="item" title={t("status.envHint")} onclick={() => ws.openTab({ kind: "environments", project: project.path })}>
      <span class="dot" style:background={envColor(envDoc?.color) ?? (env ? "var(--accent)" : "var(--muted)")}></span>
      {env?.name ? t("status.env", { name: env.name }) : t("status.noEnv")}
    </button>
  {/if}
  {#if repo}
    <button class="item" title={t("status.gitHint")} onclick={() => ws.openTab({ kind: "git", repo: repo.root })}>
      <svg viewBox="0 0 16 16" width="12" height="12" fill="none" stroke="currentColor" stroke-width="1.5" aria-hidden="true"><circle cx="4" cy="3.5" r="1.5" /><circle cx="4" cy="12.5" r="1.5" /><circle cx="12" cy="6" r="1.5" /><path d="M4 5v6M12 7.5c0 2.5-3 3-8 3.5" /></svg>
      {repo.branch ?? repo.head ?? "?"}
      {#if repo.dirty}<span class="badge warn" title={t("status.gitDirty")}>●</span>{/if}
      {#if repo.ahead}<span class="badge">↑{repo.ahead}</span>{/if}
      {#if repo.behind}<span class="badge">↓{repo.behind}</span>{/if}
    </button>
  {/if}
  {#if running.length}
    <button class="item live" title={running.map((r) => r.url).join("\n")} onclick={openMock}>
      <span class="dot pulse"></span>
      {t("status.mocks", { count: running.length })}
    </button>
  {/if}
  <span class="spacer"></span>
  <button class="item" title={t("palette.title")} onclick={() => (ui.paletteOpen = true)}>
    <svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><circle cx="11" cy="11" r="7" /><path d="M20 20l-3.5-3.5" /></svg>
    {t("status.palette")} <kbd>{mod}K</kbd>
  </button>
  {#if vault.defaultPassword}
    <button class="item warn" title={t("vault.defaultNotice")} onclick={() => ui.openSettings("security")}>
      <svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="M6 11V8a6 6 0 0112 0v3M5 11h14v10H5z" /></svg>
      {t("status.defaultPassword")}
    </button>
  {/if}
  <button class="item" class:on={netlog.open} title={t("netlog.toggle")} aria-pressed={netlog.open} onclick={() => netlog.toggle()}>
    <svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><rect x="3" y="4" width="18" height="16" rx="2" /><path d="M7 9l3 3-3 3M13 15h4" /></svg>
    {t("status.netlog")}{#if netlog.entries.length}<span class="badge">{netlog.entries.length}</span>{/if}
  </button>
</footer>

<style>
  .status {
    flex: none;
    display: flex;
    align-items: center;
    gap: 2px;
    height: 24px;
    padding: 0 6px;
    border-top: 1px solid var(--border);
    background: var(--surface);
    font-size: var(--fs-xs);
    color: var(--muted);
    overflow: hidden;
    white-space: nowrap;
  }
  .item {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 20px;
    padding: 0 6px;
    border: none;
    border-radius: 4px;
    background: none;
    color: inherit;
    font-size: inherit;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .item:hover {
    background: var(--hover);
    color: var(--text);
  }
  .item.on {
    color: var(--accent);
  }
  .item.warn {
    color: var(--warn);
  }
  .item.live {
    color: var(--ok);
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    flex: none;
  }
  .dot.pulse {
    background: var(--ok);
    animation: pulse 1.6s ease-in-out infinite;
  }
  @keyframes pulse {
    50% {
      opacity: 0.35;
    }
  }
  .badge {
    font-size: var(--fs-xs);
    color: var(--muted);
  }
  .badge.warn {
    color: var(--warn);
  }
  .spacer {
    flex: 1;
  }
  kbd {
    font-family: var(--mono);
    font-size: var(--fs-xs);
    border: 1px solid var(--border);
    border-radius: 3px;
    padding: 0 4px;
  }
</style>
