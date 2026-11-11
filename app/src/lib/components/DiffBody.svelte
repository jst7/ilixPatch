<script lang="ts">
  // Two versions of one file, compared: semantic summary for request .toml files (D16), then either a unified
  // line diff (own renderer, collapsed unchanged runs) or the side-by-side DiffView (D24). null = file absent.
  import { t, type MessageKey } from "$lib/i18n/index.svelte";
  import { git } from "$lib/stores/git.svelte";
  import { diffLines, diffStats, segmentDiff, splitLines } from "$lib/git/diff";
  import { semanticDiff, type SemChange } from "$lib/git/semantic";
  import DiffView from "./merge/DiffView.svelte";

  interface Props {
    old: string | null;
    new: string | null;
    path: string;
    oldTitle: string;
    newTitle: string;
  }

  let { old: oldText, new: newText, path, oldTitle, newTitle }: Props = $props();

  /** Lines per side above which the diff is truncated. */
  const MAX_LINES = 5000;

  const prefs = git.diffPrefs;
  let expanded = $state<Record<number, boolean>>({});

  // Collapsed sections start closed whenever the compared contents change.
  $effect(() => {
    void oldText;
    void newText;
    expanded = {};
  });

  const lower = $derived(path.toLowerCase());
  const language = $derived(lower.endsWith(".json") ? "json" : /\.(m?js|ts)$/.test(lower) ? "javascript" : "text");
  const semantic = $derived(lower.endsWith(".toml") ? semanticDiff(oldText, newText) : null);

  const text = $derived.by(() => {
    const oldAll = splitLines(oldText);
    const newAll = splitLines(newText);
    const truncated = oldAll.length > MAX_LINES || newAll.length > MAX_LINES;
    const lines = diffLines(oldAll.slice(0, MAX_LINES), newAll.slice(0, MAX_LINES));
    return { truncated, lines, segments: segmentDiff(lines), stats: diffStats(lines) };
  });

  /** Truncated sources for the side-by-side view (same cap as the unified diff). */
  function cap(s: string | null): string | null {
    if (s === null) return null;
    const lines = s.split("\n");
    return lines.length > MAX_LINES ? lines.slice(0, MAX_LINES).join("\n") : s;
  }

  const KEYS: Record<SemChange["field"], Record<SemChange["kind"], MessageKey>> = {
    request: { added: "git.sem.requestAdded", removed: "git.sem.requestRemoved", changed: "git.sem.requestAdded" },
    name: { added: "git.sem.nameChanged", removed: "git.sem.nameChanged", changed: "git.sem.nameChanged" },
    method: { added: "git.sem.methodChanged", removed: "git.sem.methodChanged", changed: "git.sem.methodChanged" },
    url: { added: "git.sem.urlChanged", removed: "git.sem.urlChanged", changed: "git.sem.urlChanged" },
    header: { added: "git.sem.headerAdded", removed: "git.sem.headerRemoved", changed: "git.sem.headerChanged" },
    query: { added: "git.sem.queryAdded", removed: "git.sem.queryRemoved", changed: "git.sem.queryChanged" },
    form: { added: "git.sem.formAdded", removed: "git.sem.formRemoved", changed: "git.sem.formChanged" },
    bodyType: { added: "git.sem.bodyAdded", removed: "git.sem.bodyRemoved", changed: "git.sem.bodyTypeChanged" },
    body: { added: "git.sem.bodyChanged", removed: "git.sem.bodyChanged", changed: "git.sem.bodyChanged" },
    preRequest: { added: "git.sem.preAdded", removed: "git.sem.preRemoved", changed: "git.sem.preChanged" },
    tests: { added: "git.sem.testsAdded", removed: "git.sem.testsRemoved", changed: "git.sem.testsChanged" },
  };

  function label(c: SemChange): string {
    return t(KEYS[c.field][c.kind], { key: c.key ?? "", type: c.new ?? c.old ?? "", name: c.new ?? c.old ?? "" });
  }

  /** Show "old → new" for scalar changes (not for whole-request or multi-line changes). */
  function showValues(c: SemChange): boolean {
    return c.field !== "request" && c.field !== "bodyType" && c.added === undefined && (c.old !== undefined || c.new !== undefined);
  }

  const showText = $derived(!semantic || prefs.showText);
</script>

