<script lang="ts">
  // D65: command palette (⌘K / Ctrl+K): go to any request or run any action from one search box.
  // Shortcuts are shown next to the actions, so they are learned along the way.
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { t } from "$lib/i18n/index.svelte";
  import { ws } from "$lib/stores/workspace.svelte";
  import { importer } from "$lib/stores/import.svelte";
  import { netlog } from "$lib/stores/netlog.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { exportProjectToPostman } from "$lib/export/postman";
  import { methodColor } from "./methods";

  interface Props {
    onclose: () => void;
  }
  let { onclose }: Props = $props();

  interface Item {
    kind: "action" | "request";
    label: string;
    hint?: string;
    keys?: string;
    method?: string;
    run: () => void;
  }

  const mod = typeof navigator !== "undefined" && /Mac/.test(navigator.platform) ? "⌘" : "Ctrl+";
  let query = $state("");
  let index = $state(0);
  let input: HTMLInputElement | undefined = $state();
  let list: HTMLUListElement | undefined = $state();

  $effect(() => {
    input?.focus();
  });

  const items = $derived.by((): Item[] => {
    const out: Item[] = [
      { kind: "action", label: t("palette.newRequest"), keys: `${mod}N`, run: () => ws.newScratch() },
      ...(ws.selectedPath
        ? [
            { kind: "action" as const, label: t("palette.send"), keys: `${mod}↵`, run: () => ws.send() },
            { kind: "action" as const, label: t("palette.save"), keys: `${mod}S`, run: () => void ws.save() },
          ]
        : []),
      { kind: "action", label: t("palette.import"), run: () => importer.open() },
      {
        kind: "action",
        label: t("palette.addProject"),
        run: () =>
          void openDialog({ directory: true, multiple: false }).then((d) => {
            if (typeof d === "string") void ws.addProject(d);
          }),
      },
      { kind: "action", label: t("palette.globalEnvs"), run: () => ws.openTab({ kind: "environments", project: null }) },
      { kind: "action", label: t("palette.netlog"), keys: `${mod}⌥C`, run: () => netlog.toggle() },
      { kind: "action", label: t("palette.settings"), run: () => ui.openSettings() },
      { kind: "action", label: t("palette.password"), run: () => ui.openSettings("security") },
    ];
    for (const e of ws.entries) {
      const p = e.project;
      if (!p) continue;
      out.push(
        { kind: "action", label: t("palette.projectEnvs", { name: p.name }), run: () => ws.openTab({ kind: "environments", project: e.path }) },
        { kind: "action", label: t("palette.mock", { name: p.name }), run: () => ws.openTab({ kind: "mock", target: e.path }) },
        { kind: "action", label: t("palette.export", { name: p.name }), run: () => void exportProjectToPostman(e.path) },
      );
      for (const c of p.collections) {
        out.push(
          { kind: "action", label: t("palette.run", { name: c.name }), hint: p.name, run: () => ws.openTab({ kind: "runner", target: c.path }) },
          { kind: "action", label: t("palette.load", { name: c.name }), hint: p.name, run: () => ws.openTab({ kind: "load", target: c.path }) },
        );
      }
    }
    for (const [path, loc] of ws.index) {
      const folders = loc.folders.map((f) => f.name).join(" / ");
      out.push({
        kind: "request",
        label: loc.file.request.name,
        hint: [loc.collection.name, folders].filter(Boolean).join(" / "),
        method: loc.file.request.method,
        run: () => ws.select(path),
      });
    }
    return out;
  });

  const shown = $derived.by(() => {
    const terms = query.toLowerCase().split(/\s+/).filter(Boolean);
    const match = (i: Item) => {
      const hay = `${i.label} ${i.hint ?? ""} ${i.method ?? ""}`.toLowerCase();
      return terms.every((x) => hay.includes(x));
    };
    // Without a query, actions first; with one, requests and actions mixed, label matches first.
    const res = items.filter(match);
    if (terms.length) res.sort((a, b) => Number(!a.label.toLowerCase().includes(terms[0])) - Number(!b.label.toLowerCase().includes(terms[0])));
    return res.slice(0, 80);
  });

  $effect(() => {
    void query;
    index = 0;
  });

  function run(i: Item | undefined) {
    if (!i) return;
    onclose();
    i.run();
  }

  function onkey(e: KeyboardEvent) {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const n = shown.length;
      if (!n) return;
      index = (index + (e.key === "ArrowDown" ? 1 : -1) + n) % n;
      list?.children[index]?.scrollIntoView({ block: "nearest" });
    } else if (e.key === "Enter") {
      e.preventDefault();
      run(shown[index]);
    } else if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    }
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="backdrop" onclick={onclose}>
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="palette" role="dialog" tabindex="-1" aria-label={t("palette.title")} onclick={(e) => e.stopPropagation()}>
    <input
      bind:this={input}
      bind:value={query}
      onkeydown={onkey}
      placeholder={t("palette.placeholder")}
      aria-label={t("palette.placeholder")}
      role="combobox"
      aria-expanded="true"
      aria-controls="palette-list"
    />
    <ul id="palette-list" bind:this={list} role="listbox">
      {#each shown as item, i (item.kind + item.label + (item.hint ?? "") + i)}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <li role="option" aria-selected={i === index} class:active={i === index} onclick={() => run(item)} onmousemove={() => (index = i)}>
          {#if item.kind === "request"}
            <span class="method" style:color={methodColor(item.method ?? "GET")}>{item.method}</span>
          {:else}
            <span class="method action">›</span>
          {/if}
          <span class="label">{item.label}</span>
          {#if item.hint}<span class="hint">{item.hint}</span>{/if}
          {#if item.keys}<kbd>{item.keys}</kbd>{/if}
        </li>
      {:else}
        <li class="empty">{t("palette.empty")}</li>
      {/each}
    </ul>
    <div class="foot">{t("palette.foot")}</div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 90;
    background: color-mix(in srgb, var(--bg) 40%, transparent);
    display: flex;
    justify-content: center;
    align-items: flex-start;
    padding-top: 12vh;
  }
  .palette {
    width: min(620px, calc(100vw - 32px));
    max-height: 70vh;
    display: flex;
    flex-direction: column;
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--surface);
    box-shadow: var(--shadow);
    overflow: hidden;
  }
  input {
    border: none;
    border-bottom: 1px solid var(--border);
    border-radius: 0;
    padding: 12px 14px;
    font-size: var(--fs-lg);
    background: transparent;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 4px;
    overflow: auto;
  }
  li {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 10px;
    border-radius: 6px;
    cursor: pointer;
    white-space: nowrap;
  }
  li.active {
    background: var(--selected);
  }
  .method {
    flex: none;
    width: 52px;
    font-family: var(--mono);
    font-size: var(--fs-xs);
    font-weight: 700;
  }
  .method.action {
    color: var(--accent);
    text-align: center;
  }
  .label {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .hint {
    color: var(--muted);
    font-size: var(--fs-sm);
    overflow: hidden;
    text-overflow: ellipsis;
    flex: 1;
  }
  kbd {
    margin-left: auto;
    font-family: var(--mono);
    font-size: var(--fs-xs);
    color: var(--muted);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 0 5px;
  }
  .empty {
    color: var(--muted);
    cursor: default;
  }
  .foot {
    border-top: 1px solid var(--border);
    padding: 5px 12px;
    font-size: var(--fs-xs);
    color: var(--muted);
  }
</style>
