<script lang="ts">
  import type { SendResult } from "$lib/api";
  import { t } from "$lib/i18n/index.svelte";
  import { ws } from "$lib/stores/workspace.svelte";
  import CodeEditor from "./CodeEditor.svelte";
  import RuntimeVariables from "./RuntimeVariables.svelte";
  import NetworkInfo from "./NetworkInfo.svelte";
  import RequestErrorView from "./RequestErrorView.svelte";
  import LargeBodyViewer from "./largebody/LargeBodyViewer.svelte";
  import BinaryBodyView from "./largebody/BinaryBodyView.svelte";
  import MediaPreview from "./MediaPreview.svelte";
  import { api } from "$lib/api";
  import { isGenericMime, previewFromMime, sniffPreview, type PreviewType } from "$lib/preview/kind";
  import { base64ToBytes } from "$lib/largebody/format";

  interface Props {
    path: string;
  }

  let { path }: Props = $props();

  const resp = $derived(ws.responses[path]);

  // Live elapsed time while a send is in progress.
  let now = $state(Date.now());
  $effect(() => {
    if (!resp?.loading) return;
    now = Date.now();
    const id = setInterval(() => (now = Date.now()), 100);
    return () => clearInterval(id);
  });
  const elapsedMs = $derived(resp?.loading && resp.startedAt ? Math.max(0, now - resp.startedAt) : 0);
  const result = $derived(resp?.result ?? null);

  const loc = $derived(ws.index.get(path) ?? null);

  let tab = $state<"body" | "headers" | "tests" | "console">("body");

  const tests = $derived(result?.tests ?? []);
  const logs = $derived(result?.logs ?? []);
  const testCounts = $derived.by(() => {
    let passed = 0,
      failed = 0,
      skipped = 0;
    for (const r of tests) {
      if (r.skipped) skipped++;
      else if (r.passed) passed++;
      else failed++;
    }
    return { passed, failed, skipped, total: tests.length };
  });
  let editor: CodeEditor | undefined = $state();

  function pretty(r: SendResult): { text: string; language: "json" | "text" } {
    const ct = r.headers.find(([k]) => k.toLowerCase() === "content-type")?.[1] ?? "";
    if (ct.toLowerCase().includes("json")) {
      try {
        return { text: JSON.stringify(JSON.parse(r.body), null, 2), language: "json" };
      } catch {
        return { text: r.body, language: "text" };
      }
    }
    return { text: r.body, language: "text" };
  }

  /** D41: small text bodies use CodeEditor; large ones the virtualized viewer; binary ones a hex preview. */
  const handle = $derived(result?.body_handle ?? null);
  const bodyMode = $derived<"editor" | "large" | "binary">(
    handle && !handle.is_text ? "binary" : handle?.preview_truncated ? "large" : "editor",
  );
  const body = $derived(
    result ? (bodyMode === "editor" ? pretty(result) : { text: "", language: "text" as const }) : null,
  );

  // D44: rendered preview (image, PDF, audio, video, HTML) besides the binary/text view.
  let sniffed = $state<PreviewType | null>(null);
  const preview = $derived(handle ? (previewFromMime(handle.content_type) ?? sniffed) : null);
  let bodyView = $state<"preview" | "raw">("preview");
  let lastHandleId: string | null = null;
  $effect.pre(() => {
    const h = handle;
    if ((h?.id ?? null) === lastHandleId) return;
    lastHandleId = h?.id ?? null;
    sniffed = null;
    // HTML opens as code (what an API client usually wants); media opens rendered.
    bodyView = previewFromMime(h?.content_type)?.kind === "html" ? "raw" : "preview";
    if (h && h.size > 0 && !previewFromMime(h.content_type) && isGenericMime(h.content_type)) {
      const id = h.id;
      api
        .responseRead(id, 0, 512)
        .then((c) => {
          if (lastHandleId !== id) return;
          const bytes = c.encoding === "base64" ? base64ToBytes(c.data) : new TextEncoder().encode(c.data);
          sniffed = sniffPreview(bytes);
        })
        .catch(() => {});
    }
  });
  const showPreview = $derived(tab === "body" && !!handle && !!preview && bodyView === "preview");

  function formatSize(n: number): string {
    if (n < 1024) return `${n} B`;
    if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
    return `${(n / 1024 / 1024).toFixed(2)} MB`;
  }
</script>

