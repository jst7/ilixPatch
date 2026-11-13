<script lang="ts">
  import { t, i18n, LOCALES, type MessageKey } from "$lib/i18n/index.svelte";
  import { themes, TOKEN_GROUPS, type Token } from "$lib/theme/index.svelte";
  import type { HistorySettings } from "$lib/api";
  import { history } from "$lib/stores/history.svelte";
  import { updates } from "$lib/stores/update.svelte";
  import { clearHistory, formatBytes } from "./historyUtil";
  import NetworkSettingsSection from "./NetworkSettingsSection.svelte";

  type SectionId = "general" | "appearance" | "colors" | "history" | "https" | "proxy" | "requests";
  let {
    open,
    onclose,
    section: initialSection,
  }: { open: boolean; onclose: () => void; section?: SectionId } = $props();

  /** Vertical tabs, one per concept. Add new sections here. */
  const SECTIONS: { id: SectionId; label: MessageKey; icon: string }[] = [
    { id: "general", label: "settings.section.general", icon: "M12 15a3 3 0 100-6 3 3 0 000 6zM19.4 15l1.6 1-2 3.4-1.8-.7a8 8 0 01-2.2 1.3L14.7 22h-4l-.3-2a8 8 0 01-2.2-1.3l-1.8.7-2-3.4 1.6-1a8 8 0 010-2.6L4.4 11l2-3.4 1.8.7A8 8 0 0110.4 7l.3-2h4l.3 2a8 8 0 012.2 1.3l1.8-.7 2 3.4-1.6 1a8 8 0 010 2.6z" },
    { id: "appearance", label: "settings.appearance", icon: "M12 3a9 9 0 100 18c.8 0 1.5-.7 1.5-1.5 0-.4-.2-.7-.4-1-.2-.3-.4-.6-.4-1 0-.8.7-1.5 1.5-1.5H16a5 5 0 005-5c0-4.4-4-8-9-8zM7.5 12a1.5 1.5 0 110-3 1.5 1.5 0 010 3zm3-4a1.5 1.5 0 110-3 1.5 1.5 0 010 3zm5 0a1.5 1.5 0 110-3 1.5 1.5 0 010 3z" },
    { id: "colors", label: "settings.section.colors", icon: "M4 20h4L18.5 9.5a2.8 2.8 0 00-4-4L4 16v4zM13.5 6.5l4 4" },
    { id: "history", label: "settings.history.title", icon: "M3 12a9 9 0 109-9 9.7 9.7 0 00-6.7 2.8L3 8M3 3v5h5M12 7v5l4 2" },
    { id: "requests", label: "settings.section.requests", icon: "M4 12h13M13 6l6 6-6 6" },
    { id: "https", label: "settings.section.https", icon: "M6 11V8a6 6 0 0112 0v3M5 11h14v10H5zM12 15v2" },
    { id: "proxy", label: "settings.section.proxy", icon: "M12 3a9 9 0 100 18 9 9 0 000-18zM3 12h18M12 3c2.5 2.5 3.5 5.5 3.5 9s-1 6.5-3.5 9c-2.5-2.5-3.5-5.5-3.5-9s1-6.5 3.5-9z" },
  ];
  const LS_SECTION = "ilixpatch.settingsSection";
  let section = $state<SectionId>(
    (() => {
      try {
        const v = localStorage.getItem(LS_SECTION) as SectionId | null;
        if (v && SECTIONS.some((x) => x.id === v)) return v;
      } catch {}
      return "general";
    })(),
  );
  function selectSection(id: SectionId) {
    section = id;
    try {
      localStorage.setItem(LS_SECTION, id);
    } catch {}
  }
  $effect(() => {
    if (open && initialSection) section = initialSection;
  });
  function navKey(e: KeyboardEvent) {
    const i = SECTIONS.findIndex((x) => x.id === section);
    let next = -1;
    if (e.key === "ArrowDown") next = (i + 1) % SECTIONS.length;
    else if (e.key === "ArrowUp") next = (i - 1 + SECTIONS.length) % SECTIONS.length;
    else if (e.key === "Home") next = 0;
    else if (e.key === "End") next = SECTIONS.length - 1;
    if (next < 0) return;
    e.preventDefault();
    selectSection(SECTIONS[next].id);
    (e.currentTarget as HTMLElement).querySelectorAll<HTMLButtonElement>("[role=tab]")[next]?.focus();
  }

  let dialog: HTMLDialogElement | undefined = $state();
  let transfer = $state(""); // JSON textarea for export/import
  let transferMode = $state<"none" | "export" | "import">("none");
  let importError = $state("");

  $effect(() => {
    if (!dialog) return;
    if (open && !dialog.open) dialog.showModal();
    if (!open && dialog.open) dialog.close();
  });

  // ----- History (D22) -----
  const DAY_PRESETS = [1, 7, 30, 90, 365];
  const MB_PRESETS = [50, 100, 200, 500, 1000];

  /** Editable copy of the backend settings (null until loaded / when unavailable). */
  let hs = $state<HistorySettings | null>(null);
  let daysCustom = $state(false);
  let mbCustom = $state(false);
  let historyError = $state<string | null>(null);
  let saveTimer: ReturnType<typeof setTimeout> | undefined;

  $effect(() => {
    if (!open) return;
    historyError = null;
    void history.loadSettings().then(() => {
      hs = history.settings ? { ...history.settings } : null;
      daysCustom = !!hs && hs.max_days !== null && !DAY_PRESETS.includes(hs.max_days);
      mbCustom = !!hs && hs.max_size_mb !== null && !MB_PRESETS.includes(hs.max_size_mb);
    });
    return () => clearTimeout(saveTimer);
  });

  function changed() {
    clearTimeout(saveTimer);
    saveTimer = setTimeout(async () => {
      if (!hs) return;
      historyError = await history.saveSettings({ ...hs });
    }, 400);
  }

  function presetValue(value: number | null, presets: number[], custom: boolean): string {
    if (value === null) return "none";
    return custom || !presets.includes(value) ? "custom" : String(value);
  }

  function onDays(v: string) {
    if (!hs) return;
    daysCustom = v === "custom";
    if (v === "none") hs.max_days = null;
    else if (v !== "custom") hs.max_days = Number(v);
    else if (hs.max_days === null) hs.max_days = 14;
    changed();
  }

  function onMb(v: string) {
    if (!hs) return;
    mbCustom = v === "custom";
    if (v === "none") hs.max_size_mb = null;
    else if (v !== "custom") hs.max_size_mb = Number(v);
    else if (hs.max_size_mb === null) hs.max_size_mb = 250;
    changed();
  }

  /** Positive integer from an input, or null when invalid. */
  function positive(v: string): number | null {
    const n = Math.round(Number(v));
    return Number.isFinite(n) && n > 0 ? n : null;
  }

  const statsLine = $derived.by(() => {
    const st = history.stats;
    if (!st) return "";
    const line = t("settings.history.stats", {
      entries: new Intl.NumberFormat(i18n.locale).format(st.entries),
      size: formatBytes(st.size_bytes),
    });
    if (st.oldest === null) return line;
    const since = new Intl.DateTimeFormat(i18n.locale, { day: "numeric", month: "short", year: "numeric" }).format(st.oldest);
    return `${line} · ${t("settings.history.since", { date: since })}`;
  });

  const active = $derived(themes.active);
  const editable = $derived(!active.builtin);

  function themeLabel(id: string, name: string, builtin?: boolean) {
    return builtin ? t(`theme.${id}` as MessageKey) : name;
  }

  function duplicate() {
    const name = t("settings.theme.copyOf", { name: themeLabel(active.id, active.name, active.builtin) });
    themes.duplicate(active.id, name);
  }

  function setColor(token: Token, value: string) {
    themes.update(active.id, { colors: { [token]: value } });
  }

  /** <input type=color> only accepts #rrggbb. */
  function asHex(v: string) {
    return /^#[0-9a-f]{6}$/i.test(v) ? v : "#000000";
  }

  function showExport() {
    transfer = themes.export(active.id);
    transferMode = "export";
  }

  function doImport() {
    try {
      themes.import(transfer);
      transferMode = "none";
      transfer = "";
      importError = "";
    } catch {
      importError = t("settings.theme.importError");
    }
  }
