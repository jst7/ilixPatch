<script lang="ts">
  import { t } from "$lib/i18n/index.svelte";
  import type { Draft } from "$lib/stores/workspace.svelte";
  import CodeEditor from "./CodeEditor.svelte";
  import type { BlameInfo } from "$lib/api";
  import { SNIPPETS, type ScriptKind } from "./snippets";

  interface Props {
    /** Reactive draft (mutated in place). */
    draft: Draft;
    /** Request path; editors are recreated when it changes. */
    path: string;
    /** Git annotate lines for the pre-request / tests scripts (null hides the gutter). */
    preAnnotations?: BlameInfo[] | null;
    testsAnnotations?: BlameInfo[] | null;
    onannotationclick?: (info: BlameInfo) => void;
  }

  let { draft, path, preAnnotations = null, testsAnnotations = null, onannotationclick }: Props = $props();

  let kind = $state<ScriptKind>("pre");
  let editor: CodeEditor | undefined = $state();

  const snippets = $derived(SNIPPETS.filter((s) => s.where.includes(kind)));

  function insertSnippet(code: string) {
    if (editor) editor.insert(code);
    else if (kind === "pre") draft.preRequest += (draft.preRequest ? "\n" : "") + code;
    else draft.tests += (draft.tests ? "\n" : "") + code;
  }
</script>

<div class="scripts">
  <div class="kinds" role="tablist">
    <button role="tab" aria-selected={kind === "pre"} class:active={kind === "pre"} onclick={() => (kind = "pre")}>
      {t("request.scripts.preRequest")}
      {#if draft.preRequest.trim()}<span class="dot" title={t("request.scripts.hasScript")}></span>{/if}
    </button>
    <button role="tab" aria-selected={kind === "tests"} class:active={kind === "tests"} onclick={() => (kind = "tests")}>
      {t("request.scripts.tests")}
      {#if draft.tests.trim()}<span class="dot" title={t("request.scripts.hasScript")}></span>{/if}
    </button>
  </div>

  <div class="main">
    <div class="editor-col">
      <p class="hint">{kind === "pre" ? t("request.scripts.preRequestHint") : t("request.scripts.testsHint")}</p>
      <div class="code">
        {#key path + kind}
          {#if kind === "pre"}
            <CodeEditor
              bind:this={editor}
              value={draft.preRequest}
              onchange={(v) => (draft.preRequest = v)}
              language="javascript"
              annotations={preAnnotations}
              {onannotationclick}
            />
          {:else}
            <CodeEditor
              bind:this={editor}
              value={draft.tests}
              onchange={(v) => (draft.tests = v)}
              language="javascript"
              annotations={testsAnnotations}
              {onannotationclick}
            />
          {/if}
        {/key}
      </div>
    </div>

    <aside class="snippets" aria-label={t("request.scripts.snippets")}>
      <div class="snip-title">{t("request.scripts.snippets")}</div>
      <div class="snip-hint">{t("request.scripts.snippetsHint")}</div>
      <ul>
        {#each snippets as s (s.id)}
          <li>
            <button class="snip" onclick={() => insertSnippet(s.code(t(s.label)))}>{t(s.label)}</button>
          </li>
        {/each}
      </ul>
    </aside>
  </div>
</div>

<style>
  .scripts {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding-top: 8px;
  }
  .kinds {
    display: flex;
    gap: 4px;
  }
  .kinds button {
    padding: 3px 10px;
    font-size: 12px;
    border-radius: 999px;
    color: var(--muted);
    background: none;
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .kinds button.active {
    color: var(--text);
    background: var(--surface-2);
    border-color: var(--accent);
  }
  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--ok);
  }
  .main {
    flex: 1;
    min-height: 0;
    display: flex;
    gap: 10px;
  }
  .editor-col {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .hint {
    margin: 0;
    color: var(--muted);
    font-size: 12px;
  }
  .code {
    flex: 1;
    min-height: 0;
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow: hidden;
  }
  .snippets {
    width: 210px;
    flex: none;
    overflow: auto;
    border-left: 1px solid var(--border);
    padding-left: 10px;
  }
  .snip-title {
    font-weight: 600;
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--muted);
  }
  .snip-hint {
    font-size: 11px;
    color: var(--muted);
    margin: 2px 0 6px;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .snip {
    width: 100%;
    text-align: left;
    border: none;
    background: none;
    color: var(--accent);
    font-size: 12px;
    padding: 4px 6px;
    border-radius: 4px;
  }
  .snip:hover {
    background: var(--hover);
  }
</style>
