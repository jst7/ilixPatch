<script lang="ts">
  // Themed environment picker (D28): "No environment", then Project / Global / Collection (legacy) groups.
  import type { Collection } from "$lib/api";
  import { t } from "$lib/i18n/index.svelte";
  import { envStore, sameChoice, type EnvChoice } from "$lib/stores/environments.svelte";
  import { ws } from "$lib/stores/workspace.svelte";
  import EnvIcon from "./EnvIcon.svelte";

  interface Props {
    /** Project whose environments are offered (null: only global ones). */
    projectPath: string | null;
    /** Collection of the request/target, for legacy collection environments. */
    collection?: Collection | null;
    value: EnvChoice | null;
    onchange: (choice: EnvChoice | null) => void;
    disabled?: boolean;
  }

  let { projectPath, collection = null, value, onchange, disabled = false }: Props = $props();

  const uid = $props.id();
  const listId = `${uid}-list`;
  const optionId = (i: number) => `${uid}-opt-${i}`;

  interface Option {
    choice: EnvChoice | null;
    label: string;
    /** D49 */
    icon?: string | null;
    color?: string | null;
    /** Group heading shown before this option. */
    group?: string;
  }

  const project = $derived(projectPath ? (ws.entries.find((e) => e.path === projectPath)?.project ?? null) : null);

  const options = $derived.by(() => {
    const out: Option[] = [{ choice: null, label: t("request.noEnvironment") }];
    type Env = { name: string; icon?: string | null; color?: string | null };
    const add = (group: string, scope: EnvChoice["scope"], envs: Env[]) =>
      envs.forEach((e, i) =>
        out.push({
          choice: { scope, name: e.name } as EnvChoice,
          label: e.name,
          icon: e.icon,
          color: e.color,
          group: i === 0 ? group : undefined,
        }),
      );
    add(t("env.group.project"), "project", project?.environments ?? []);
    add(t("env.group.global"), "global", envStore.globalEnvs);
    add(t("env.group.collection"), "collection", collection?.environments ?? []);
    return out;
  });

  const currentIndex = $derived(options.findIndex((o) => sameChoice(o.choice, value)));
  const current = $derived(currentIndex >= 0 ? options[currentIndex] : null);

  let open = $state(false);
  let active = $state(0);
  let root = $state<HTMLDivElement>();
  let button = $state<HTMLButtonElement>();
  let list = $state<HTMLUListElement>();

  function show() {
    if (disabled) return;
    active = Math.max(0, currentIndex);
    open = true;
  }

  function hide(refocus = true) {
    open = false;
    if (refocus) button?.focus();
  }

  function choose(i: number) {
    const o = options[i];
    if (o) onchange(o.choice);
    hide();
  }

  function manage() {
    hide(false);
    ws.openEnvironments(projectPath);
  }

  $effect(() => {
    if (!open || !list) return;
    list.focus();
    list.querySelector(`#${CSS.escape(optionId(active))}`)?.scrollIntoView({ block: "nearest" });
  });

  function onbuttonkeydown(e: KeyboardEvent) {
    if (e.key === "ArrowDown" || e.key === "ArrowUp" || e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      show();
    }
  }

  function onlistkeydown(e: KeyboardEvent) {
    const n = options.length;
    if (e.key === "ArrowDown") active = (active + 1) % n;
    else if (e.key === "ArrowUp") active = (active - 1 + n) % n;
    else if (e.key === "Home") active = 0;
    else if (e.key === "End") active = n - 1;
    else if (e.key === "Enter" || e.key === " ") choose(active);
    else if (e.key === "Escape") hide();
    else if (e.key === "Tab") {
      hide(false);
      return;
    } else if (e.key.length === 1 && !e.metaKey && !e.ctrlKey) {
      const l = e.key.toLowerCase();
      for (let k = 1; k <= n; k++) {
        const i = (active + k) % n;
        if (options[i].label.toLowerCase().startsWith(l)) {
          active = i;
          break;
        }
      }
    } else return;
    e.preventDefault();
    e.stopPropagation();
  }

  function scopeLabel(c: EnvChoice | null): string {
    if (!c) return "";
    return c.scope === "project" ? t("env.group.project") : c.scope === "global" ? t("env.group.global") : t("env.group.collection");
  }
