<script lang="ts">
  interface Props {
    placeholder: string;
    depth: number;
    oncommit: (name: string) => void;
    oncancel: () => void;
  }

  let { placeholder, depth, oncommit, oncancel }: Props = $props();

  let value = $state("");
  let done = false;

  function focusOnMount(el: HTMLInputElement) {
    el.focus();
  }

  function finish(commit: boolean) {
    if (done) return;
    done = true;
    if (commit && value.trim()) oncommit(value.trim());
    else oncancel();
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      finish(true);
    } else if (e.key === "Escape") {
      e.preventDefault();
      finish(false);
    }
  }
</script>

<div class="inline" style:padding-left="{8 + depth * 14 + 18}px">
  <input
    use:focusOnMount
    bind:value
    {placeholder}
    {onkeydown}
    onblur={() => finish(false)}
    spellcheck="false"
    autocomplete="off"
  />
</div>

<style>
  .inline {
    padding: 2px 8px 2px 0;
  }
  input {
    width: 100%;
    padding: 3px 6px;
    font-size: 13px;
  }
</style>
