<script lang="ts">
  // Response comparator (D64). "Summary" lists only the data differences (grouped, with old → new); "Normalized" and
  // "As is" are text diffs. Normalizing (sorted keys/arrays, trimmed strings, ignored paths) tells order/format
  // differences apart from data ones.
  import { diff, kindOf, normalize, summary, tryParse, type Change, type Json, type JsonKind } from "$lib/json/model";
  import { compares } from "$lib/stores/compare.svelte";
  import { t, type MessageKey } from "$lib/i18n/index.svelte";
  import DiffBody from "./DiffBody.svelte";

  interface Props {
    id: string;
  }
  let { id }: Props = $props();

  const session = $derived(compares.sessions[id] ?? null);
  let mode = $state<"summary" | "normalized" | "raw">("summary");
  let sortKeys = $state(true);
  let sortArrays = $state(true);
  let trimStrings = $state(true);
  let ignoreText = $state("");
  let open = $state<Record<string, boolean>>({});

  const opts = $derived({
    sortKeys,
    sortArrays,
    trimStrings,
    ignore: ignoreText.split(/[,\n]/).map((s) => s.trim()).filter(Boolean),
  });
  const a = $derived(session ? tryParse(session.left.text) : undefined);
  const b = $derived(session ? tryParse(session.right.text) : undefined);
  const bothJson = $derived(a !== undefined && b !== undefined);

  const pretty = (v: Json | undefined) => (v === undefined ? "" : JSON.stringify(v, null, 2));
  const na = $derived(bothJson ? normalize(a as Json, opts) : undefined);
  const nb = $derived(bothJson ? normalize(b as Json, opts) : undefined);
  const changes = $derived<Change[]>(bothJson ? diff((na ?? null) as Json, (nb ?? null) as Json) : []);
  const rawChanges = $derived(bothJson ? diff(a as Json, b as Json).length : 0);

  const verdict = $derived.by((): { kind: "same" | "order" | "data" | "text"; key: MessageKey } => {
    if (!session) return { kind: "same", key: "compare.identical" };
    if (!bothJson) {
      return session.left.text === session.right.text
        ? { kind: "same", key: "compare.identical" }
        : { kind: "text", key: "compare.notJson" };
    }
    if (rawChanges === 0 && JSON.stringify(a) === JSON.stringify(b)) return { kind: "same", key: "compare.identical" };
    if (changes.length === 0) return { kind: "order", key: "compare.orderOnly" };
    return { kind: "data", key: "compare.dataDiffs" };
  });

  const counts = $derived({
    changed: changes.filter((c) => c.kind === "changed").length,
    added: changes.filter((c) => c.kind === "added").length,
    removed: changes.filter((c) => c.kind === "removed").length,
  });

  /** Changes grouped by their first path segment (the "section" of the document). */
  const groups = $derived.by(() => {
    const map = new Map<string, Change[]>();
    for (const c of changes) {
      const m = c.path.match(/^([^.[]+|\[[^\]]*\])/);
      const key = c.path === "(root)" ? "(root)" : (m?.[1] ?? c.path);
      map.set(key, [...(map.get(key) ?? []), c]);
    }
    return [...map.entries()];
  });

  const KIND: Record<JsonKind, MessageKey> = {
    null: "compare.kind.null",
    boolean: "compare.kind.boolean",
    number: "compare.kind.number",
    string: "compare.kind.string",
    array: "compare.kind.array",
    object: "compare.kind.object",
  };

  /** Extra explanation of a change: type change, array length, numeric delta. */
  function note(c: Change): string | null {
    if (c.kind !== "changed" || c.old === undefined || c.new === undefined) return null;
    const ko = kindOf(c.old);
    const kn = kindOf(c.new);
    if (ko !== kn) return t("compare.typeChange", { from: t(KIND[ko]), to: t(KIND[kn]) });
    if (ko === "number") {
      const d = (c.new as number) - (c.old as number);
      return `${d > 0 ? "+" : ""}${+d.toFixed(6)}`;
    }
    return null;
  }

  function lengthNotes(): string[] {
    if (!bothJson) return [];
    const out: string[] = [];
    const walk = (x: Json | undefined, y: Json | undefined, path: string) => {
      if (Array.isArray(x) && Array.isArray(y)) {
        if (x.length !== y.length) out.push(t("compare.lengthChange", { path: path || "$", from: x.length, to: y.length }));
        x.slice(0, Math.min(x.length, y.length, 200)).forEach((c, i) => walk(c, y[i], `${path}[${i}]`));
      } else if (x && y && typeof x === "object" && typeof y === "object" && !Array.isArray(x) && !Array.isArray(y)) {
        for (const k of Object.keys(x)) if (k in y) walk(x[k], y[k], path ? `${path}.${k}` : k);
      }
    };
    walk(na, nb, "");
    return out.slice(0, 20);
  }
  const lengths = $derived(lengthNotes());

  const textLeft = $derived(
    !session ? "" : !bothJson ? session.left.text : mode === "raw" ? pretty(a) : pretty(na),
  );
  const textRight = $derived(
    !session ? "" : !bothJson ? session.right.text : mode === "raw" ? pretty(b) : pretty(nb),
  );
