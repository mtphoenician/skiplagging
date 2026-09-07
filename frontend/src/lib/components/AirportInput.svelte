<script lang="ts">
  import { fetchAirport, searchAirports } from '$lib/api';
  import type { Airport } from '$lib/types';

  let {
    label,
    value = $bindable(''),
    placeholder = 'City or airport'
  }: { label: string; value: string; placeholder?: string } = $props();

  let open = $state(false);
  let hits = $state<Airport[]>([]);
  let active = $state(0);
  let draft = $state('');
  let picked = $state<Airport | null>(null);
  let focused = $state(false);
  let timer: ReturnType<typeof setTimeout>;
  let root: HTMLDivElement | undefined = $state();
  let inputEl: HTMLInputElement | undefined = $state();

  function placeId(a: Airport) {
    return a.place_id || a.iata;
  }

  function displayOf(a: Airport) {
    return a.type === 'city' ? a.city : a.iata;
  }

  $effect(() => {
    if (focused) return;
    const v = value;
    if (picked && placeId(picked) === v) {
      draft = displayOf(picked);
      return;
    }
    if (v === draft) {
      if (/^(CITY-)?[A-Z]{3}$/.test(v) && (!picked || placeId(picked) !== v)) void hydrate(v);
      return;
    }
    draft = v;
    if (/^(CITY-)?[A-Z]{3}$/i.test(v.trim())) {
      const code = v.trim().toUpperCase();
      if (!picked || placeId(picked) !== code) void hydrate(code);
    } else if (!v) {
      picked = null;
    }
  });

  async function hydrate(code: string) {
    try {
      const ap = await fetchAirport(code);
      if (ap && !focused && value.toUpperCase() === code.toUpperCase()) {
        picked = ap;
        draft = displayOf(ap);
      }
    } catch {
      /* keep the IATA even if the name never loads */
    }
  }

  function onDoc(e: MouseEvent) {
    if (root && !root.contains(e.target as Node)) open = false;
  }

  async function lookup(q: string) {
    if (q.trim().length < 1) {
      hits = [];
      open = false;
      return;
    }
    try {
      hits = await searchAirports(q);
      open = hits.length > 0;
      active = 0;
    } catch {
      hits = [];
      open = false;
    }
  }

  function onInput(e: Event) {
    const next = (e.target as HTMLInputElement).value;
    draft = next;
    picked = null;
    value = next;
    clearTimeout(timer);
    timer = setTimeout(() => lookup(next.trim()), 80);
  }

  function pick(a: Airport) {
    picked = a;
    value = placeId(a);
    draft = displayOf(a);
    open = false;
    hits = [];
  }

  function onFocus() {
    focused = true;
    if (picked && picked.type !== 'city' && /^[A-Za-z]{3}$/.test(draft)) {
      queueMicrotask(() => inputEl?.select());
    }
    if (hits.length) open = true;
    else if (draft.trim().length >= 1) lookup(draft.trim());
  }

  function onBlur() {
    focused = false;
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Enter' && (!open || !hits.length)) {
      return;
    }
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

  function kind(a: Airport) {
    if (a.type === 'large_airport' && a.scheduled_service) return '';
    if (!a.scheduled_service) return 'No scheduled flights';
    if (a.type === 'medium_airport') return 'Regional';
    if (a.type === 'heliport') return 'Heliport';
    return '';
  }
</script>

<svelte:window onclick={onDoc} />

<div class="field suggest" bind:this={root}>
  <label for="ap-{label}">{label}</label>
  <div class="suggest-box">
    <input
      id="ap-{label}"
      bind:this={inputEl}
      class:has-meta={Boolean(picked && !focused)}
      {placeholder}
      maxlength="48"
      value={draft}
      oninput={onInput}
      onkeydown={onKey}
      onfocus={onFocus}
      onblur={onBlur}
      autocomplete="off"
      autocorrect="off"
      autocapitalize="none"
      spellcheck="false"
    />
    {#if picked && !focused}
      <span class="suggest-inline">{picked.type === 'city' ? 'All airports' : picked.city}</span>
    {/if}
  </div>
  {#if open && hits.length}
    <div class="suggest-list scroll" role="listbox">
      {#each hits as a, i}
        <button class:active={i === active} type="button" role="option" aria-selected={i === active} onmousedown={() => pick(a)}>
          <span class="suggest-top">
            <strong>{a.iata}</strong>
            <span class="suggest-name">{a.type === 'city' ? 'All airports' : a.name}</span>
          </span>
          <span class="suggest-meta">
            {a.type === 'city' && a.members?.length ? a.members.join(', ') : a.city}
            · {a.country_name || a.country}
            {#if kind(a)}
              · {kind(a)}
            {/if}
          </span>
        </button>
      {/each}
    </div>
  {/if}
</div>
