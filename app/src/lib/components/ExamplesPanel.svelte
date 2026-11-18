<script lang="ts">
  // "Examples" tab of a request (D62): saved responses the mock server answers with.
  // Saved in the request file ([[examples]]), so they are versioned with it.
  import type { Example } from "$lib/api";
  import { t } from "$lib/i18n/index.svelte";
  import { ws, type Draft } from "$lib/stores/workspace.svelte";
  import CodeEditor from "./CodeEditor.svelte";
  import { exampleFromResponse } from "$lib/mock/examples";

  interface Props {
    draft: Draft;
    /** Key of the draft's response in ws.responses ("Save last response"). */
    path: string;
  }

  let { draft, path }: Props = $props();
  let selected = $state(0);
  const current = $derived(draft.examples[selected] ?? null);
  const result = $derived(ws.responses[path]?.result ?? null);

  function add(e: Example) {
    draft.examples = [...draft.examples, e];
    selected = draft.examples.length - 1;
  }

  async function fromResponse() {
    if (result) add(await exampleFromResponse(result));
  }

  function remove(i: number) {
    draft.examples = draft.examples.filter((_, j) => j !== i);
    selected = Math.max(0, Math.min(selected, draft.examples.length - 1));
  }

  /** Headers as "Name: value" lines (simple to edit and paste). */
  const headerText = $derived(
    current ? Object.entries(current.headers ?? {}).map(([k, v]) => `${k}: ${v}`).join("\n") : "",
  );
  function setHeaders(text: string) {
    if (!current) return;
    const out: Record<string, string> = {};
    for (const line of text.split("\n")) {
      const i = line.indexOf(":");
      if (i > 0) out[line.slice(0, i).trim()] = line.slice(i + 1).trim();
    }
    current.headers = out;
  }

  const isJson = $derived.by(() => {
    if (!current?.body) return true;
    try {
      JSON.parse(current.body);
      return true;
    } catch {
      return false;
    }
  });
</script>

<div class="examples">
  <aside>
    <div class="buttons">
      <button class="small" onclick={() => add({ name: "OK", status: 200, headers: { "Content-Type": "application/json" }, body: "{\n  \n}" })}>
        + {t("examples.new")}
      </button>
      <button class="small" disabled={!result} title={t("examples.fromResponseHint")} onclick={fromResponse}>
        {t("examples.fromResponse")}
      </button>
    </div>
    {#each draft.examples as e, i (i)}
      <button class="item" class:active={i === selected} onclick={() => (selected = i)}>
        <span class="status" class:ok={e.status < 400}>{e.status}</span>
        <span class="name">{e.name}</span>
      </button>
    {:else}
      <p class="hint">{t("examples.empty")}</p>
    {/each}
  </aside>

  {#if current}
    <div class="editor">
      <div class="row">
        <label>{t("examples.name")}<input bind:value={current.name} /></label>
        <label class="narrow">{t("examples.status")}<input type="number" min="100" max="599" bind:value={current.status} /></label>
        <label class="narrow" title={t("examples.delayHint")}>{t("examples.delay")}
          <input type="number" min="0" step="100" value={current.delay_ms ?? ""} oninput={(e) => (current.delay_ms = e.currentTarget.value ? Number(e.currentTarget.value) : null)} />
        </label>
        <button class="ghost danger" onclick={() => remove(selected)} title={t("examples.delete")}>✕</button>
      </div>
      <label class="block">{t("examples.headers")}
        <textarea class="mono" rows="3" spellcheck="false" value={headerText} onchange={(e) => setHeaders(e.currentTarget.value)} placeholder="Content-Type: application/json"></textarea>
      </label>
      <div class="block grow">
        <span>{t("examples.body")}</span>
        <div class="code">
          {#key selected}
            <CodeEditor value={current.body ?? ""} onchange={(v) => (current.body = v)} language={isJson ? "json" : "text"} />
          {/key}
        </div>
      </div>
      <p class="hint">{t("examples.mockHint")}</p>
    </div>
  {/if}
</div>

<style>
  .examples {
    display: flex;
    gap: 10px;
    height: 100%;
    min-height: 0;
    padding-top: 8px;
  }
  aside {
    flex: none;
    width: 200px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    overflow: auto;
  }
  .buttons {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin-bottom: 6px;
  }
  .item {
    display: flex;
    gap: 8px;
    align-items: center;
    border: none;
    background: none;
    text-align: left;
    padding: 4px 6px;
  }
  .item.active {
    background: var(--selected);
  }
  .item:hover:not(.active) {
    background: var(--hover);
  }
  .status {
    font-family: var(--mono);
    font-size: var(--fs-xs);
    color: var(--err);
  }
  .status.ok {
    color: var(--ok);
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .editor {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .row {
    display: flex;
    gap: 8px;
    align-items: flex-end;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: var(--fs-xs);
    color: var(--muted);
    flex: 1;
  }
  label.narrow {
    flex: 0 0 90px;
  }
  .block {
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: var(--fs-xs);
    color: var(--muted);
  }
  .grow {
    flex: 1;
    min-height: 0;
  }
  .code {
    flex: 1;
    min-height: 80px;
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow: hidden;
  }
  textarea {
    resize: vertical;
    font-size: var(--fs-sm);
  }
  .hint {
    color: var(--muted);
    font-size: var(--fs-xs);
    margin: 0;
  }
  .danger {
    color: var(--err);
  }
</style>
