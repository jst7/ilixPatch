<script lang="ts">
  let { size = 14 }: { size?: number } = $props();
</script>

<!-- Environment icon (stacked layers); inherits the text colour. -->
<svg viewBox="0 0 16 16" width={size} height={size} aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round">
  <path d="M8 2l6 3-6 3-6-3z" />
  <path d="M2 8l6 3 6-3" />
  <path d="M2 11l6 3 6-3" />
</svg>
