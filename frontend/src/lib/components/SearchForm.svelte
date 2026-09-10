<script lang="ts">
  import { goto } from '$app/navigation';
  import AirportInput from '$lib/components/AirportInput.svelte';
  import DatePicker from '$lib/components/DatePicker.svelte';
  import SelectMenu from '$lib/components/SelectMenu.svelte';
  import { buildSearchHref, searchAirports } from '$lib/api';
  import type { Cabin, SearchMode } from '$lib/types';

  const cabins = [
    { value: 'ECONOMY', label: 'Economy' },
    { value: 'PREMIUM_ECONOMY', label: 'Premium' },
    { value: 'BUSINESS', label: 'Business' },
    { value: 'FIRST', label: 'First' }
  ];

  let {
    origin = $bindable(''),
    destination = $bindable(''),
    date = $bindable(''),
    return_date = $bindable(''),
    adults = $bindable(1),
    cabin = $bindable<Cabin>('ECONOMY'),
    include_nearby = $bindable(false),
    mode = $bindable<SearchMode>('hidden'),
    compact = false
  }: {
    origin: string;
    destination: string;
    date: string;
    return_date?: string;
    adults: number;
    cabin: Cabin;
    include_nearby: boolean;
    mode: SearchMode;
    compact?: boolean;
  } = $props();

  let error = $state('');
  let submitting = $state(false);

  $effect(() => {
    if (return_date && date && return_date < date) return_date = '';
  });

  function setRoundTrip(on: boolean) {
    mode = on ? 'compare' : 'hidden';
    include_nearby = on;
  }

  function swap() {
    const a = origin;
    origin = destination;
    destination = a;
  }

  async function resolveIata(raw: string): Promise<string> {
    const t = raw.trim();
    if (/^city-[A-Za-z]{3}$/i.test(t)) return `CITY-${t.slice(-3).toUpperCase()}`;
    if (/^[A-Za-z]{3}$/.test(t)) return t.toUpperCase();
    if (t.length < 2) return '';
    const hits = await searchAirports(t);
    return hits[0]?.place_id || hits[0]?.iata || '';
  }

  async function submit(e: Event) {
    e.preventDefault();
    error = '';
    if (!date) {
      error = 'Choose a date.';
      return;
    }
    if (return_date && return_date < date) {
      error = 'Return date must be on or after the outbound date.';
      return;
    }
    if (mode === 'compare' && !return_date) {
      error = 'A round-trip ticket needs a return date. Uncheck it to look for hidden-city — a back date is a second one-way.';
      return;
    }
    submitting = true;
    try {
      const [o, d] = await Promise.all([resolveIata(origin), resolveIata(destination)]);
      if (!o || !d) {
        error = 'Choose a from and to city or airport.';
        return;
      }
      if (o === d) {
        error = 'From and to must be different.';
        return;
      }
      origin = o;
      destination = d;
      await goto(
        buildSearchHref({
          origin: o,
          destination: d,
          date,
          returnDate: return_date,
          adults,
          cabin,
          nearby: include_nearby,
          mode
        })
      );
    } catch (err) {
      error = err instanceof Error ? err.message : 'Airport lookup failed.';
    } finally {
      submitting = false;
    }
  }
</script>

<form class="search-card" class:compact onsubmit={submit}>
  <p class="trip-hint">
    {#if mode === 'compare'}
      One round-trip ticket. Hidden-city cannot be classified — uncheck below to search two one-ways instead.
    {:else}
      This search looks for a cheaper complete ticket that continues past your city. A back date is a second one-way home, never a round-trip PNR.
    {/if}
  </p>
  <div class="search-grid">
    <AirportInput bind:value={origin} label="From" />
    <button class="swap" type="button" onclick={swap} aria-label="Swap airports">
      <svg viewBox="0 0 20 20" width="18" height="18" aria-hidden="true">
        <path d="M4 7h10M11 4l3 3-3 3M16 13H6M9 10l-3 3 3 3" fill="none" stroke="currentColor" stroke-width="1.6" />
      </svg>
    </button>
    <AirportInput bind:value={destination} label="To" />
    <div class="span-date">
      <DatePicker bind:value={date} label="Depart" />
    </div>
    <div class="span-return">
      <DatePicker
        bind:value={return_date}
        label={mode === 'compare' ? 'Return' : 'Back (one-way home)'}
        clearable
        min={date}
      />
    </div>
    <div class="span-cabin">
      <SelectMenu bind:value={cabin} label="Cabin" options={cabins} />
    </div>
    <button class="go" type="submit" disabled={submitting}>
      {submitting ? 'Searching…' : mode === 'compare' ? 'Compare round-trip' : 'Find hidden-city'}
    </button>
  </div>
  <div class="toggles">
    <button class="check" type="button" aria-pressed={include_nearby} onclick={() => (include_nearby = !include_nearby)}>
      <span class="box" class:on={include_nearby}>
        {#if include_nearby}
          <svg viewBox="0 0 12 12" width="10" height="10"><path d="M2 6.2 4.6 9 10 3" fill="none" stroke="#fff" stroke-width="1.8" /></svg>
        {/if}
      </span>
      <span class="check-label">Nearby airports</span>
    </button>
    <button
      class="check"
      type="button"
      aria-pressed={mode === 'compare'}
      onclick={() => setRoundTrip(mode !== 'compare')}
    >
      <span class="box" class:on={mode === 'compare'}>
        {#if mode === 'compare'}
          <svg viewBox="0 0 12 12" width="10" height="10"><path d="M2 6.2 4.6 9 10 3" fill="none" stroke="#fff" stroke-width="1.8" /></svg>
        {/if}
      </span>
      <span class="check-label">Round-trip only (skips hidden-city)</span>
    </button>
    <div class="stepper" role="group" aria-label="Adults">
      <button type="button" class="icon-btn" aria-label="Fewer adults" onclick={() => (adults = Math.max(1, adults - 1))}>−</button>
      <span>{adults}</span>
      <button type="button" class="icon-btn" aria-label="More adults" onclick={() => (adults = Math.min(9, adults + 1))}>+</button>
      <em>Adults</em>
    </div>
  </div>
  {#if error}
    <p class="search-error" role="alert">{error}</p>
  {/if}
</form>

<style>
  .search-error {
    margin: 8px 2px 0;
    color: var(--muted);
    font-size: 0.85rem;
  }
  .go:disabled {
    opacity: 0.6;
  }
</style>