</script>

<svelte:window
  onpointerdown={(e) => open && root && !root.contains(e.target as Node) && hide(false)}
  onblur={() => open && hide(false)}
/>

<div class="env-select" bind:this={root}>
  <button
    bind:this={button}
    type="button"
    class="trigger"
    class:none={!value}
    class:missing={!!value && !current}
    {disabled}
    aria-haspopup="listbox"
    aria-expanded={open}
    aria-controls={listId}
    aria-label="{t('request.environment')}: {value?.name ?? t('request.noEnvironment')}"
    title={value ? `${t("request.environment")}: ${value.name} (${scopeLabel(value)})` : t("request.environment")}
    onclick={() => (open ? hide() : show())}
    onkeydown={onbuttonkeydown}
  >
    <EnvIcon size={13} icon={current?.icon} color={current?.color} />
    <span class="label">{value?.name ?? t("request.noEnvironment")}</span>
    {#if value && current}<span class="scope">{scopeLabel(value)}</span>{/if}
    <svg class="caret" viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
      <path d="M4 6l4 4 4-4" fill="none" stroke="currentColor" stroke-width="1.6" />
    </svg>
  </button>

  {#if open}
    <div class="pop">
      <ul
        bind:this={list}
        id={listId}
        class="list"
        role="listbox"
        tabindex="-1"
        aria-label={t("request.environment")}
        aria-activedescendant={optionId(active)}
        onkeydown={onlistkeydown}
      >
        {#each options as o, i (i)}
          {#if o.group}<li class="group" role="presentation">{o.group}</li>{/if}
          <!-- Keyboard interaction is handled on the listbox (aria-activedescendant pattern). -->
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <li
            id={optionId(i)}
            role="option"
            aria-selected={i === currentIndex}
            class:active={i === active}
            class:none={!o.choice}
            onclick={() => choose(i)}
            onpointermove={() => (active = i)}
          >
            {#if o.choice}<EnvIcon size={12} icon={o.icon} color={o.color} />{/if}
            <span class="opt-label">{o.label}</span>
            {#if i === currentIndex}
              <svg class="check" viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
                <path d="M3.5 8.5l3 3 6-7" fill="none" stroke="currentColor" stroke-width="1.8" />
              </svg>
            {/if}
          </li>
        {/each}
      </ul>
      <button type="button" class="manage" onclick={manage}>{t("env.manage")}</button>
    </div>
  {/if}
</div>

<style>
  .env-select {
    position: relative;
    flex: none;
    max-width: 220px;
    min-width: 0;
  }
  .trigger {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--text);
  }
  .trigger :global(svg) {
    flex: none;
    color: var(--muted);
  }
  .trigger.none .label {
    color: var(--muted);
  }
  .trigger.missing .label {
    color: var(--err);
    text-decoration: line-through;
  }
  .trigger:focus-visible,
  .trigger[aria-expanded="true"] {
    outline: none;
    border-color: var(--accent);
  }
  .label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .scope {
    flex: none;
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
    padding: 1px 5px;
    border-radius: 4px;
    background: var(--hover);
  }
  .caret {
    margin-left: auto;
  }
  .pop {
    position: absolute;
    z-index: 40;
    top: calc(100% + 4px);
    right: 0;
    min-width: max(100%, 220px);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: var(--shadow);
    padding: 4px;
    display: flex;
    flex-direction: column;
  }
  .list {
    margin: 0;
    padding: 0;
    list-style: none;
    max-height: 320px;
    overflow: auto;
  }
  .list:focus {
    outline: none;
  }
  .group {
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--muted);
    padding: 8px 8px 2px;
  }
  li[role="option"] {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 5px 8px;
    border-radius: 5px;
    cursor: pointer;
  }
  li.none {
    color: var(--muted);
  }
  li.active {
    background: var(--hover);
  }
  .opt-label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .check {
    flex: none;
    color: var(--accent);
  }
  .manage {
    margin-top: 4px;
    border: none;
    border-top: 1px solid var(--border);
    border-radius: 0 0 5px 5px;
    background: none;
    text-align: left;
    padding: 6px 8px;
    font-size: 12px;
    color: var(--accent);
  }
  .manage:hover {
    background: var(--hover);
  }
</style>
