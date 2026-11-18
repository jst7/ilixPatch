<script lang="ts">
  // D60: the secrets are locked (the master password isn't the default one). Shown before the workspace loads.
  import { ask } from "@tauri-apps/plugin-dialog";
  import { t } from "$lib/i18n/index.svelte";
  import { vault } from "$lib/stores/vault.svelte";

  let password = $state("");
  let input: HTMLInputElement | undefined = $state();

  $effect(() => {
    input?.focus();
  });

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    if (!password || vault.busy) return;
    if (!(await vault.unlock(password))) {
      password = "";
      input?.focus();
    }
  }

  async function forgot() {
    const ok = await ask(t("vault.resetConfirm"), {
      title: t("vault.resetTitle"),
      kind: "warning",
      okLabel: t("vault.resetOk"),
      cancelLabel: t("vault.cancel"),
    });
    if (ok) await vault.reset();
  }
</script>

<div class="backdrop">
  <form class="card" onsubmit={submit} aria-labelledby="vault-title">
    <svg viewBox="0 0 24 24" width="28" height="28" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M6 11V8a6 6 0 0112 0v3M5 11h14v10H5zM12 15v2" /></svg>
    <h2 id="vault-title">{t("vault.unlockTitle")}</h2>
    <p class="muted">{t("vault.unlockHint")}</p>
    <input
      bind:this={input}
      type="password"
      autocomplete="current-password"
      placeholder={t("vault.password")}
      aria-label={t("vault.password")}
      aria-invalid={!!vault.error}
      bind:value={password}
    />
    {#if vault.error}<p class="error" role="alert">{t("vault.wrong")}</p>{/if}
    <button class="primary" type="submit" disabled={!password || vault.busy}>{t("vault.unlock")}</button>
    <button class="link" type="button" onclick={forgot}>{t("vault.forgot")}</button>
  </form>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 100;
    display: grid;
    place-items: center;
    background: var(--bg);
  }
  .card {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    width: min(360px, calc(100vw - 32px));
    padding: 24px;
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--surface);
    box-shadow: var(--shadow);
    text-align: center;
  }
  h2 {
    margin: 0;
    font-size: var(--fs-lg);
  }
  p {
    margin: 0;
  }
  .muted {
    color: var(--muted);
    font-size: var(--fs-sm);
  }
  input,
  button.primary {
    width: 100%;
  }
  .error {
    color: var(--err);
    font-size: var(--fs-sm);
  }
  .link {
    border: none;
    background: none;
    color: var(--muted);
    font-size: var(--fs-sm);
    text-decoration: underline;
    padding: 2px;
  }
</style>