<div class="diff-body">
  <div class="bar">
    <span class="stats mono"><span class="plus">+{text.stats.added}</span> <span class="minus">−{text.stats.removed}</span></span>
    <span class="spacer"></span>
    {#if semantic}
      <button class="ghost small" aria-pressed={prefs.showText} onclick={() => git.setDiffPref("showText", !prefs.showText)}>
        {prefs.showText ? t("git.hideTextDiff") : t("git.showTextDiff")}
      </button>
    {/if}
    {#if showText}
      <div class="seg" role="group" aria-label={t("git.diffMode")}>
        <button class="small" aria-pressed={prefs.mode === "unified"} onclick={() => git.setDiffPref("mode", "unified")}
          >{t("git.diffUnified")}</button
        >
        <button class="small" aria-pressed={prefs.mode === "split"} onclick={() => git.setDiffPref("mode", "split")}
          >{t("git.diffSplit")}</button
        >
      </div>
    {/if}
  </div>

  {#if semantic}
    <section class="semantic" class:grow={!showText} aria-label={t("git.semanticDiff")}>
      {#if semantic.length === 0}
        <p class="muted">{t("git.sem.formatOnly")}</p>
      {:else}
        <ul>
          {#each semantic as c, i (i)}
            <li class={c.kind}>
              <span class="mark mono" aria-hidden="true">{c.kind === "added" ? "+" : c.kind === "removed" ? "−" : "~"}</span>
              <span class="what">{label(c)}</span>
              {#if showValues(c)}
                <span class="values mono">
                  {#if c.old !== undefined}<span class="old">{c.old || t("git.sem.empty")}</span>{/if}
                  {#if c.old !== undefined && c.new !== undefined}<span class="arrow">→</span>{/if}
                  {#if c.new !== undefined}<span class="new">{c.new || t("git.sem.empty")}</span>{/if}
                </span>
              {/if}
              {#if c.added !== undefined}
                <span class="stats mono"><span class="plus">+{c.added}</span> <span class="minus">−{c.removed}</span></span>
              {/if}
            </li>
          {/each}
        </ul>
      {/if}
    </section>
  {/if}

  {#if showText}
    {#if text.truncated}<p class="notice">{t("git.diffTruncated", { count: MAX_LINES.toLocaleString() })}</p>{/if}
    {#if prefs.mode === "split"}
      <div class="split">
        <DiffView left={cap(oldText)} right={cap(newText)} leftTitle={oldTitle} rightTitle={newTitle} {language} />
      </div>
    {:else if text.lines.length === 0}
      <p class="muted pad">{oldText === null && newText === null ? t("git.noContent") : t("git.noTextChanges")}</p>
    {:else}
      <div class="scroll">
        <div class="lines mono" role="table" aria-label={t("git.textDiff")}>
          {#each text.segments as seg, si (si)}
            {#if seg.kind === "gap" && !expanded[seg.id]}
              <button class="gap" onclick={() => (expanded[seg.id] = true)}>
                ⋯ {t("git.unchangedLines", { count: seg.lines.length })}
              </button>
            {:else}
              {#each seg.lines as line, li (li)}
                <div class="line {line.kind}" role="row">
                  <span class="no" role="cell">{line.oldNo ?? ""}</span>
                  <span class="no" role="cell">{line.newNo ?? ""}</span>
                  <span class="sign" role="cell">{line.kind === "add" ? "+" : line.kind === "del" ? "−" : " "}</span>
                  <span class="code" role="cell">{line.text}</span>
                </div>
              {/each}
            {/if}
          {/each}
        </div>
      </div>
    {/if}
  {/if}
</div>

<style>
  .diff-body {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    min-width: 0;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 10px;
    border-bottom: 1px solid var(--border);
    font-size: 12px;
  }
  .spacer {
    flex: 1;
  }
  .small {
    padding: 2px 8px;
    font-size: 12px;
  }
  .seg {
    display: inline-flex;
  }
  .seg button {
    background: none;
    color: var(--muted);
  }
  .seg button:first-child {
    border-top-right-radius: 0;
    border-bottom-right-radius: 0;
  }
  .seg button:last-child {
    border-left: none;
    border-top-left-radius: 0;
    border-bottom-left-radius: 0;
  }
  .seg button[aria-pressed="true"] {
    color: var(--text);
    background: var(--hover);
  }
  .muted {
    color: var(--muted);
  }
  .pad {
    padding: 0 12px;
  }
  .notice {
    margin: 8px 12px;
    color: var(--warn);
    font-size: 12px;
  }
  .plus {
    color: var(--ok);
  }
  .minus {
    color: var(--err);
  }
  .semantic {
    flex: none;
    max-height: 40%;
    overflow: auto;
    padding: 8px 12px;
    border-bottom: 1px solid var(--border);
  }
  .semantic.grow {
    flex: 1;
    max-height: none;
    border-bottom: none;
  }
  .semantic ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .semantic li {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 6px;
    font-size: 13px;
  }
  .semantic li.added .mark {
    color: var(--ok);
  }
  .semantic li.removed .mark {
    color: var(--err);
  }
  .semantic li.changed .mark {
    color: var(--warn);
  }
  .mark {
    width: 12px;
    font-weight: 700;
  }
  .values {
    display: inline-flex;
    flex-wrap: wrap;
    gap: 6px;
    font-size: 12px;
    word-break: break-all;
  }
  .values .old {
    color: var(--err);
    text-decoration: line-through;
  }
  .values .new {
    color: var(--ok);
  }
  .arrow {
    color: var(--muted);
  }
  .split {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .scroll {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }
  .lines {
    font-size: 12px;
    line-height: 18px;
    min-width: max-content;
    padding-bottom: 8px;
  }
  .line {
    display: flex;
  }
  .line.add {
    background: var(--diff-added-bg);
  }
  .line.del {
    background: var(--diff-removed-bg);
  }
  .no {
    flex: none;
    width: 44px;
    padding-right: 6px;
    text-align: right;
    color: var(--muted);
    user-select: none;
  }
  .sign {
    flex: none;
    width: 16px;
    text-align: center;
    color: var(--muted);
    user-select: none;
  }
  .line.add .sign {
    color: var(--ok);
  }
  .line.del .sign {
    color: var(--err);
  }
  .code {
    white-space: pre;
    padding-right: 12px;
  }
  .gap {
    display: block;
    width: 100%;
    text-align: left;
    border: none;
    border-radius: 0;
    background: var(--surface-2);
    color: var(--muted);
    padding: 2px 12px 2px 110px;
    font-size: 12px;
  }
  .gap:hover {
    color: var(--text);
    background: var(--hover);
  }
</style>
