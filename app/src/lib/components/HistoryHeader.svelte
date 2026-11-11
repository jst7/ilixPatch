<script lang="ts">
  // Header of a history tab inside RequestEditor: name, origin and "Save to collection…" with a folder picker.
  import type { Folder } from "$lib/api";
  import { t } from "$lib/i18n/index.svelte";
  import { historyKey, ws } from "$lib/stores/workspace.svelte";
  import { dateTime } from "./historyUtil";

  let { id }: { id: number } = $props();

  const key = $derived(historyKey(id));
  const draft = $derived(ws.drafts[key]);
  const st = $derived(ws.historyTabs[id]);
  const dirty = $derived(ws.isHistoryDirty(id));

  interface Target {
    path: string;
    projectPath: string;
    name: string;
    depth: number;
    kind: "project" | "collection" | "folder";
  }

  /** Projects (headings), collections and folders, in tree order. */
  const targets = $derived.by(() => {
    const out: Target[] = [];
    for (const entry of ws.entries) {
      const project = entry.project;
      if (!project) continue;
      out.push({ path: entry.path, projectPath: entry.path, name: project.name, depth: 0, kind: "project" });
      for (const c of project.collections) {
        out.push({ path: c.path, projectPath: entry.path, name: c.name, depth: 1, kind: "collection" });
        const walk = (f: Folder, depth: number) => {
          for (const sub of f.folders) {
            out.push({ path: sub.path, projectPath: entry.path, name: sub.name, depth, kind: "folder" });
            walk(sub, depth + 1);
          }
        };
        walk(c.root, 2);
      }
    }
    return out;
  });

  let open = $state(false);
  let target = $state<string | null>(null);
  let name = $state("");
  let saving = $state(false);
  let root = $state<HTMLDivElement>();

  function defaultTarget(): string | null {
    const p = st?.entry?.request_path;
    const loc = p ? ws.index.get(p) : undefined;
    if (loc) return loc.folders.at(-1)?.path ?? loc.collection.path;
    return targets.find((x) => x.kind !== "project")?.path ?? null;
  }

  function show() {
    if (!draft) return;
    target = defaultTarget();
    name = draft.name || st?.entry?.name || "";
    open = true;
  }

  async function save() {
    const tgt = targets.find((x) => x.path === target && x.kind !== "project");
    if (!tgt || !name.trim() || saving) return;
    saving = true;
    const ok = await ws.saveHistoryAs(id, tgt.path, tgt.projectPath, name);
    saving = false;
    if (ok) open = false;
  }

  // Cmd/Ctrl+S on a history tab asks for the picker.
  $effect(() => {
    if (ws.historySaveRequest !== id) return;
    ws.historySaveRequest = null;
    show();
  });

  function onwindowpointerdown(e: PointerEvent) {
    if (open && root && !root.contains(e.target as Node)) open = false;
  }

  function focusOnMount(el: HTMLElement) {
    el.focus();
    if (el instanceof HTMLInputElement) el.select();
  }
</script>

<svelte:window onpointerdown={onwindowpointerdown} />

{#if draft}
  <div class="head">
    <span class="icon" title={st?.entry ? dateTime(st.entry.timestamp) : ""}>
      <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.4">
        <circle cx="8" cy="8" r="6" /><path d="M8 4.5V8l2.5 1.5" />
      </svg>
    </span>
    <input
      class="name"
      bind:value={draft.name}
      placeholder={t("request.namePlaceholder")}
      aria-label={t("request.namePlaceholder")}
      spellcheck="false"
    />
    {#if dirty}<span class="dirty-dot" title={t("history.edited")}></span>{/if}
    <div class="save" bind:this={root}>
      <button
        class="small"
        aria-haspopup="dialog"
        aria-expanded={open}
        title={t("history.saveHint")}
        onclick={() => (open ? (open = false) : show())}>{t("history.saveToCollection")}</button
      >
      {#if open}
        <div
          class="popover"
          role="dialog"
          aria-label={t("history.saveToCollection")}
          tabindex="-1"
          onkeydown={(e) => {
            if (e.key === "Escape") {
              e.stopPropagation();
              open = false;
            }
          }}
        >
          <label class="field">
            <span>{t("history.saveName")}</span>
            <input
              use:focusOnMount
              bind:value={name}
              spellcheck="false"
              onkeydown={(e) => e.key === "Enter" && save()}
            />
          </label>
          <span class="label">{t("history.saveLocation")}</span>
          <div class="picker" role="listbox" aria-label={t("history.saveLocation")}>
            {#each targets as x (x.path)}
              {#if x.kind === "project"}
                <div class="proj" role="presentation">{x.name}</div>
              {:else}
                <div
                  class="opt"
                  class:active={target === x.path}
                  role="option"
                  aria-selected={target === x.path}
                  tabindex="0"
                  style:padding-left="{8 + (x.depth - 1) * 14}px"
                  onclick={() => (target = x.path)}
                  ondblclick={save}
                  onkeydown={(e) => {
                    if (e.key === "Enter" || e.key === " ") {
                      e.preventDefault();
                      target = x.path;
                    }
                  }}
                >
                  <svg viewBox="0 0 16 16" width="13" height="13" aria-hidden="true">
                    {#if x.kind === "collection"}
                      <path d="M3 3h10v3H3zM3 7.5h10v3H3zM3 12h10" fill="none" stroke="currentColor" stroke-width="1.3" />
                    {:else}
                      <path d="M1.5 4.5a1 1 0 011-1h3.5l1.5 1.5h6a1 1 0 011 1v6.5a1 1 0 01-1 1h-11a1 1 0 01-1-1z" fill="none" stroke="currentColor" stroke-width="1.3" />
                    {/if}
                  </svg>
                  <span>{x.name}</span>
                </div>
              {/if}
            {:else}
              <p class="hint">{t("history.noCollections")}</p>
            {/each}
          </div>
          <div class="actions">
            <button class="ghost small" onclick={() => (open = false)}>{t("tabs.cancel")}</button>
            <button class="primary small" disabled={!target || !name.trim() || saving} onclick={save}>
              {t("request.save")}
            </button>
          </div>
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .head {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .icon {
    display: flex;
    color: var(--muted);
  }
  .name {
    flex: 1 1 140px;
    min-width: 80px;
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
  .small {
    padding: 4px 10px;
    font-size: 12px;
  }
  .save {
    position: relative;
    flex: none;
  }
  .popover {
    position: absolute;
    right: 0;
    top: calc(100% + 4px);
    z-index: 40;
    width: 300px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: var(--shadow);
  }
  .popover:focus {
    outline: none;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .field span,
  .label {
    font-size: 12px;
    color: var(--muted);
  }
  .field input {
    padding: 4px 8px;
  }
  .picker {
    max-height: 240px;
    overflow: auto;
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 4px 0;
    font-size: 13px;
  }
  .proj {
    font-size: 11px;
    font-weight: 600;
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    padding: 6px 8px 2px;
  }
  .opt {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 3px 8px;
    margin: 0 4px;
    border-radius: 4px;
    cursor: pointer;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .opt svg {
    flex: none;
    color: var(--muted);
  }
  .opt:hover {
    background: var(--hover);
  }
  .opt.active {
    background: var(--selected);
  }
  .opt:focus-visible {
    outline: 1px solid var(--accent);
  }
  .hint {
    color: var(--muted);
    font-size: 12px;
    margin: 6px 8px;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 6px;
  }
</style>
