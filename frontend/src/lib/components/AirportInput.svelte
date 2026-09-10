<script lang="ts">
  import { onDestroy } from 'svelte';
  import { fetchAirport, isAbortError, searchAirports } from '$lib/api';
  import type { Airport } from '$lib/types';

  /** Pause after the last keystroke before searching. Cancels in-flight prefix queries. */
  const DEBOUNCE_MS = 500;

  let {
    label,
    value = $bindable(''),
    placeholder = 'City or airport'
  }: { label: string; value: string; placeholder?: string } = $props();

  let open = $state(false);
  let hits = $state.raw<Airport[]>([]);
  let hitsQuery = $state('');
  let active = $state(0);
  let draft = $state('');
  let picked = $state<Airport | null>(null);
  let focused = $state(false);
  let timer: ReturnType<typeof setTimeout> | undefined;
  let abort: AbortController | null = null;
  let root: HTMLDivElement | undefined = $state();
  let inputEl: HTMLInputElement | undefined = $state();

  function placeId(a: Airport) {
    return a.place_id || a.iata;
  }

  function displayOf(a: Airport) {
    return a.type === 'city' ? a.city : a.iata;
  }

  function cancelLookup() {
    clearTimeout(timer);
    timer = undefined;
    abort?.abort();
    abort = null;
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
    abort?.abort();
    const ac = new AbortController();
    abort = ac;
    try {
      const ap = await fetchAirport(code, ac.signal);
      if (ap && !focused && value.toUpperCase() === code.toUpperCase()) {
        picked = ap;
        draft = displayOf(ap);
      }
    } catch (e) {
      if (isAbortError(e)) return;
      /* keep the IATA even if the name never loads */
    }
  }

  $effect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (root && !root.contains(e.target as Node)) open = false;
    };
    window.addEventListener('click', onDoc);
    return () => window.removeEventListener('click', onDoc);
  });

  async function lookup(q: string) {
    if (draft.trim() !== q) return;
    abort?.abort();
    const ac = new AbortController();
    abort = ac;
    try {
      const next = await searchAirports(q, ac.signal);
      if (ac.signal.aborted || draft.trim() !== q) return;
      hits = next;
      hitsQuery = q;
      open = hits.length > 0;
      active = 0;
    } catch (e) {
      if (isAbortError(e) || draft.trim() !== q) return;
      hits = [];
      hitsQuery = '';
      open = false;
    }
  }

  function scheduleLookup(q: string, immediate = false) {
    cancelLookup();
    if (q.length < 1) {
      hits = [];
      hitsQuery = '';
      open = false;
      return;
    }
    if (hitsQuery !== q) {
      hits = [];
      open = false;
    }
    if (immediate) {
      void lookup(q);
      return;
    }
    timer = setTimeout(() => lookup(q), DEBOUNCE_MS);
  }

  function onInput(e: Event) {
    const next = (e.target as HTMLInputElement).value;
    draft = next;
    picked = null;
    value = next;
    scheduleLookup(next.trim());
  }

  function pick(a: Airport) {
    cancelLookup();
    picked = a;
    value = placeId(a);
    draft = displayOf(a);
    open = false;
    hits = [];
    hitsQuery = '';
  }

  function onFocus() {
    focused = true;
    if (picked && picked.type !== 'city' && /^[A-Za-z]{3}$/.test(draft)) {
      queueMicrotask(() => inputEl?.select());
    }
    const q = draft.trim();
    if (picked && (placeId(picked) === value || displayOf(picked) === q)) return;
    if (hits.length && hitsQuery === q) open = true;
    else if (q.length >= 1) scheduleLookup(q, true);
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

  onDestroy(cancelLookup);
</script>

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
      {#each hits as a, i (a.place_id || a.iata)}
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
