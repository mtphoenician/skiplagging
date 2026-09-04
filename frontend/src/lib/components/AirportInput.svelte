<script lang="ts">
  import { searchAirports } from '$lib/api';
  import type { Airport } from '$lib/types';

  let { label, value = $bindable(''), placeholder = 'JFK' }: { label: string; value: string; placeholder?: string } =
    $props();

  let open = $state(false);
  let hits = $state<Airport[]>([]);
  let active = $state(0);
  let timer: ReturnType<typeof setTimeout>;

  async function lookup(q: string) {
    if (q.trim().length < 1) {
      hits = [];
      return;
    }
    try {
      hits = await searchAirports(q);
      open = hits.length > 0;
      active = 0;
    } catch {
      hits = [];
    }
  }

  function onInput(e: Event) {
    const next = (e.target as HTMLInputElement).value.toUpperCase();
    value = next;
    clearTimeout(timer);
    timer = setTimeout(() => lookup(next), 120);
  }

  function pick(a: Airport) {
    value = a.iata;
    open = false;
    hits = [];
  }

  function onKey(e: KeyboardEvent) {
    if (!open || !hits.length) return;
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      active = (active + 1) % hits.length;
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      active = (active - 1 + hits.length) % hits.length;
    } else if (e.key === 'Enter' && hits[active]) {
      e.preventDefault();
      pick(hits[active]);
    } else if (e.key === 'Escape') {
      open = false;
    }
  }
</script>

<div class="field suggest">
  <label for="ap-{label}">{label}</label>
  <input
    id="ap-{label}"
    {placeholder}
    maxlength="32"
    value={value}
    oninput={onInput}
    onkeydown={onKey}
    onfocus={() => hits.length && (open = true)}
    autocomplete="off"
  />
  {#if open && hits.length}
    <div class="suggest-list">
      {#each hits as a, i}
        <button class:active={i === active} type="button" onmousedown={() => pick(a)}>
          <strong>{a.iata}</strong>
          <span style="color:var(--mist)"> {a.city} · {a.name}</span>
        </button>
      {/each}
    </div>
  {/if}
</div>
