<script lang="ts">
  // Environment icon (D49): the environment's own icon and colour, or stacked layers in the text colour.
  import { envColor, envIconPaths } from "$lib/env/style";

  let {
    size = 14,
    icon = null,
    color = null,
  }: { size?: number; icon?: string | null; color?: string | null } = $props();
</script>

<svg
  viewBox="0 0 16 16"
  width={size}
  height={size}
  aria-hidden="true"
  fill="none"
  stroke="currentColor"
  stroke-width="1.4"
  stroke-linecap="round"
  stroke-linejoin="round"
  style:color={envColor(color)}
  style:flex="none"
>
  {#each envIconPaths(icon) as d, i (i)}<path {d} />{/each}
</svg>
