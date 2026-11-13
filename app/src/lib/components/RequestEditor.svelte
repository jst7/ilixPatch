<script lang="ts">
  import type { Snippet } from "svelte";
  import { api, type BlameInfo, type Collection, type RequestBlame } from "$lib/api";
  import { t, type MessageKey } from "$lib/i18n/index.svelte";
  import { ws, isWithin, draftFromRequest, requestFromDraft, type BodyMode, type RequestLocation } from "$lib/stores/workspace.svelte";
  import { looksLikeCurl, parseCurl, resolveRequest, toCurl } from "$lib/curl";
  import { git } from "$lib/stores/git.svelte";
  import { blameLabel, blameTooltip, isUncommitted } from "$lib/editor/blame";
  import CodeEditor from "./CodeEditor.svelte";
  import KeyValueTable from "./KeyValueTable.svelte";
  import ScriptsPanel from "./ScriptsPanel.svelte";
  import RequestSettingsPanel from "./RequestSettingsPanel.svelte";
  import AuthEditor from "./auth/AuthEditor.svelte";
  import { cleanAuth } from "$lib/auth";
  import { unexportedAuth } from "$lib/curl";
  import MethodSelect from "./MethodSelect.svelte";
  import EnvSelect from "./EnvSelect.svelte";
  import { envStore } from "$lib/stores/environments.svelte";
  import UrlInput from "./UrlInput.svelte";
  import ExtractPanel from "./ExtractPanel.svelte";

  interface Props {
    /** The request file being edited. Optional: history tabs edit an in-memory draft instead. */
    loc?: RequestLocation | null;
    /** Key of the draft in ws.drafts / ws.responses when there is no file (e.g. "history:12"). */
    draftKey?: string;
    /** Collection whose environments apply when `loc` is null (none → no environment picker). */
    collection?: Collection | null;
    /** Replaces the breadcrumb + save header (used by history tabs). */
    header?: Snippet;
    /** Replaces the default send action (ws.send(path)). */
    onsend?: () => void;
  }

  let { loc = null, draftKey, collection: collectionProp = null, header, onsend }: Props = $props();

  const path = $derived(draftKey ?? loc?.file.path ?? "");
  const draft = $derived(ws.drafts[path]);
  const dirty = $derived(ws.isDirty(path));
  const sending = $derived(ws.responses[path]?.loading ?? false);
  const collection = $derived(loc?.collection ?? collectionProp);

  function send() {
    if (onsend) onsend();
    else ws.send(path);
  }

  let tab = $state<"auth" | "params" | "headers" | "body" | "scripts" | "extract" | "settings">("params");

  /** What this request gets from its folders/collection when set to "inherit" (D39). */
  const inheritedAuth = $derived(draft?.auth?.type === "inherit" ? ws.effectiveAuthFor(path) : null);

  // ----- cURL import (paste/Enter in the URL bar) and export (D38) -----
  let curlNotice = $state<{ kind: "ok" | "error"; text: string; details: string[] } | null>(null);
  let curlNoticeTimer: ReturnType<typeof setTimeout> | undefined;
  function showCurlNotice(n: typeof curlNotice, ms = 6000) {
    curlNotice = n;
    clearTimeout(curlNoticeTimer);
    if (n && n.kind === "ok" && !n.details.length) curlNoticeTimer = setTimeout(() => (curlNotice = null), ms);
  }

  /** Replace the request (method, URL, params, headers, body, settings) with a pasted curl command. */
  function applyCurl(text: string) {
    try {
      const { request, warnings } = parseCurl(text);
      // Form bodies aren't editable in the UI: import them as an editable text body.
      if (request.body?.type === "form") {
        const fields = request.body.fields;
        request.body = {
          type: "text",
          content: Object.entries(fields)
            .map(([k, v]) => `${encodeURIComponent(k)}=${encodeURIComponent(v)}`)
            .join("&"),
        };
        const headers = (request.headers ??= {});
        if (!Object.keys(headers).some((k) => k.toLowerCase() === "content-type"))
          headers["Content-Type"] = "application/x-www-form-urlencoded";
      }
      const next = draftFromRequest(request);
      draft.method = next.method;
      draft.url = next.url;
      draft.query = next.query;
      draft.headers = next.headers;
      draft.bodyMode = next.bodyMode;
      draft.bodyContent = next.bodyContent;
      draft.formFields = next.formFields;
      draft.settings = next.settings;
      draft.auth = next.auth;
      if (next.bodyMode !== "none") tab = "body";
      else if (Object.keys(request.headers ?? {}).length) tab = "headers";
      showCurlNotice({
        kind: "ok",
        text: warnings.length ? t("request.curlImportedNotes", { n: warnings.length }) : t("request.curlImported"),
        details: warnings,
      });
    } catch (e) {
      showCurlNotice({ kind: "error", text: t("request.curlError", { msg: (e as Error).message }), details: [] });
    }
  }

  async function copyCurl(mode: "resolved" | "raw" | "oneline") {
    let req = requestFromDraft(draft);
    req.auth = cleanAuth(ws.effectiveAuthFor(path).auth);
    if (mode !== "raw" && (collection || !loc)) req = resolveRequest(req, ws.variablesFor(collection));
    const skipped = unexportedAuth(req.auth);
    try {
      await navigator.clipboard.writeText(toCurl(req, { multiline: mode !== "oneline" }));
      showCurlNotice(
        skipped
          ? {
              kind: "ok",
              text: t("request.curlCopied"),
              details: [t("request.curlAuthNotExported", { type: t(`auth.type.${skipped}` as MessageKey) })],
            }
          : { kind: "ok", text: t("request.curlCopied"), details: [] },
        2500,
      );
    } catch (e) {
      ws.fail(e);
    }
  }

  function curlMenu(e: MouseEvent) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    ws.menu = {
      x: r.left,
      y: r.bottom + 2,
      items: [
        { label: t("request.curlCopy"), action: () => copyCurl("resolved") },
        { label: t("request.curlCopyRaw"), action: () => copyCurl("raw") },
        { label: t("request.curlCopyOneLine"), action: () => copyCurl("oneline") },
      ],
    };
  }

  const filled = (rows: { key: string }[]) => rows.filter((r) => r.key.trim()).length;

  function formatJson() {
    try {
      draft.bodyContent = JSON.stringify(JSON.parse(draft.bodyContent), null, 2);
    } catch {
      ws.fail(t("request.invalidJson"));
    }
  }

  // ----- Git annotate (D25) -----
  const LS_ANNOTATE = "ilixpatch.annotate";
  function lsAnnotate(): boolean {
    try {
      return localStorage.getItem(LS_ANNOTATE) === "1";
    } catch {
      return false;
    }
  }
  let annotate = $state(lsAnnotate());
  function toggleAnnotate() {
    annotate = !annotate;
    try {
      localStorage.setItem(LS_ANNOTATE, annotate ? "1" : "0");
    } catch {}
  }

  /** Only real request files (not history drafts) can be annotated. */
  const filePath = $derived(!draftKey && loc ? loc.file.path : null);
  /** Nearest git repo containing the file. */
  const repo = $derived.by(() => {
    if (!filePath) return null;
    let best: string | null = null;
    for (const root of ws.repoByRoot.keys())
      if (isWithin(filePath, root) && (!best || root.length > best.length)) best = root;
    return best;
  });
  let blame = $state.raw<RequestBlame | null>(null);
  let blameFailed = $state(false);
  const annotateOn = $derived(annotate && !!repo && !blameFailed);
  const annotating = $derived(annotateOn && !!blame);
  let blameSeq = 0;

  $effect(() => {
    const p = filePath;
    const r = repo;
    const on = annotate;
    // Refetch after saves / commits / window focus (each bumps the repo's status version).
    void (r ? git.statusVersion[r] : 0);
    const seq = ++blameSeq;
    if (!on || !p || !r) {
      blame = null;
      blameFailed = false;
      return;
    }
    api.gitBlameRequest(p).then(
      (b) => {
        if (seq !== blameSeq) return;
        blame = b;
        blameFailed = false;
      },
      () => {
        if (seq !== blameSeq) return;
        blame = null;
        blameFailed = true;
      },
    );
  });

  function openBlame(info: BlameInfo) {
    if (!repo) return;
    if (info.commit) git.showCommit(repo, info.commit);
    else {
      git.showHistory(repo, filePath);
      ws.openTab({ kind: "git", repo });
    }
  }

  const bodyModes: { mode: BodyMode; label: () => string }[] = [
    { mode: "none", label: () => t("request.body.none") },
    { mode: "json", label: () => "JSON" },
    { mode: "text", label: () => t("request.body.text") },
  ];

