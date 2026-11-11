<script lang="ts">
  // Text with search matches wrapped in <mark>.
  import { highlight } from "$lib/search/match";

  let { text, terms }: { text: string; terms: string[] } = $props();

  const parts = $derived(highlight(text, terms));
</script>

{#each parts as p, i (i)}{#if p.hit}<mark class="search-hit">{p.text}</mark>{:else}{p.text}{/if}{/each}

<style>
  .search-hit {
    background: color-mix(in srgb, var(--search-match) 70%, transparent);
    color: inherit;
    border-radius: 2px;
    padding: 0;
  }
</style>
