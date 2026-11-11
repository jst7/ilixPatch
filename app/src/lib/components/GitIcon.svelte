<script lang="ts">
  let { size = 12 }: { size?: number } = $props();
</script>

<!-- Branch icon; inherits the text colour. -->
<svg viewBox="0 0 16 16" width={size} height={size} aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.4">
  <circle cx="4.5" cy="3.5" r="1.6" />
  <circle cx="4.5" cy="12.5" r="1.6" />
  <circle cx="11.5" cy="5.5" r="1.6" />
  <path d="M4.5 5.1v5.8M11.5 7.1c0 2.4-3.5 2.2-6.2 4" />
</svg>