</script>

{#snippet envPicker()}
  {#if collection}
    {@const envProject = ws.projectOf(collection)?.path ?? null}
    <EnvSelect
      projectPath={envProject}
      {collection}
      value={ws.resolveEnv(collection)?.choice ?? ws.envChoice(envProject)}
      onchange={(c) => envProject && ws.setEnvChoice(envProject, c)}
    />
  {:else if !loc}
    <!-- D55: requests outside any project (unsaved tabs, history of deleted requests): global environments only. -->
    <EnvSelect projectPath={null} value={envStore.noProjectChoice} onchange={(c) => envStore.setNoProjectChoice(c)} />
  {/if}
{/snippet}

{#if draft}
  <section class="editor">
    {#if header}
      {@render header()}
    {:else if loc}
    <div class="head">
      <nav class="crumbs" aria-label={t("request.breadcrumb")}>
        <span>{loc.projectName}</span>
        <span class="sep">/</span>
        <span>{loc.collection.name}</span>
        {#each loc.folders as f (f.path)}
          <span class="sep">/</span>
          <span>{f.name}</span>
        {/each}
        <span class="sep">/</span>
        <input
          class="name"
          bind:value={draft.name}
          placeholder={t("request.namePlaceholder")}
          aria-label={t("request.namePlaceholder")}
          spellcheck="false"
        />
        {#if dirty}<span class="dirty-dot" title={t("request.unsaved")}></span>{/if}
      </nav>
      <!-- The environment applies to the request: next to its name, not in the URL bar. -->
      <div class="head-env">{@render envPicker()}</div>
      <div class="head-actions">
        {#if dirty}
          <button class="ghost small" onclick={() => ws.revert(path)}>{t("request.revert")}</button>
        {/if}
        <button class="small" disabled={!dirty || ws.saving} title={t("request.saveHint")} onclick={() => ws.save(path)}>
          {ws.saving ? t("request.saving") : t("request.save")}
        </button>
      </div>
    </div>
    {/if}

    <div class="bar">
      <MethodSelect bind:value={draft.method} label={t("request.method")} />
      <!-- Multi-line URL: wraps long URLs and grows up to ~6 lines. A URL has no newlines,
           so Enter sends and pasted line breaks are removed. -->
      <UrlInput
        bind:value={draft.url}
        {collection}
        placeholder={t("request.urlPlaceholder")}
        ariaLabel={t("request.url")}
        onpaste={(e) => {
          // Pasting a curl command fills in the whole request, like Postman.
          const text = e.clipboardData?.getData("text/plain") ?? "";
          if (looksLikeCurl(text)) {
            e.preventDefault();
            applyCurl(text);
          }
        }}
        onkeydown={(e) => {
          if (e.key === "Enter" && !e.isComposing && !(e.metaKey || e.ctrlKey)) {
            e.preventDefault();
            if (looksLikeCurl(draft.url)) applyCurl(draft.url);
            else if (!sending) send();
          }
        }}
        oninput={(e) => {
          const el = e.currentTarget;
          if (/[\r\n]/.test(el.value) && !looksLikeCurl(el.value)) {
            const pos = el.selectionStart;
            const before = el.value.slice(0, pos).replace(/\s*[\r\n]+\s*/g, "").length;
            draft.url = el.value.replace(/\s*[\r\n]+\s*/g, "");
            queueMicrotask(() => el.setSelectionRange(before, before));
          }
        }}
      />
      {#if annotating && blame}
        {@const info = blame.url}
        {#if info && !isUncommitted(info)}
          <button class="url-blame" title={blameTooltip(info)} onclick={() => openBlame(info)}>{blameLabel(info)}</button>
        {:else}
          <span class="url-blame uncommitted" title={blameTooltip(null)}>—</span>
        {/if}
      {/if}
      {#if header}{@render envPicker()}{/if}
      <button
        class="send"
        class:primary={!sending}
        class:cancel={sending}
        title={sending ? t("request.cancelHint") : t("request.sendHint")}
        onclick={() => (sending ? ws.cancelSend(path) : send())}
      >
        {sending ? t("request.cancel") : t("request.send")}
      </button>
    </div>

    <div class="tabs">
      <button class:active={tab === "auth"} onclick={() => (tab = "auth")}>
        {t("request.tab.auth")}{#if draft.auth.type !== "inherit" && draft.auth.type !== "no_auth"}<span class="count">•</span>{/if}
      </button>
      <button class:active={tab === "params"} onclick={() => (tab = "params")}>
        {t("request.tab.params")}{#if filled(draft.query)}<span class="count">{filled(draft.query)}</span>{/if}
      </button>
      <button class:active={tab === "headers"} onclick={() => (tab = "headers")}>
        {t("request.tab.headers")}{#if filled(draft.headers)}<span class="count">{filled(draft.headers)}</span>{/if}
      </button>
      <button class:active={tab === "body"} onclick={() => (tab = "body")}>
        {t("request.tab.body")}{#if draft.bodyMode !== "none"}<span class="count">•</span>{/if}
      </button>
      <button class:active={tab === "scripts"} onclick={() => (tab = "scripts")}>
        {t("request.tab.scripts")}{#if draft.preRequest.trim() || draft.tests.trim()}<span
            class="script-dot"
            title={t("request.scripts.hasScript")}
          ></span>{/if}
      </button>
      <button class:active={tab === "extract"} onclick={() => (tab = "extract")} title={t("extract.tabHint")}>
        {t("request.tab.extract")}{#if draft.extract.some((e) => e.enabled && e.var.trim())}<span class="count"
            >{draft.extract.filter((e) => e.enabled && e.var.trim()).length}</span
          >{/if}
      </button>
      <button class:active={tab === "settings"} onclick={() => (tab = "settings")}>
        {t("request.tab.settings")}{#if draft.settings.verify_ssl === false}<span
            class="ssl-off"
            title={t("reqset.sslOffBadge")}
            aria-label={t("reqset.sslOffBadge")}>!</span
          >{:else if Object.values(draft.settings).some((v) => v !== null)}<span class="count">•</span>{/if}
      </button>
      {#if filePath}
        <button
          class="annotate"
          class:on={annotateOn}
          aria-pressed={annotateOn}
          disabled={!repo || blameFailed}
          title={!repo || blameFailed ? t("blame.notInRepo") : t("blame.annotateHint")}
          onclick={toggleAnnotate}
        >
          <svg viewBox="0 0 16 16" width="13" height="13" aria-hidden="true"
            ><path
              d="M2 3h4M2 8h4M2 13h4M9 3h5M9 8h5M9 13h5"
              fill="none"
              stroke="currentColor"
              stroke-width="1.6"
              stroke-linecap="round"
            /></svg
          >
          {t("blame.annotate")}
        </button>
      {/if}
      <button
        class="curl-btn"
        class:push={!filePath}
        title={t("request.curlHint")}
        aria-haspopup="menu"
        onclick={curlMenu}
      >
        <svg viewBox="0 0 16 16" width="13" height="13" aria-hidden="true"
          ><path
            d="M5 4 1.5 8 5 12M11 4l3.5 4L11 12M9.5 2.5l-3 11"
            fill="none"
            stroke="currentColor"
            stroke-width="1.5"
            stroke-linecap="round"
            stroke-linejoin="round"
          /></svg
        >
        cURL
      </button>
    </div>
    {#if curlNotice}
      <div class="curl-notice" class:error={curlNotice.kind === "error"} role="status">
        <span>{curlNotice.text}</span>
        {#if curlNotice.details.length}
          <ul>{#each curlNotice.details as d}<li>{d}</li>{/each}</ul>
        {/if}
        <button class="close" aria-label={t("settings.close")} onclick={() => (curlNotice = null)}>✕</button>
      </div>
    {/if}

    <div class="content">
      {#if tab === "auth"}
        <AuthEditor
          bind:auth={draft.auth}
          scope="request"
          inherited={inheritedAuth}
          collection={collection ?? null}
          projectPath={loc?.projectPath ?? null}
        />
      {:else if tab === "params"}
        <div class="scroll">
          <KeyValueTable
            bind:rows={draft.query}
            annotations={annotating ? blame?.query : null}
            onannotationclick={openBlame}
          />
        </div>
      {:else if tab === "headers"}
        <div class="scroll">
          <KeyValueTable
            bind:rows={draft.headers}
            annotations={annotating ? blame?.headers : null}
            onannotationclick={openBlame}
          />
        </div>
      {:else if tab === "settings"}
        <RequestSettingsPanel {draft} />
      {:else if tab === "extract"}
        <ExtractPanel {draft} {path} />
      {:else if tab === "scripts"}
        <ScriptsPanel
          {draft}
          {path}
          preAnnotations={annotating ? blame?.pre_request_lines : null}
          testsAnnotations={annotating ? blame?.tests_lines : null}
          onannotationclick={openBlame}
        />
      {:else}
        <div class="body-bar">
          <select bind:value={draft.bodyMode} class="small" aria-label={t("request.bodyType")}>
            {#each bodyModes as b (b.mode)}<option value={b.mode}>{b.label()}</option>{/each}
            {#if draft.bodyMode === "form"}<option value="form">{t("request.body.form")}</option>{/if}
          </select>
          {#if draft.bodyMode === "json"}
            <button class="ghost small" onclick={formatJson}>{t("request.format")}</button>
          {/if}
        </div>
        {#if draft.bodyMode === "json" || draft.bodyMode === "text"}
          <div class="code">
            {#key path}
              <CodeEditor
                value={draft.bodyContent}
                onchange={(v) => (draft.bodyContent = v)}
                language={draft.bodyMode === "json" ? "json" : "text"}
                annotations={annotating ? blame?.body_lines : null}
                onannotationclick={openBlame}
              />
            {/key}
          </div>
        {:else if draft.bodyMode === "form"}
          <p class="hint">{t("request.formReadonly")}</p>
        {:else}
          <p class="hint">{t("request.noBody")}</p>
        {/if}
      {/if}
    </div>
  </section>
{/if}

<style>
  .editor {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    padding: 12px 12px 0;
    gap: 10px;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .crumbs {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--muted);
    font-size: 13px;
    white-space: nowrap;
    overflow: hidden;
  }
  .crumbs > span:not(.dirty-dot) {
    overflow: hidden;
    text-overflow: ellipsis;
    flex: 0 1 auto;
    min-width: 2em;
  }
  .sep {
    flex: none !important;
    min-width: 0 !important;
    opacity: 0.6;
  }
  .name {
    flex: 1 1 140px;
    min-width: 80px;
    color: var(--text);
    font-weight: 600;
    font-size: 14px;
    border-color: transparent;
    background: none;
    padding: 4px 6px;
  }
  .name:hover {
    border-color: var(--border);
  }
  .name:focus {
    border-color: var(--accent);
  }
  .head-env {
    flex: 0 1 auto;
    min-width: 0;
    display: flex;
  }
  .head-actions {
    display: flex;
    gap: 6px;
    flex: none;
  }
  .small {
    padding: 4px 10px;
    font-size: 12px;
  }
  .bar {
    display: flex;
    align-items: flex-start;
    gap: 8px;
  }
  .send {
    flex: none;
    min-width: 84px;
  }
  .content {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    padding-bottom: 8px;
  }
  .scroll {
    flex: 1;
    overflow: auto;
  }
  .body-bar {
    display: flex;
    gap: 8px;
    align-items: center;
    padding: 8px 0;
  }
  .code {
    flex: 1;
    min-height: 0;
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow: hidden;
  }
  .ssl-off {
    display: inline-block;
    margin-left: 6px;
    min-width: 14px;
    height: 14px;
    line-height: 14px;
    border-radius: 7px;
    text-align: center;
    font-size: 10px;
    font-weight: 700;
    color: var(--bg);
    background: var(--warn);
    vertical-align: middle;
  }
  .script-dot {
    display: inline-block;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--ok);
    margin-left: 6px;
    vertical-align: middle;
  }
  .hint {
    color: var(--muted);
    font-size: 13px;
  }
  .annotate {
    margin-left: auto;
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 12px;
  }
  .annotate.on {
    color: var(--accent);
  }
  .annotate:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .url-blame {
    flex: none;
    align-self: center;
    max-width: 130px;
    border: none;
    background: none;
    padding: 0 2px;
    font-size: 11px;
    color: var(--blame-text);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  button.url-blame:hover {
    color: var(--text);
    text-decoration: underline;
  }
  .url-blame.uncommitted {
    color: var(--muted);
  }
  .send.cancel {
    background: var(--surface-2);
    border-color: var(--err);
    color: var(--err);
  }
  .send.cancel:hover {
    background: var(--err);
    color: var(--accent-text);
  }
  .curl-btn {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 3px 9px;
    font-size: 12px;
    color: var(--muted);
    background: none;
    border: 1px solid var(--border);
    border-radius: 6px;
    cursor: pointer;
  }
  .curl-btn.push {
    margin-left: auto;
  }
  .curl-btn:hover {
    color: var(--text);
    border-color: var(--accent);
  }
  .curl-notice {
    position: relative;
    margin: 6px 0 0;
    padding: 6px 30px 6px 10px;
    border: 1px solid var(--border);
    border-left: 3px solid var(--ok);
    border-radius: 6px;
    background: var(--surface-2);
    font-size: 12px;
  }
  .curl-notice.error {
    border-left-color: var(--err);
    color: var(--err);
  }
  .curl-notice ul {
    margin: 4px 0 0;
    padding-left: 18px;
    color: var(--muted);
  }
  .curl-notice .close {
    position: absolute;
    top: 4px;
    right: 6px;
    border: none;
    background: none;
    color: var(--muted);
    cursor: pointer;
  }
</style>
