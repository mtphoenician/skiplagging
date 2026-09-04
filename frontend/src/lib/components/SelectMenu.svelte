<script lang="ts">
  let {
    value = $bindable(''),
    label,
    options
  }: { value: string; label: string; options: { value: string; label: string }[] } = $props();

  let open = $state(false);
  let root: HTMLDivElement | undefined = $state();
  const current = $derived(options.find((o) => o.value === value)?.label ?? 'Choose');

  function pick(v: string) {
    value = v;
    open = false;
  }

  function onDoc(e: MouseEvent) {
    if (root && !root.contains(e.target as Node)) open = false;
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') open = false;
  }
</script>

<svelte:window onclick={onDoc} onkeydown={onKey} />

<div class="field picker" bind:this={root}>
  <span class="field-label">{label}</span>
  <button class="ctrl" type="button" aria-expanded={open} aria-haspopup="listbox" onclick={() => (open = !open)}>
    <span>{current}</span>
    <svg class="chev" class:up={open} viewBox="0 0 20 20" width="14" height="14" aria-hidden="true">
      <path d="M5 7.5 10 13l5-5.5" fill="none" stroke="currentColor" stroke-width="1.6" />
    </svg>
  </button>
  {#if open}
    <div class="pop menu" role="listbox">
      {#each options as o}
        <button
          class="menu-item"
          class:on={o.value === value}
          type="button"
          role="option"
          aria-selected={o.value === value}
          onclick={() => pick(o.value)}
        >
          {o.label}
        </button>
      {/each}
    </div>
  {/if}
</div>
