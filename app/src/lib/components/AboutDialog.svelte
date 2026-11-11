<script lang="ts">
  import { getTauriVersion, getVersion } from "@tauri-apps/api/app";
  import CodeEditor from "./CodeEditor.svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { t } from "$lib/i18n/index.svelte";
  import { AUTHOR, GITHUB_URL, GITHUB_USER, LICENSE } from "$lib/about";

  let { open, onclose }: { open: boolean; onclose: () => void } = $props();

  let dialog: HTMLDialogElement | undefined = $state();
  let version = $state<string | null>(null);
  let tauriVersion = $state<string | null>(null);
  let copied = $state(false);
  let licensesOpen = $state(false);
  let licenses = $state<string | null>(null);
  let licensesError = $state(false);

  async function showLicenses() {
    licensesOpen = true;
    if (licenses !== null) return;
    try {
      // Generated at build time by scripts/third-party-licenses.mjs (app/static).
      const res = await fetch("/THIRD_PARTY_LICENSES.txt");
      if (!res.ok) throw new Error(String(res.status));
      licenses = await res.text();
    } catch {
      licensesError = true;
    }
  }

  $effect(() => {
    if (!dialog) return;
    if (open && !dialog.open) {
      dialog.showModal();
      if (version === null) {
        getVersion().then((v) => (version = v), () => (version = "?"));
        getTauriVersion().then((v) => (tauriVersion = v), () => (tauriVersion = null));
      }
    }
    if (!open && dialog.open) dialog.close();
    if (!open) licensesOpen = false;
  });

  const platform = typeof navigator !== "undefined" ? navigator.userAgent : "";

  async function openGithub(e: Event) {
    e.preventDefault();
    try {
      await openUrl(GITHUB_URL);
    } catch {
      window.open(GITHUB_URL, "_blank", "noopener");
    }
  }

  async function copyInfo() {
    const info = `ilixPatch ${version ?? "?"} (Tauri ${tauriVersion ?? "?"})\n${platform}`;
    try {
      await navigator.clipboard.writeText(info);
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch {}
  }
</script>

<dialog
  bind:this={dialog}
  class:wide={licensesOpen}
  onclose={onclose}
  onclick={(e) => e.target === dialog && onclose()}
  aria-labelledby="about-title"
>
  {#if licensesOpen}
    <div class="licenses">
      <header>
        <button class="ghost" onclick={() => (licensesOpen = false)}>← {t("about.back")}</button>
        <h2 id="about-title">{t("about.thirdParty")}</h2>
      </header>
      <p class="hint">{t("about.thirdPartyHint")}</p>
      <div class="viewer">
        {#if licensesError}
          <p class="hint">{t("about.thirdPartyError")}</p>
        {:else if licenses === null}
          <p class="hint">…</p>
        {:else}
          <CodeEditor value={licenses} language="text" readonly />
        {/if}
      </div>
    </div>
  {:else}
  <div class="content">
    <img class="logo" src="/icon.svg" width="72" height="72" alt="" />
    <h2 id="about-title">ilixPatch</h2>
    <p class="tagline">{t("about.tagline")}</p>
    <p class="version">
      {t("about.version")} <strong>{version ?? "…"}</strong>
    </p>

    <dl>
      <dt>{t("about.author")}</dt>
      <dd>{AUTHOR}</dd>
      <dt>GitHub</dt>
      <dd>
        <a href={GITHUB_URL} onclick={openGithub}>
          <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true" fill="currentColor">
            <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z" />
          </svg>
          {GITHUB_USER}
        </a>
      </dd>
      <dt>{t("about.license")}</dt>
      <dd>{LICENSE}</dd>
      {#if tauriVersion}
        <dt>Tauri</dt>
        <dd>{tauriVersion}</dd>
      {/if}
    </dl>

    <p class="copyright">© {new Date().getFullYear()} {AUTHOR} {t("about.andContributors")}</p>
    <p class="credits">
      {t("about.builtWith")}
      <button class="link" onclick={showLicenses}>{t("about.thirdParty")}</button>
    </p>

    <div class="actions">
      <button onclick={copyInfo}>{copied ? t("about.copied") : t("about.copyInfo")}</button>
      <button class="primary" onclick={onclose}>{t("settings.close")}</button>
    </div>
  </div>
  {/if}
</dialog>

<style>
  dialog {
    width: min(380px, calc(100vw - 32px));
    padding: 0;
    border: 1px solid var(--border);
    border-radius: 12px;
    background: var(--surface);
    color: var(--text);
  }
  dialog.wide {
    width: min(900px, calc(100vw - 32px));
  }
  .licenses {
    display: flex;
    flex-direction: column;
    height: min(640px, calc(100vh - 80px));
    padding: 14px 16px 16px;
    box-sizing: border-box;
    gap: 8px;
  }
  .licenses header {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .licenses h2 {
    font-size: 16px;
  }
  .hint {
    margin: 0;
    color: var(--muted);
    font-size: 12px;
  }
  .viewer {
    flex: 1;
    min-height: 0;
    border: 1px solid var(--border);
    border-radius: 8px;
    overflow: hidden;
  }
  .credits {
    margin: 6px 0 0;
    color: var(--muted);
    font-size: 12px;
  }
  button.link {
    border: none;
    background: none;
    padding: 0;
    color: var(--accent);
    font-size: inherit;
    cursor: pointer;
  }
  button.link:hover {
    text-decoration: underline;
  }
  button.ghost {
    background: none;
    border: none;
    color: var(--muted);
    padding: 4px 6px;
  }
  dialog::backdrop {
    background: rgb(0 0 0 / 0.4);
  }
  .content {
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 24px 24px 18px;
    text-align: center;
  }
  .logo {
    margin-bottom: 8px;
  }
  h2 {
    margin: 0;
    font-size: 20px;
  }
  .tagline {
    margin: 4px 0 0;
    color: var(--muted);
    font-size: 13px;
  }
  .version {
    margin: 10px 0 14px;
    font-size: 13px;
  }
  .version strong {
    font-family: ui-monospace, monospace;
  }
  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 6px 14px;
    margin: 0;
    width: 100%;
    padding: 12px 14px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--surface-2);
    text-align: left;
    font-size: 13px;
    box-sizing: border-box;
  }
  dt {
    color: var(--muted);
  }
  dd {
    margin: 0;
    overflow-wrap: anywhere;
  }
  a {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    color: var(--accent);
    text-decoration: none;
  }
  a:hover {
    text-decoration: underline;
  }
  .copyright {
    margin: 12px 0 0;
    color: var(--muted);
    font-size: 12px;
  }
  .actions {
    display: flex;
    gap: 8px;
    margin-top: 16px;
  }
  button {
    font: inherit;
    color: var(--text);
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 6px 12px;
    cursor: pointer;
  }
  button.primary {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--accent-text);
  }
</style>
