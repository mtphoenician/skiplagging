<script lang="ts">
  import { page } from '$app/state';
  import HiddenCityCard from '$lib/components/HiddenCityCard.svelte';
  import OfferCard from '$lib/components/OfferCard.svelte';
  import { money, searchFares } from '$lib/api';
  import type { Cabin, SearchResponse } from '$lib/types';

  const params = $derived(page.url.searchParams);
  let data = $state<SearchResponse | null>(null);
  let error = $state('');
  let loading = $state(true);
  $effect(() => {
    const origin = (params.get('origin') || '').toUpperCase();
    const destination = (params.get('destination') || '').toUpperCase();
    const date = params.get('date') || '';
    const adults = Number(params.get('adults') || 1);
    const cabin = (params.get('cabin') || 'ECONOMY') as Cabin;
    const include_nearby = params.get('nearby') !== '0';
    const live = params.get('live') === '1';
    loading = true;
    error = '';
    data = null;
    if (origin.length !== 3 || destination.length !== 3 || !date) {
      error = 'Need origin, destination and date.';
      loading = false;
      return;
    }
    searchFares({
      origin,
      destination,
      date,
      adults,
      cabin,
      currency: 'USD',
      include_nearby,
      live
    })
      .then((r) => {
        data = r;
      })
      .catch((e: Error) => {
        error = e.message;
      })
      .finally(() => {
        loading = false;
      });
  });
</script>

<svelte:head>
  <title>Results — Skiplagging</title>
</svelte:head>

<div class="wrap">
  {#if loading}
    <p class="empty">Shopping nonstops, connections, nearby airports and hidden-city C candidates…</p>
  {:else if error}
    <p class="empty">{error}</p>
  {:else if data}
    <header class="results-head">
      <div>
        <p class="kicker">{data.origin.city} → {data.destination.city}</p>
        <h1 class="serif" style="margin:8px 0;font-size:clamp(2rem,4vw,3rem)">
          {data.query.origin} → {data.query.destination}
          <span style="color:var(--mist);font-weight:400"> {data.query.date}</span>
        </h1>
        <p class="note">
          Local market vs through-market shopping. Hidden-city rows are A→B→C tickets whose first sector is the useful A→B
          flight.
        </p>
      </div>
      <div class="stat">
        {data.elapsed_ms} ms · {data.sources_used.join(', ')}<br />
        cheapest local {data.cheapest_local != null ? money(data.cheapest_local) : '—'}<br />
        cheapest any {data.cheapest_any != null ? money(data.cheapest_any) : '—'}<br />
        {data.hidden_city.length} inversions
      </div>
    </header>

    <section>
      <h2 class="serif">Hidden-city inversions</h2>
      <p class="note">P(A,B,C) &lt; P(A,B) on a ticketed destination C, preferably the same first aircraft.</p>
      {#if data.hidden_city.length}
        {#each data.hidden_city as match}
          <HiddenCityCard {match} />
        {/each}
      {:else}
        <p class="empty">No price inversion on this date in the current shop. Try a hub destination near departure.</p>
      {/if}
    </section>

    {#each data.channels.filter((c) => c.kind !== 'hidden-city') as ch}
      <section style="margin-top:36px">
        <h2 class="serif">{ch.label}</h2>
        <p class="note">{ch.blurb}</p>
        {#if ch.offers.length}
          {#each ch.offers as offer}
            <OfferCard {offer} />
          {/each}
        {:else}
          <p class="empty">No offers in this channel.</p>
        {/if}
      </section>
    {/each}

    <section style="margin:40px 0 24px">
      {#each data.notes as n}
        <p class="note">{n}</p>
      {/each}
    </section>
  {/if}
</div>