<section class="response">
  <div class="head">
    <span class="title">{t("response.title")}</span>
    {#if result}
      <span class="status" class:ok={result.status < 400}>{result.status} {result.status_text}</span>
      <span class="meta">{t("response.time", { ms: Math.round(result.elapsed_ms) })}</span>
      <span class="meta">{formatSize(result.size_bytes)}</span>
      {#if result.network}<NetworkInfo network={result.network} />{/if}
    {/if}
    {#if resp?.loading}
      <span class="meta sending">
        {t("response.sendingFor", { s: (elapsedMs / 1000).toFixed(1) })}
        <button class="cancel-link" onclick={() => ws.cancelSend(path)} title={t("request.cancelHint")}>
          {t("request.cancel")}
        </button>
      </span>
    {:else if resp?.cancelled}
      <span class="meta">{t("response.cancelled")}</span>
    {/if}
    {#if loc}
      <div class="head-right">
        <RuntimeVariables collection={loc.collection} envName={ws.envFor(loc.collection) || null} />
      </div>
    {/if}
  </div>

  {#if result?.script_error}
    <div class="script-error" role="alert">
      <strong>{t("response.scriptError")}</strong>
      <pre>{result.script_error}</pre>
    </div>
  {/if}

  {#if resp?.error}
    <RequestErrorView error={resp.error} {path} />
  {:else if result && body}
    <div class="tabs">
      <button class:active={tab === "body"} onclick={() => (tab = "body")}>{t("response.tab.body")}</button>
      <button class:active={tab === "headers"} onclick={() => (tab = "headers")}>
        {t("response.tab.headers")}<span class="count">{result.headers.length}</span>
      </button>
      <button class:active={tab === "tests"} onclick={() => (tab = "tests")}>
        {t("response.tab.tests")}{#if testCounts.total > 0}<span
            class="count tests-count"
            class:pass={testCounts.failed === 0}
            class:fail={testCounts.failed > 0}>{testCounts.passed}/{testCounts.total}</span
          >{/if}
      </button>
      <button class:active={tab === "console"} onclick={() => (tab = "console")}>
        {t("response.tab.console")}{#if logs.length}<span class="count">{logs.length}</span>{/if}
      </button>
      {#if tab === "body" && preview}
        <div class="viewtoggle" role="group" aria-label={t("preview.view")}>
          <button class:active={bodyView === "preview"} onclick={() => (bodyView = "preview")}>{t("preview.preview")}</button>
          <button class:active={bodyView === "raw"} onclick={() => (bodyView = "raw")}>
            {handle?.is_text ? t("preview.code") : t("preview.binary")}
          </button>
        </div>
      {/if}
      {#if tab === "body" && bodyMode === "editor" && !showPreview}
        <div class="tools">
          {#if body.language === "json"}
            <button class="ghost small" onclick={() => editor?.foldAll()}>{t("response.collapseAll")}</button>
            <button class="ghost small" onclick={() => editor?.unfoldAll()}>{t("response.expandAll")}</button>
          {/if}
          <button class="ghost small" onclick={() => editor?.openSearch()}>{t("response.search")}</button>
        </div>
      {/if}
    </div>
    <div class="content">
      {#if showPreview && handle && preview}
        {#key handle.id}<MediaPreview {handle} type={preview} />{/key}
      {:else if tab === "body" && handle && bodyMode === "large"}
        {#key handle.id}<LargeBodyViewer {handle} />{/key}
      {:else if tab === "body" && handle && bodyMode === "binary"}
        {#key handle.id}<BinaryBodyView {handle} />{/key}
      {:else if tab === "body"}
        <div class="code">
          <CodeEditor
            bind:this={editor}
            value={body.text}
            language={body.language}
            readonly
            placeholder={t("response.emptyBody")}
          />
        </div>
      {:else if tab === "tests"}
        <div class="scroll">
          {#if tests.length === 0}
            <p class="hint">{t("response.tests.none")}</p>
          {:else}
            <p class="hint">
              {t("response.tests.summary", {
                passed: testCounts.passed,
                failed: testCounts.failed,
                skipped: testCounts.skipped,
              })}
            </p>
            <ul class="tests">
              {#each tests as r, i (i)}
                {@const state = r.skipped ? "skip" : r.passed ? "pass" : "fail"}
                <li class={state}>
                  <span class="badge">
                    {state === "skip"
                      ? t("response.tests.skipped")
                      : state === "pass"
                        ? t("response.tests.passed")
                        : t("response.tests.failed")}
                  </span>
                  <span class="tname">{r.name}</span>
                  {#if r.error}<pre class="terr">{r.error}</pre>{/if}
                </li>
              {/each}
            </ul>
          {/if}
        </div>
      {:else if tab === "console"}
        <div class="scroll console mono">
          {#if logs.length === 0}
            <p class="hint">{t("response.console.empty")}</p>
          {:else}
            {#each logs as line, i (i)}<pre class="log">{line}</pre>{/each}
          {/if}
        </div>
      {:else}
        <div class="scroll">
          <table>
            <thead>
              <tr><th>{t("response.headerName")}</th><th>{t("response.headerValue")}</th></tr>
            </thead>
            <tbody>
              {#each result.headers as [k, v], i (i)}
                <tr><td class="mono">{k}</td><td class="mono">{v}</td></tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}
    </div>
  {:else if !resp?.loading}
    <p class="empty">{t("response.empty")}</p>
  {/if}
</section>

<style>
  .response {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    padding: 8px 12px 12px;
  }
  .head {
    display: flex;
    align-items: baseline;
    gap: 14px;
    padding: 4px 0 6px;
  }
  .title {
    font-weight: 600;
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--muted);
  }
  .status {
    font-weight: 700;
    color: var(--err);
  }
  .status.ok {
    color: var(--ok);
  }
  .meta {
    color: var(--muted);
    font-size: 13px;
  }
  .tools {
    margin-left: auto;
    display: flex;
    gap: 2px;
    align-items: center;
  }
  .small {
    padding: 3px 8px;
    font-size: 12px;
  }
  .content {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    padding-top: 8px;
  }
  .code {
    flex: 1;
    min-height: 0;
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow: hidden;
  }
  .scroll {
    flex: 1;
    overflow: auto;
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
    word-break: break-all;
    vertical-align: top;
  }
  td:first-child {
    width: 35%;
    font-weight: 600;
  }
  .head-right {
    margin-left: auto;
    align-self: center;
  }
  .script-error {
    color: var(--err);
    background: color-mix(in srgb, var(--err) 10%, transparent);
    border: 1px solid var(--err);
    border-radius: 6px;
    padding: 6px 10px;
    margin-bottom: 6px;
    max-height: 30%;
    overflow: auto;
    flex: none;
  }
  .script-error pre {
    margin: 4px 0 0;
    white-space: pre-wrap;
    word-break: break-word;
    font-family: var(--mono);
    font-size: 12px;
  }
  .tests-count.pass {
    color: var(--ok);
    font-weight: 600;
  }
  .tests-count.fail {
    color: var(--err);
    font-weight: 600;
  }
  .hint {
    color: var(--muted);
    font-size: 13px;
    margin: 4px 0 8px;
  }
  .tests {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .tests li {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 8px;
    padding: 5px 4px;
    border-bottom: 1px solid var(--border);
    font-size: 13px;
  }
  .tests .badge {
    flex: none;
    font-size: 11px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    padding: 1px 6px;
    border-radius: 4px;
    min-width: 72px;
    text-align: center;
  }
  .tests .pass .badge {
    color: var(--ok);
    background: color-mix(in srgb, var(--ok) 14%, transparent);
  }
  .tests .fail .badge {
    color: var(--err);
    background: color-mix(in srgb, var(--err) 14%, transparent);
  }
  .tests .skip .badge {
    color: var(--muted);
    background: var(--surface-2);
  }
  .tname {
    flex: 1;
    min-width: 0;
    word-break: break-word;
  }
  .terr {
    flex-basis: 100%;
    margin: 0 0 0 80px;
    color: var(--err);
    white-space: pre-wrap;
    word-break: break-word;
    font-family: var(--mono);
    font-size: 12px;
  }
  .console {
    font-size: 12px;
  }
  .log {
    margin: 0;
    padding: 3px 6px;
    border-bottom: 1px solid var(--border);
    white-space: pre-wrap;
    word-break: break-word;
    font-family: var(--mono);
  }
  .error {
    color: var(--err);
    border: 1px solid var(--err);
    border-radius: 6px;
    padding: 8px 12px;
    overflow: auto;
  }
  .error pre {
    margin: 6px 0 0;
    white-space: pre-wrap;
    word-break: break-word;
    font-family: var(--mono);
    font-size: 12px;
  }
  .empty {
    color: var(--muted);
    margin: auto;
  }
  .sending {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-variant-numeric: tabular-nums;
  }
  .cancel-link {
    border: 1px solid var(--err);
    background: none;
    color: var(--err);
    border-radius: 5px;
    padding: 1px 8px;
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }
  .cancel-link:hover {
    background: var(--err);
    color: var(--accent-text);
  }
  .viewtoggle {
    display: flex;
    margin-left: auto;
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow: hidden;
    align-self: center;
  }
  .viewtoggle + .tools {
    margin-left: 8px;
  }
  .viewtoggle button {
    border: none;
    border-radius: 0;
    background: none;
    padding: 2px 10px;
    font-size: 12px;
    color: var(--muted);
  }
  .viewtoggle button.active {
    background: color-mix(in srgb, var(--accent) 16%, transparent);
    color: var(--accent);
  }
</style>
