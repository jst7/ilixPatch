<script lang="ts">
  import type { Collection } from "$lib/api";
  import { t } from "$lib/i18n/index.svelte";
  import { runtimeVars, type Overrides } from "$lib/stores/runtimeVars.svelte";
  import { ws } from "$lib/stores/workspace.svelte";

  interface Props {
    collection: Collection;
    /** Ignored: the environment is resolved from the workspace selection (kept for compatibility). */
    envName?: string | null;
  }

  let { collection }: Props = $props();

  const env = $derived(ws.resolveEnv(collection));
  const envName = $derived(env?.name ?? null);
  const envId = $derived(env?.id ?? null);

  let open = $state(false);
  let root: HTMLDivElement | undefined = $state();

  const count = $derived(runtimeVars.count(collection.path, envId));

  type Scope = "global" | "collection" | "environment";
  const groups = $derived.by(() => {
    const out: { scope: Scope; title: string; values: Overrides }[] = [
      { scope: "global", title: t("vars.scope.global"), values: runtimeVars.globals },
      {
        scope: "collection",
        title: t("vars.scope.collection", { name: collection.name }),
        values: runtimeVars.collectionOverrides(collection.path),
      },
    ];
    if (envName) {
      out.push({
        scope: "environment",
        title: t("vars.scope.environment", { name: envName }),
        values: runtimeVars.environmentOverrides(envId),
      });
    }
    return out.filter((g) => Object.keys(g.values).length > 0);
  });

  function resetGroup(scope: Scope) {
    if (scope === "global") runtimeVars.resetGlobals();
    else if (scope === "collection") runtimeVars.resetCollection(collection.path);
    else if (envId) runtimeVars.resetEnvironment(envId);
  }

  function resetAllVisible() {
    runtimeVars.resetGlobals();
    runtimeVars.resetCollection(collection.path);
    if (envId) runtimeVars.resetEnvironment(envId);
  }

  function onWindowPointer(e: PointerEvent) {
    if (open && root && !root.contains(e.target as Node)) open = false;
  }

  function onKey(e: KeyboardEvent) {
    if (open && e.key === "Escape") open = false;
  }
</script>

<svelte:window onpointerdown={onWindowPointer} onkeydown={onKey} />

<div class="wrap" bind:this={root}>
  <button class="ghost small" class:has={count > 0} aria-expanded={open} onclick={() => (open = !open)}>
    {t("vars.button")}{#if count > 0}<span class="badge">{count}</span>{/if}
  </button>
  {#if open}
    <div class="pop" role="dialog" aria-label={t("vars.title")}>
      <div class="pop-head">
        <strong>{t("vars.title")}</strong>
        {#if count > 0}
          <button class="ghost small" onclick={resetAllVisible}>{t("vars.resetAll")}</button>
        {/if}
        <button class="ghost small close" aria-label={t("vars.close")} title={t("vars.close")} onclick={() => (open = false)}
          >×</button
        >
      </div>
      <p class="hint">{t("vars.hint")}</p>
      {#if groups.length === 0}
        <p class="hint">{t("vars.empty")}</p>
      {/if}
      {#each groups as g (g.scope)}
        <div class="group">
          <div class="group-head">
            <span>{g.title}</span>
            <button class="ghost small" onclick={() => resetGroup(g.scope)}>{t("vars.reset")}</button>
          </div>
          <table>
            <tbody>
              {#each Object.entries(g.values) as [k, v] (k)}
                <tr>
                  <td class="mono key">{k}</td>
                  <td class="mono val" class:unset={v === null}>{v === null ? t("vars.unset") : v}</td>
                  <td class="act">
                    <button
                      class="ghost x"
                      title={t("vars.resetKey")}
                      aria-label={t("vars.resetKey")}
                      onclick={() => runtimeVars.resetKey(g.scope, k, collection.path, envId)}>×</button
                    >
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .wrap {
    position: relative;
  }
  .small {
    padding: 3px 8px;
    font-size: 12px;
  }
  .has {
    color: var(--text);
  }
  .badge {
    margin-left: 5px;
    padding: 0 5px;
    border-radius: 999px;
    background: var(--accent);
    color: var(--accent-text);
    font-size: 11px;
  }
  .pop {
    position: absolute;
    right: 0;
    top: calc(100% + 4px);
    width: min(420px, 80vw);
    max-height: 60vh;
    overflow: auto;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: var(--shadow);
    padding: 10px 12px;
    z-index: 40;
    font-size: 13px;
  }
  .pop-head {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .pop-head strong {
    flex: 1;
  }
  .close {
    font-size: 16px;
    line-height: 1;
  }
  .hint {
    color: var(--muted);
    font-size: 12px;
    margin: 6px 0;
  }
  .group {
    margin-top: 8px;
  }
  .group-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    font-weight: 600;
    font-size: 12px;
    color: var(--muted);
    border-bottom: 1px solid var(--border);
    padding-bottom: 2px;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    table-layout: fixed;
  }
  td {
    padding: 3px 4px;
    border-bottom: 1px solid var(--border);
    word-break: break-all;
    vertical-align: top;
    font-size: 12px;
  }
  .key {
    width: 38%;
    font-weight: 600;
  }
  .unset {
    color: var(--muted);
    font-style: italic;
  }
  .act {
    width: 28px;
  }
  .x {
    padding: 0 6px;
  }
</style>
