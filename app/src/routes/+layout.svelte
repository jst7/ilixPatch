<script lang="ts">
  import { onMount } from "svelte";
  import "$lib/styles/global.css";
  import { initTheme } from "$lib/theme/index.svelte";

  let { children } = $props();

  /** Inputs that take text (a checkbox or a button has nothing to cut, copy or paste). */
  const TEXT_INPUT = new Set(["text", "search", "url", "email", "password", "number", "tel", ""]);

  /**
   * Where the webview's own context menu is still useful: text fields and editors (cut / copy / paste)
   * and selected text (copy). Elsewhere it only offers "Reload" (and "Inspect Element" in dev builds).
   */
  function nativeMenuUseful(e: MouseEvent): boolean {
    const el = e.target instanceof Element ? e.target : null;
    if (el?.closest("textarea, [contenteditable='true'], [contenteditable='']")) return true;
    const input = el?.closest("input");
    if (input && TEXT_INPUT.has(input.type)) return true;
    return !!window.getSelection()?.toString();
  }

  onMount(() => {
    initTheme();
    // Bubbling phase: the app's own menus (tree, tabs…) already called preventDefault.
    const oncontextmenu = (e: MouseEvent) => {
      if (e.defaultPrevented || nativeMenuUseful(e)) return;
      // Development builds: Shift + right click keeps the native menu, for "Inspect Element".
      if (import.meta.env.DEV && e.shiftKey) return;
      e.preventDefault();
    };
    window.addEventListener("contextmenu", oncontextmenu);
    return () => window.removeEventListener("contextmenu", oncontextmenu);
  });
</script>

{@render children()}
