<script lang="ts">
  import { onMount } from "svelte";
  import "$lib/styles/global.css";
  import { initTheme } from "$lib/theme/index.svelte";

  let { children } = $props();

  onMount(() => initTheme());
</script>

{@render children()}
