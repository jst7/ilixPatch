<script lang="ts">
  // Pick the icon and colour of an environment (D49). Theme colours follow light/dark themes;
  // "Custom" stores a #rrggbb.
  import { t } from "$lib/i18n/index.svelte";
  import { ENV_COLORS, ENV_ICON_IDS, envColor } from "$lib/env/style";
  import EnvIcon from "./EnvIcon.svelte";

  let { icon = $bindable(), color = $bindable() }: { icon: string | null; color: string | null } = $props();

  let open = $state(false);
  let root = $state<HTMLDivElement>();

  $effect(() => {
    if (!open) return;
    const down = (e: MouseEvent) => {
      if (root && !root.contains(e.target as Node)) open = false;
    };
    const key = (e: KeyboardEvent) => e.key === "Escape" && (open = false);
    document.addEventListener("mousedown", down, true);
    document.addEventListener("keydown", key, true);
    return () => {
      document.removeEventListener("mousedown", down, true);
      document.removeEventListener("keydown", key, true);
    };
  });

  /** A #rrggbb for the colour input (theme tokens are resolved from the page). */
  function hexOf(c: string | null): string {
    if (c && c.startsWith("#") && c.length === 7) return c;
    if (c && root) {
      const v = getComputedStyle(root).getPropertyValue(`--${c}`).trim();
      if (/^#[0-9a-f]{6}$/i.test(v)) return v;
    }
    return "#4c6ef5";
  }
</script>

<div class="picker" bind:this={root}>
  <button
    class="trigger ghost"
    title={t("env.style")}
    aria-label={t("env.style")}
    aria-haspopup="dialog"
    aria-expanded={open}
    onclick={() => (open = !open)}
  >
    <EnvIcon size={18} {icon} {color} />
  </button>
  {#if open}
    <div class="pop" role="dialog" aria-label={t("env.style")}>
      <p class="label">{t("env.icon")}</p>
      <div class="grid">
        {#each ENV_ICON_IDS as id (id)}
          <button
            class="cell ghost"
            class:sel={(icon ?? "layers") === id}
            title={id}
            aria-label={id}
            aria-pressed={(icon ?? "layers") === id}
            onclick={() => (icon = id === "layers" ? null : id)}
          >
            <EnvIcon size={16} icon={id} {color} />
          </button>
        {/each}
      </div>
      <p class="label">{t("env.color")}</p>
      <div class="grid">
        <button
          class="swatch none"
          class:sel={!color}
          title={t("env.colorNone")}
          aria-label={t("env.colorNone")}
          aria-pressed={!color}
          onclick={() => (color = null)}
        ></button>
        {#each ENV_COLORS as c (c)}
          <button
            class="swatch"
            class:sel={color === c}
            style:background={envColor(c)}
            title={c}
            aria-label={c}
            aria-pressed={color === c}
            onclick={() => (color = c)}
          ></button>
        {/each}
        <label class="swatch custom" class:sel={!!color?.startsWith("#")} title={t("env.colorCustom")}>
          <input
            type="color"
            value={hexOf(color)}
            aria-label={t("env.colorCustom")}
            oninput={(e) => (color = e.currentTarget.value)}
          />
        </label>
      </div>
    </div>
  {/if}
</div>

<style>
  .picker {
    position: relative;
    flex: none;
  }
  .trigger {
    display: flex;
    padding: 5px;
    border: 1px solid var(--border);
    border-radius: 6px;
  }
  .pop {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    z-index: 50;
    width: 236px;
    padding: 8px 10px 10px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: var(--shadow);
  }
  .label {
    margin: 4px 0 6px;
    font-size: 11px;
    color: var(--muted);
  }
  .grid {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .cell {
    display: flex;
    padding: 5px;
    border: 1px solid transparent;
    border-radius: 5px;
  }
  .cell.sel {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 12%, transparent);
  }
  .swatch {
    width: 22px;
    height: 22px;
    padding: 0;
    border-radius: 50%;
    border: 2px solid transparent;
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--text) 20%, transparent);
    cursor: pointer;
  }
  .swatch.sel {
    border-color: var(--text);
  }
  .swatch.none {
    background: linear-gradient(135deg, transparent 45%, var(--err) 45%, var(--err) 55%, transparent 55%), var(--surface);
  }
  .swatch.custom {
    position: relative;
    overflow: hidden;
    background: conic-gradient(red, yellow, lime, cyan, blue, magenta, red);
  }
  .swatch.custom input {
    position: absolute;
    inset: 0;
    opacity: 0;
    cursor: pointer;
  }
</style>