</script>

{#if session}
  <section class="compare">
    <header>
      <div class="sides">
        <span class="side left">◀ {session.left.label}</span>
        {#if !session.pasted}<button class="ghost small" onclick={() => compares.swap(id)} title={t("compare.swap")}>⇄</button>{/if}
        <span class="side right">{session.right.label} ▶</span>
      </div>
      <div class="modes" role="group">
        <button class:active={mode === "summary"} onclick={() => (mode = "summary")}>{t("compare.mode.summary")}</button>
        <button class:active={mode === "normalized"} onclick={() => (mode = "normalized")}>{t("compare.mode.normalized")}</button>
        <button class:active={mode === "raw"} onclick={() => (mode = "raw")}>{t("compare.mode.raw")}</button>
      </div>
    </header>

    {#if session.pasted}
      <textarea
        class="paste mono"
        rows="4"
        spellcheck="false"
        placeholder={t("compare.pasteHint")}
        bind:value={session.right.text}
      ></textarea>
    {/if}

    {#if mode !== "raw" && bothJson}
      <div class="opts">
        <label><input type="checkbox" bind:checked={sortKeys} /> {t("compare.opt.sortKeys")}</label>
        <label><input type="checkbox" bind:checked={sortArrays} /> {t("compare.opt.sortArrays")}</label>
        <label><input type="checkbox" bind:checked={trimStrings} /> {t("compare.opt.trim")}</label>
        <label class="ignore">
          {t("compare.opt.ignore")}
          <input class="mono" placeholder="*.updatedAt, meta.requestId, items[*].id" bind:value={ignoreText} />
        </label>
      </div>
    {/if}

    <div class="verdict {verdict.kind}" role="status">
      <strong>{t(verdict.key, { count: changes.length })}</strong>
      {#if verdict.kind === "data"}
        <span class="chips">
          {#if counts.changed}<span class="chip changed">~ {t("compare.changed", { count: counts.changed })}</span>{/if}
          {#if counts.added}<span class="chip added">+ {t("compare.added", { count: counts.added })}</span>{/if}
          {#if counts.removed}<span class="chip removed">− {t("compare.removed", { count: counts.removed })}</span>{/if}
        </span>
      {/if}
      {#if bothJson && rawChanges > changes.length && verdict.kind !== "same"}
        <span class="muted">{t("compare.alsoOrder")}</span>
      {/if}
    </div>

    <div class="body">
      {#if mode === "summary" && bothJson}
        {#if lengths.length}
          <ul class="lengths">
            {#each lengths as l (l)}<li>{l}</li>{/each}
          </ul>
        {/if}
        {#each groups as [group, list] (group)}
          <details open={list.length <= 30}>
            <summary><code>{group}</code> <span class="muted">· {list.length}</span></summary>
            <ul class="changes">
              {#each list as c (c.path)}
                {@const n = note(c)}
                <li class={c.kind}>
                  <button class="row" onclick={() => (open[c.path] = !open[c.path])}>
                    <span class="sign">{c.kind === "added" ? "+" : c.kind === "removed" ? "−" : "~"}</span>
                    <code class="path">{c.path}</code>
                    {#if c.kind !== "added"}<span class="old">{summary(c.old ?? null, 50)}</span>{/if}
                    {#if c.kind === "changed"}<span class="arrow">→</span>{/if}
                    {#if c.kind !== "removed"}<span class="new">{summary(c.new ?? null, 50)}</span>{/if}
                    {#if n}<span class="note">{n}</span>{/if}
                  </button>
                  {#if open[c.path]}
                    <div class="full">
                      {#if c.kind !== "added"}<pre class="old">{pretty(c.old)}</pre>{/if}
                      {#if c.kind !== "removed"}<pre class="new">{pretty(c.new)}</pre>{/if}
                    </div>
                  {/if}
                </li>
              {/each}
            </ul>
          </details>
        {:else}
          <p class="hint">{verdict.kind === "order" ? t("compare.orderOnlyHint") : t("compare.nothing")}</p>
        {/each}
      {:else}
        <div class="diff">
          {#key `${mode}:${textLeft.length}:${textRight.length}`}
            <DiffBody old={textLeft} new={textRight} path="compare" oldTitle={session.left.label} newTitle={session.right.label} />
          {/key}
        </div>
      {/if}
    </div>
  </section>
{:else}
  <p class="hint">{t("compare.gone")}</p>
{/if}

<style>
  .compare {
    display: flex;
    flex-direction: column;
    gap: 8px;
    height: 100%;
    min-height: 0;
    padding: 10px 12px;
  }
  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }
  .sides {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .side {
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .side.left {
    color: var(--err);
  }
  .side.right {
    color: var(--ok);
  }
  .modes button {
    border-radius: 0;
  }
  .modes button:first-child {
    border-radius: 5px 0 0 5px;
  }
  .modes button:last-child {
    border-radius: 0 5px 5px 0;
  }
  .modes button.active {
    background: var(--selected);
  }
  .paste {
    resize: vertical;
    font-size: var(--fs-sm);
  }
  .opts {
    display: flex;
    gap: 12px;
    align-items: center;
    flex-wrap: wrap;
    font-size: var(--fs-sm);
  }
  .opts label {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .ignore {
    flex: 1;
    min-width: 260px;
  }
  .ignore input {
    flex: 1;
  }
  .verdict {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
    padding: 6px 10px;
    border-radius: 6px;
    border-left: 3px solid var(--muted);
    background: var(--hover);
  }
  .verdict.same,
  .verdict.order {
    border-left-color: var(--ok);
  }
  .verdict.data,
  .verdict.text {
    border-left-color: var(--warn);
  }
  .chips {
    display: flex;
    gap: 6px;
  }
  .chip {
    font-size: var(--fs-xs);
    padding: 1px 6px;
    border-radius: 10px;
    background: var(--surface);
    border: 1px solid var(--border);
  }
  .chip.added {
    color: var(--ok);
  }
  .chip.removed {
    color: var(--err);
  }
  .chip.changed {
    color: var(--warn);
  }
  .muted {
    color: var(--muted);
    font-size: var(--fs-sm);
  }
  .body {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }
  .diff {
    height: 100%;
  }
  .lengths {
    margin: 0 0 8px;
    padding-left: 18px;
    color: var(--muted);
    font-size: var(--fs-sm);
  }
  details {
    margin-bottom: 6px;
  }
  summary {
    cursor: pointer;
    font-size: var(--fs-sm);
  }
  .changes {
    list-style: none;
    margin: 4px 0 0;
    padding: 0 0 0 12px;
  }
  .row {
    display: flex;
    align-items: baseline;
    gap: 8px;
    width: 100%;
    border: none;
    background: none;
    text-align: left;
    padding: 2px 4px;
    font-size: var(--fs-sm);
    white-space: nowrap;
    overflow: hidden;
  }
  .row:hover {
    background: var(--hover);
  }
  .sign {
    font-family: var(--mono);
    font-weight: 700;
    width: 10px;
  }
  li.added .sign {
    color: var(--ok);
  }
  li.removed .sign {
    color: var(--err);
  }
  li.changed .sign {
    color: var(--warn);
  }
  .path {
    color: var(--accent);
  }
  .old {
    color: var(--err);
    font-family: var(--mono);
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .new {
    color: var(--ok);
    font-family: var(--mono);
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .arrow,
  .note {
    color: var(--muted);
  }
  .note {
    font-size: var(--fs-xs);
  }
  .full {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
    margin: 4px 0 6px 18px;
  }
  .full pre {
    margin: 0;
    padding: 6px;
    border: 1px solid var(--border);
    border-radius: 4px;
    font-size: var(--fs-xs);
    max-height: 240px;
    overflow: auto;
  }
  .hint {
    color: var(--muted);
    padding: 8px;
  }
</style>
