<script lang="ts">
  // Popover of a {{variable}} in the URL bar (D43): its value, where it comes from,
  // and a form to change the value and the scope it is saved in (move or copy).
  import type { Collection } from "$lib/api";
  import { t } from "$lib/i18n/index.svelte";
  import { saveVar, varInfo, type FileScope, type VarLayer } from "$lib/vars/edit";

  interface Props {
    name: string;
    anchor: DOMRect;
    collection: Collection | null;
    onenter: () => void;
    onleave: () => void;
    /** true while the user edits (focus inside): hovering away does not close it. */
    onpin: (pinned: boolean) => void;
    onclose: () => void;
  }

  let { name, anchor, collection, onenter, onleave, onpin, onclose }: Props = $props();

  const info = $derived(varInfo(collection, name));
  const editable = $derived(info.layers.filter((l) => l.editable));

  function scopeLabel(l: VarLayer): string {
    if (l.scope === "global") return t("vars.scope.global");
    if (l.scope === "collection") return t("vars.scope.collection", { name: l.label });
    return t("vars.scope.environment", { name: l.label });
  }

  /** Where to save by default: where it is now, else the most specific editable scope. */
  function defaultTarget(): FileScope {
    const src = info.layers.find((l) => l.scope === info.source && l.editable);
    return src?.scope ?? editable[0]?.scope ?? "global";
  }

  // Form state; re-initialised when another variable is hovered.
  let value = $state("");
  let target = $state<FileScope>("global");
  let move = $state(true);
  let saving = $state(false);
  let error = $state<string | null>(null);
  let lastName = "";
  $effect.pre(() => {
    if (name !== lastName) {
      lastName = name;
      value = info.value ?? "";
      target = defaultTarget();
      move = true;
      error = null;
    }
  });

  /** The scope the value comes from now, if it is a file value that a move can remove. */
  const sourceLayer = $derived(
    info.layers.find((l) => l.scope === info.source && l.editable && l.file !== undefined) ?? null,
  );
  const canMove = $derived(sourceLayer !== null && sourceLayer.scope !== target);
  const shadowed = $derived(info.layers.filter((l) => l.scope !== info.source && l.file !== undefined));
  const changed = $derived(value !== (info.value ?? "") || target !== info.source || info.fromRuntime);

  async function save() {
    if (saving) return;
    saving = true;
    error = null;
    try {
      await saveVar(collection, name, value, target, canMove && move && sourceLayer ? [sourceLayer.scope] : []);
      onclose();
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
    }
  }

  // Position under the variable, kept inside the window.
  const WIDTH = 360;
  const left = $derived(Math.max(8, Math.min(anchor.left, window.innerWidth - WIDTH - 8)));
  const top = $derived(anchor.bottom + 6);

  let root = $state<HTMLDivElement>();
  $effect(() => {
    const down = (e: MouseEvent) => {
      if (root && !root.contains(e.target as Node)) onclose();
    };
    document.addEventListener("mousedown", down, true);
    return () => document.removeEventListener("mousedown", down, true);
  });
</script>

<div
  bind:this={root}
  class="pop"
  role="dialog"
  aria-label={`{{${name}}}`}
  tabindex="-1"
  style:left="{left}px"
  style:top="{top}px"
  style:width="{WIDTH}px"
  onmouseenter={onenter}
  onmouseleave={onleave}
  onfocusin={() => onpin(true)}
  onkeydown={(e) => {
    if (e.key === "Escape") {
      e.stopPropagation();
      onclose();
    }
  }}
>
  <div class="head">
    <code class="name">{`{{${name}}}`}</code>
    {#if info.source}
      {@const src = info.layers.find((l) => l.scope === info.source)}
      <span class="badge ok">{src ? scopeLabel(src) : ""}</span>
    {:else if info.dynamic}
      <span class="badge dyn">{t("urlvar.dynamicBadge")}</span>
    {:else}
      <span class="badge missing">{t("urlvar.undefined")}</span>
    {/if}
  </div>

  {#if info.dynamic}
    <p class="note">{t("urlvar.dynamic")}</p>
  {/if}
  {#if info.fromRuntime}
    <p class="note warn">{t("urlvar.runtime")}</p>
  {/if}

  <form
    onsubmit={(e) => {
      e.preventDefault();
      void save();
    }}
  >
    <label>
      <span>{t("urlvar.value")}</span>
      <input class="mono" bind:value spellcheck="false" autocomplete="off" />
    </label>
    <label>
      <span>{t("urlvar.saveIn")}</span>
      <select bind:value={target}>
        {#each editable as l (l.scope)}
          <option value={l.scope}>{scopeLabel(l)}</option>
        {/each}
      </select>
    </label>
    {#if canMove && sourceLayer}
      <label class="check">
        <input type="checkbox" bind:checked={move} />
        <span>{t("urlvar.move", { scope: scopeLabel(sourceLayer) })}</span>
      </label>
    {/if}
    {#if info.layers.some((l) => l.scope === "environment" && !l.editable)}
      <p class="note">{t("urlvar.legacyEnv")}</p>
    {/if}
    {#each shadowed as l (l.scope)}
      <p class="note muted">{t("urlvar.alsoIn", { scope: scopeLabel(l), value: l.file ?? "" })}</p>
    {/each}
    {#if error}
      <p class="note err">{error}</p>
    {/if}
    <div class="actions">
      <button type="button" class="small" onclick={onclose}>{t("urlvar.cancel")}</button>
      <button type="submit" class="small primary" disabled={saving || !changed}>
        {saving ? t("urlvar.saving") : t("urlvar.save")}
      </button>
    </div>
  </form>
</div>

<style>
  .pop {
    position: fixed;
    z-index: 1000;
    box-sizing: border-box;
    max-width: calc(100vw - 16px);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: var(--shadow);
    padding: 10px 12px;
    font-size: 12px;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 6px;
    min-width: 0;
  }
  .name {
    font-family: var(--mono);
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }
  .badge {
    flex: none;
    margin-left: auto;
    border-radius: 10px;
    padding: 1px 8px;
    font-size: 11px;
  }
  .badge.ok {
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 14%, transparent);
  }
  .badge.dyn {
    color: var(--syntax-keyword);
    background: color-mix(in srgb, var(--syntax-keyword) 14%, transparent);
  }
  .badge.missing {
    color: var(--err);
    background: color-mix(in srgb, var(--err) 12%, transparent);
  }
  form {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 3px;
    color: var(--muted);
  }
  label.check {
    flex-direction: row;
    align-items: center;
    gap: 6px;
    color: var(--text);
  }
  input.mono {
    font-family: var(--mono);
    font-size: 12px;
    color: var(--text);
  }
  select {
    color: var(--text);
  }
  .note {
    margin: 0 0 6px;
    color: var(--text);
    line-height: 1.4;
  }
  form .note {
    margin: 0;
  }
  .note.muted {
    color: var(--muted);
    word-break: break-all;
  }
  .note.warn {
    color: var(--warn);
  }
  .note.err {
    color: var(--err);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 6px;
  }
</style>