</script>

<dialog bind:this={dialog} onclose={onclose} onclick={(e) => e.target === dialog && onclose()} aria-labelledby="settings-title">
  <div class="layout">
    <nav aria-label={t("settings.title")}>
      <h2 id="settings-title">{t("settings.title")}</h2>
      <!-- svelte-ignore a11y_interactive_supports_focus -->
      <div role="tablist" aria-orientation="vertical" tabindex="-1" onkeydown={navKey}>
        {#each SECTIONS as s (s.id)}
          <button
            role="tab"
            id="settings-tab-{s.id}"
            aria-selected={section === s.id}
            aria-controls="settings-panel"
            tabindex={section === s.id ? 0 : -1}
            class:active={section === s.id}
            onclick={() => selectSection(s.id)}
          >
            <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d={s.icon} /></svg>
            {t(s.label)}
          </button>
        {/each}
      </div>
    </nav>

  <div class="content" id="settings-panel" role="tabpanel" aria-labelledby="settings-tab-{section}">
    <header>
      <h3>{t(SECTIONS.find((x) => x.id === section)!.label)}</h3>
      <button class="ghost" onclick={onclose} aria-label={t("settings.close")}>✕</button>
    </header>

    {#if section === "general"}
    <section>
      <label class="row">
        <span>{t("settings.language")}</span>
        <select value={i18n.locale} onchange={(e) => (i18n.locale = e.currentTarget.value)}>
          {#each LOCALES as l (l.code)}<option value={l.code}>{l.label}</option>{/each}
        </select>
      </label>
      <label class="row">
        <span>{t("settings.updates.auto")}</span>
        <input class="toggle" type="checkbox" checked={updates.auto} onchange={(e) => updates.setAuto(e.currentTarget.checked)} />
      </label>
      <p class="hint">{t("settings.updates.hint")}</p>
    </section>
    {:else if section === "history"}
    <section>
      <p class="hint">{t("settings.history.localOnly")}</p>
      {#if hs}
        <label class="row">
          <span>{t("settings.history.enabled")}</span>
          <input class="toggle" type="checkbox" bind:checked={hs.enabled} onchange={changed} />
        </label>
        <fieldset class="plain" disabled={!hs.enabled}>
          <label class="row">
            <span>{t("settings.history.retention")}</span>
            <span class="inline">
              <select value={presetValue(hs.max_days, DAY_PRESETS, daysCustom)} onchange={(e) => onDays(e.currentTarget.value)}>
                {#each DAY_PRESETS as d (d)}<option value={String(d)}>{t("settings.history.days", { n: d })}</option>{/each}
                <option value="none">{t("settings.history.forever")}</option>
                <option value="custom">{t("settings.history.custom")}</option>
              </select>
              {#if daysCustom && hs.max_days !== null}
                <input
                  class="num"
                  type="number"
                  min="1"
                  value={hs.max_days}
                  aria-label={t("settings.history.retention")}
                  onchange={(e) => {
                    const n = positive(e.currentTarget.value);
                    if (hs && n !== null) {
                      hs.max_days = n;
                      changed();
                    } else e.currentTarget.value = String(hs?.max_days ?? "");
                  }}
                />
                <span class="unit">{t("settings.history.daysUnit")}</span>
              {/if}
            </span>
          </label>
          <label class="row">
            <span>{t("settings.history.maxSize")}</span>
            <span class="inline">
              <select value={presetValue(hs.max_size_mb, MB_PRESETS, mbCustom)} onchange={(e) => onMb(e.currentTarget.value)}>
                {#each MB_PRESETS as m (m)}<option value={String(m)}>{m} MB</option>{/each}
                <option value="none">{t("settings.history.unlimited")}</option>
                <option value="custom">{t("settings.history.custom")}</option>
              </select>
              {#if mbCustom && hs.max_size_mb !== null}
                <input
                  class="num"
                  type="number"
                  min="1"
                  value={hs.max_size_mb}
                  aria-label={t("settings.history.maxSize")}
                  onchange={(e) => {
                    const n = positive(e.currentTarget.value);
                    if (hs && n !== null) {
                      hs.max_size_mb = n;
                      changed();
                    } else e.currentTarget.value = String(hs?.max_size_mb ?? "");
                  }}
                />
                <span class="unit">MB</span>
              {/if}
            </span>
          </label>
          <label class="row">
            <span>{t("settings.history.saveBodies")}</span>
            <input class="toggle" type="checkbox" bind:checked={hs.save_response_body} onchange={changed} />
          </label>
          <label class="row">
            <span>{t("settings.history.maxBody")}</span>
            <span class="inline">
              <input
                class="num"
                type="number"
                min="1"
                disabled={!hs.save_response_body}
                value={hs.max_body_kb}
                onchange={(e) => {
                  const n = positive(e.currentTarget.value);
                  if (hs && n !== null) {
                    hs.max_body_kb = n;
                    changed();
                  } else e.currentTarget.value = String(hs?.max_body_kb ?? "");
                }}
              />
              <span class="unit">KB</span>
            </span>
          </label>
        </fieldset>
      {:else}
        <p class="hint error">{t("settings.history.unavailable")}</p>
      {/if}
      <div class="actions">
        {#if statsLine}<span class="hint">{statsLine}</span>{/if}
        <button class="danger" disabled={!history.stats?.entries} onclick={() => clearHistory()}>
          {t("settings.history.clear")}
        </button>
      </div>
      {#if historyError}<p class="hint error">{historyError}</p>{/if}
    </section>
    {:else if section === "appearance"}
    <section>
      <label class="row">
        <span>{t("settings.theme")}</span>
        <select value={themes.selected} onchange={(e) => themes.select(e.currentTarget.value)}>
          <option value="system">{t("settings.theme.system")}</option>
          {#each themes.all as th (th.id)}
            <option value={th.id}>{themeLabel(th.id, th.name, th.builtin)}</option>
          {/each}
        </select>
      </label>

      <div class="actions">
        <button onclick={duplicate}>{t("settings.theme.duplicate")}</button>
        <button onclick={showExport}>{t("settings.theme.export")}</button>
        <button onclick={() => ((transferMode = "import"), (transfer = ""), (importError = ""))}>
          {t("settings.theme.import")}
        </button>
        {#if editable}
          <button onclick={() => themes.reset(active.id)}>{t("settings.theme.reset")}</button>
          <button class="danger" onclick={() => themes.remove(active.id)}>{t("settings.theme.delete")}</button>
        {/if}
      </div>

      {#if transferMode !== "none"}
        <textarea bind:value={transfer} readonly={transferMode === "export"} spellcheck="false" rows="8"></textarea>
        {#if transferMode === "import"}
          <div class="actions">
            <button class="primary" onclick={doImport}>{t("settings.theme.import")}</button>
            {#if importError}<span class="error">{importError}</span>{/if}
          </div>
        {/if}
      {/if}

      {#if editable}
        <label class="row">
          <span>{t("settings.theme.name")}</span>
          <input value={active.name} oninput={(e) => themes.update(active.id, { name: e.currentTarget.value })} />
        </label>
        <label class="row">
          <span>{t("settings.theme")}</span>
          <select value={active.base} onchange={(e) => themes.update(active.id, { base: e.currentTarget.value as "light" | "dark" })}>
            <option value="light">{t("theme.light")}</option>
            <option value="dark">{t("theme.dark")}</option>
          </select>
        </label>
      {:else}
        <p class="hint">{t("settings.theme.builtinHint")}</p>
      {/if}
      <p class="hint">
        <button class="link" onclick={() => selectSection("colors")}>{t("settings.theme.customize")} →</button>
      </p>
    </section>
    {:else if section === "colors"}
    <section>
      <p class="hint">{t("settings.colors.editing", { name: themeLabel(active.id, active.name, active.builtin) })}</p>
      {#if !editable}
        <div class="notice">
          <span>{t("settings.theme.builtinHint")}</span>
          <button class="primary" onclick={duplicate}>{t("settings.theme.duplicate")}</button>
        </div>
      {/if}
      {#each TOKEN_GROUPS as group (group.id)}
        <fieldset disabled={!editable}>
          <legend>{t(`settings.theme.group.${group.id}`)}</legend>
          {#each group.tokens as token (token)}
            <label class="color">
              <input
                type="color"
                value={asHex(active.colors[token])}
                oninput={(e) => setColor(token, e.currentTarget.value)}
              />
              <span>{t(`token.${token}`)}</span>
              <input
                class="hex"
                value={active.colors[token]}
                onchange={(e) => setColor(token, e.currentTarget.value.trim())}
                spellcheck="false"
              />
            </label>
          {/each}
        </fieldset>
      {/each}
    </section>
    {:else if section === "https" || section === "proxy" || section === "requests"}
    <NetworkSettingsSection part={section} />
    {/if}
  </div>
  </div>
</dialog>

<style>
  dialog {
    width: min(860px, calc(100vw - 32px));
    height: min(620px, calc(100vh - 64px));
    max-height: calc(100vh - 64px);
    padding: 0;
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--surface);
    color: var(--text);
  }
  dialog::backdrop {
    background: rgb(0 0 0 / 0.4);
  }
  .layout {
    display: grid;
    grid-template-columns: 200px 1fr;
    height: 100%;
  }
  nav {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 16px 10px;
    background: var(--surface-2);
    border-right: 1px solid var(--border);
    overflow-y: auto;
  }
  nav h2 {
    font-size: 15px;
    padding: 0 8px;
  }
  [role="tablist"] {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  [role="tab"] {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 7px 10px;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--muted);
    text-align: left;
    cursor: pointer;
  }
  [role="tab"]:hover {
    color: var(--text);
    background: var(--surface);
  }
  [role="tab"].active {
    color: var(--text);
    background: var(--selection);
    box-shadow: inset 3px 0 0 var(--accent);
  }
  .content {
    padding: 16px 20px 20px;
    display: flex;
    flex-direction: column;
    gap: 16px;
    overflow-y: auto;
    min-width: 0;
  }
  .notice {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 10px 12px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--surface-2);
    font-size: 13px;
  }
  button.link {
    border: none;
    background: none;
    padding: 0;
    color: var(--accent);
    cursor: pointer;
  }
  @media (max-width: 620px) {
    .layout {
      grid-template-columns: 1fr;
      grid-template-rows: auto 1fr;
    }
    nav {
      border-right: none;
      border-bottom: 1px solid var(--border);
    }
    [role="tablist"] {
      flex-direction: row;
      overflow-x: auto;
    }
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  h2,
  h3 {
    margin: 0;
  }
  h3 {
    font-size: 15px;
  }
  section {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .row {
    display: grid;
    grid-template-columns: 160px 1fr;
    align-items: center;
    gap: 12px;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: center;
  }
  select,
  input,
  textarea,
  button {
    font: inherit;
    color: var(--text);
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 6px 10px;
  }
  button {
    cursor: pointer;
  }
  button.primary {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--accent-text);
  }
  button.danger {
    color: var(--err);
  }
  button.ghost {
    background: none;
    border: none;
    color: var(--muted);
  }
  textarea {
    font-family: ui-monospace, monospace;
    font-size: 12px;
    resize: vertical;
  }
  fieldset {
    border: 1px solid var(--border);
    border-radius: 8px;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
    gap: 6px 16px;
    padding: 8px 12px 12px;
  }
  fieldset:disabled {
    opacity: 0.7;
  }
  legend {
    color: var(--muted);
    font-size: 12px;
    padding: 0 4px;
  }
  .color {
    display: grid;
    grid-template-columns: 28px 1fr 90px;
    align-items: center;
    gap: 8px;
    font-size: 13px;
  }
  .color input[type="color"] {
    width: 28px;
    height: 24px;
    padding: 0;
    border: 1px solid var(--border);
    background: none;
  }
  .hex {
    font-family: ui-monospace, monospace;
    font-size: 12px;
    padding: 3px 6px;
  }
  .hint {
    color: var(--muted);
    font-size: 13px;
    margin: 0;
  }
  .error {
    color: var(--err);
  }
  fieldset.plain {
    border: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .inline {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .num {
    width: 90px;
  }
  .unit {
    color: var(--muted);
    font-size: 13px;
  }
  input.toggle {
    justify-self: start;
    width: 16px;
    height: 16px;
    padding: 0;
    accent-color: var(--accent);
  }
</style>
