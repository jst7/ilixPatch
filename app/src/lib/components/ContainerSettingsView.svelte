<script lang="ts">
  // Settings tab of a collection or folder (D39): Authorization and (collections) Variables.
  import { inheritedAuth } from "$lib/auth";
  import { t } from "$lib/i18n/index.svelte";
  import { containerSession } from "$lib/stores/containerSettings.svelte";
  import AuthEditor from "./auth/AuthEditor.svelte";
  import KeyValueTable from "./KeyValueTable.svelte";

  interface Props {
    path: string;
  }

  let { path }: Props = $props();

  const session = $derived(containerSession(path));
  const info = $derived(session.info);

  $effect(() => {
    session.ensureLoaded();
  });

  let tab = $state<"auth" | "variables">("auth");

  // What a folder inherits when it is set to "inherit".
  const inherited = $derived(info && info.kind === "folder" ? inheritedAuth(info.collection, info.ancestors) : null);
  const varCount = $derived(session.variables.filter((r) => r.key.trim()).length);
</script>

<section class="container">
  {#if !info}
    <p class="empty">{t("container.notFound")}</p>
  {:else}
    <header class="head">
      <div class="title">
        <span class="kind">{info.kind === "collection" ? t("container.title.collection") : t("container.title.folder")}</span>
        <span class="crumbs">
          {info.projectName} / {info.collection.name}{#each info.ancestors as f (f.path)}&nbsp;/ {f.name}{/each}{#if info.folder}&nbsp;/ {info.folder.name}{/if}
        </span>
        {#if session.dirty}<span class="dirty-dot" title={t("request.unsaved")}></span>{/if}
      </div>
      <div class="actions">
        {#if session.dirty}
          <button class="ghost small" onclick={() => session.revert()}>{t("request.revert")}</button>
        {/if}
        <button class="small primary" disabled={!session.dirty || session.saving} title={t("request.saveHint")} onclick={() => session.save()}>
          {session.saving ? t("request.saving") : t("request.save")}
        </button>
      </div>
    </header>

    {#if session.error}<div class="error" role="alert">{session.error}</div>{/if}

    <div class="tabs">
      <button class:active={tab === "auth"} onclick={() => (tab = "auth")}>
        {t("container.tab.auth")}{#if session.authDirty}<span class="count">•</span>{/if}
      </button>
      {#if info.kind === "collection"}
        <button class:active={tab === "variables"} onclick={() => (tab = "variables")}>
          {t("container.tab.variables")}{#if varCount}<span class="count">{varCount}</span>{/if}
        </button>
      {/if}
    </div>

    <div class="content">
      {#if tab === "auth" || info.kind !== "collection"}
        <p class="hint">{info.kind === "collection" ? t("container.authHint.collection") : t("container.authHint.folder")}</p>
        <AuthEditor
          bind:auth={session.auth}
          scope={info.kind}
          {inherited}
          collection={info.collection}
          projectPath={info.projectPath}
        />
      {:else}
        <p class="hint">{t("container.varsHint")}</p>
        <div class="scroll"><KeyValueTable bind:rows={session.variables} /></div>
      {/if}
    </div>
  {/if}
</section>

<style>
  .container {
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
    align-items: center;
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
  .small {
    padding: 4px 10px;
    font-size: 12px;
  }
  .error {
    color: var(--err);
    border: 1px solid var(--err);
    border-radius: 6px;
    padding: 6px 10px;
    font-size: 13px;
    word-break: break-word;
  }
  .content {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .hint {
    color: var(--muted);
    font-size: 12px;
    margin: 8px 0 0;
  }
  .scroll {
    flex: 1;
    overflow: auto;
    margin-top: 6px;
  }
  .empty {
    margin: auto;
    color: var(--muted);
  }
</style>
