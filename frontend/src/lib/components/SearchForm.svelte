<script lang="ts">
  import { goto } from '$app/navigation';
  import AirportInput from '$lib/components/AirportInput.svelte';
  import DatePicker from '$lib/components/DatePicker.svelte';
  import SelectMenu from '$lib/components/SelectMenu.svelte';
  import { searchAirports } from '$lib/api';
  import type { Cabin } from '$lib/types';

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
    adults = $bindable(1),
    cabin = $bindable<Cabin>('ECONOMY'),
    include_nearby = $bindable(true),
    compact = false
  }: {
    origin: string;
    destination: string;
    date: string;
    adults: number;
    cabin: Cabin;
    include_nearby: boolean;
    compact?: boolean;
  } = $props();

  function swap() {
    const a = origin;
    origin = destination;
    destination = a;
  }

  async function resolveIata(raw: string): Promise<string> {
    const t = raw.trim();
    if (/^[A-Za-z]{3}$/.test(t)) return t.toUpperCase();
    if (t.length < 2) return '';
    const hits = await searchAirports(t);
    return hits[0]?.iata || '';
  }

  async function submit(e: Event) {
    e.preventDefault();
    const [o, d] = await Promise.all([resolveIata(origin), resolveIata(destination)]);
    if (!o || !d || !date || o === d) return;
    origin = o;
    destination = d;
    const q = new URLSearchParams({
      origin: o,
      destination: d,
      date,
      adults: String(adults),
      cabin,
      nearby: include_nearby ? '1' : '0'
    });
    goto(`/results?${q.toString()}`);
  }
</script>

<form class="search-card" class:compact onsubmit={submit}>
  <div class="search-grid">
    <AirportInput bind:value={origin} label="From" />
    <button class="swap" type="button" onclick={swap} aria-label="Swap airports">
      <svg viewBox="0 0 20 20" width="18" height="18" aria-hidden="true">
        <path d="M4 7h10M11 4l3 3-3 3M16 13H6M9 10l-3 3 3 3" fill="none" stroke="currentColor" stroke-width="1.6" />
      </svg>
    </button>
    <AirportInput bind:value={destination} label="To" />
    <div class="span-date">
      <DatePicker bind:value={date} />
    </div>
    <div class="span-cabin">
      <SelectMenu bind:value={cabin} label="Cabin" options={cabins} />
    </div>
    <button class="go" type="submit">Search</button>
  </div>
  <div class="toggles">
    <button class="check" type="button" aria-pressed={include_nearby} onclick={() => (include_nearby = !include_nearby)}>
      <span class="box" class:on={include_nearby}>
        {#if include_nearby}
          <svg viewBox="0 0 12 12" width="10" height="10"><path d="M2 6.2 4.6 9 10 3" fill="none" stroke="#fff" stroke-width="1.8" /></svg>
        {/if}
      </span>
      Nearby airports
    </button>
    <div class="stepper" role="group" aria-label="Adults">
      <button type="button" class="icon-btn" aria-label="Fewer adults" onclick={() => (adults = Math.max(1, adults - 1))}>−</button>
      <span>{adults}</span>
      <button type="button" class="icon-btn" aria-label="More adults" onclick={() => (adults = Math.min(9, adults + 1))}>+</button>
      <em>Adults</em>
    </div>
  </div>
</form>
