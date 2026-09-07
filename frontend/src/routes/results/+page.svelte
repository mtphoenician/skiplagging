<script lang="ts">
  import { page } from '$app/state';
  import BookerList from '$lib/components/BookerList.svelte';
  import HiddenCityCard from '$lib/components/HiddenCityCard.svelte';
  import OfferCard from '$lib/components/OfferCard.svelte';
  import SearchForm from '$lib/components/SearchForm.svelte';
  import TrafficPanel from '$lib/components/TrafficPanel.svelte';
  import { money, searchFares } from '$lib/api';
  import type { Cabin, SearchResponse } from '$lib/types';

  const params = $derived(page.url.searchParams);
  let data = $state<SearchResponse | null>(null);
  let error = $state('');
  let loading = $state(true);
  let tab = $state<'flights' | 'hidden' | 'book' | 'live'>('flights');

  let origin = $state('');
  let destination = $state('');
  let date = $state('');
  let adults = $state(1);
  let cabin = $state<Cabin>('ECONOMY');
  let include_nearby = $state(true);

  $effect(() => {
    const o = (params.get('origin') || '').toUpperCase();
    const d = (params.get('destination') || '').toUpperCase();
    const dt = params.get('date') || '';
    const ad = Number(params.get('adults') || 1);
    const cb = (params.get('cabin') || 'ECONOMY') as Cabin;
    const near = params.get('nearby') !== '0';

    origin = o;
    destination = d;
    date = dt;
    adults = ad;
    cabin = cb;
    include_nearby = near;
    loading = true;
    error = '';
    data = null;

    if (!/^(CITY-)?[A-Z]{3}$/.test(o) || !/^(CITY-)?[A-Z]{3}$/.test(d) || !dt) {
      error = 'Choose two airports and a date.';
      loading = false;
      return;
    }

    let cancelled = false;
    searchFares({
      origin: o,
      destination: d,
      date: dt,
      adults: ad,
      cabin: cb,
      currency: 'USD',
      include_nearby: near,
      allow_synthetic: false
    })
      .then((r) => {
        if (cancelled) return;
        data = r;
        if (r.hidden_city.length && !r.channels.some((c) => c.kind !== 'hidden-city' && c.offers.length)) {
          tab = 'hidden';
        }
      })
      .catch((e: Error) => {
        if (!cancelled) error = e.message;
      })
      .finally(() => {
        if (!cancelled) loading = false;
      });

    return () => {
      cancelled = true;
    };
  });

  const priced = $derived(data?.channels.filter((c) => c.kind !== 'hidden-city') ?? []);
  const pricedCount = $derived(priced.reduce((n, c) => n + c.offers.length, 0));
  const noShop = $derived(Boolean(data && data.cheapest_any == null && data.data_gaps.length));
</script>

<svelte:head>
  <title>Results — Skiplagging</title>
</svelte:head>

<div class="wrap page">
  <SearchForm
    compact
    bind:origin
    bind:destination
    bind:date
    bind:adults
    bind:cabin
    bind:include_nearby
  />

  {#if loading}
    <p class="empty">Searching…</p>
  {:else if error}
    <p class="empty">{error}</p>
  {:else if data}
    <header class="results-head">
      <h1>{data.origin.city} → {data.destination.city}</h1>
      <p class="meta">
        {data.origin.type === 'city' ? `${data.origin.city} (all)` : data.origin.iata}–{data.destination.type === 'city'
          ? `${data.destination.city} (all)`
          : data.destination.iata} · {data.query.date}
        · {data.query.adults} adult{data.query.adults === 1 ? '' : 's'}
      </p>
      <div class="stats">
        {#if data.honest_pick}
          <span class="stat-pill">{data.honest_pick.reason}</span>
        {:else if data.cheapest_local != null}
          <span class="stat-pill">From {money(data.cheapest_local)}</span>
        {/if}
        {#if data.hidden_if_cheaper}
          <span class="stat-pill">Hidden-city is cheaper</span>
        {/if}
      </div>
    </header>

    {#if noShop}
      <p class="banner">
        No fare-shop keys are configured, so this app cannot price a ticket. Google Flights, Kayak, and the others below
        have the live inventory — open the same trip there.
      </p>
    {/if}

    {#if data.honest_pick}
      <p class="pick-kicker">{data.honest_pick.reason}</p>
      <OfferCard offer={data.honest_pick.offer} featured />
    {/if}
    {#if data.hidden_if_cheaper}
      <p class="pick-kicker">Cheaper hidden-city ticket</p>
      <HiddenCityCard match={data.hidden_if_cheaper} />
    {/if}

    <p class="note" style="margin:14px 0 8px">Compare this trip</p>
    <BookerList links={data.bookers} compact />

    <div class="tabs">
      <button class:on={tab === 'flights'} type="button" onclick={() => (tab = 'flights')}>
        Flights {pricedCount ? `(${pricedCount})` : ''}
      </button>
      <button class:on={tab === 'hidden'} type="button" onclick={() => (tab = 'hidden')}>
        Hidden city {data.hidden_city.length ? `(${data.hidden_city.length})` : ''}
      </button>
      <button class:on={tab === 'book'} type="button" onclick={() => (tab = 'book')}>Book</button>
      <button class:on={tab === 'live'} type="button" onclick={() => (tab = 'live')}>Live</button>
    </div>

    {#if tab === 'flights'}
      {#each priced as ch}
        {#if ch.offers.length}
          <h2 class="section-title">
            {ch.kind === 'nonstop' ? 'Nonstop' : ch.kind === 'connecting' ? 'Connecting' : 'Nearby airports'}
          </h2>
          {#each ch.offers.filter((o) => o.id !== data.honest_pick?.offer.id) as offer}
            <OfferCard {offer} />
          {/each}
        {/if}
      {/each}
      {#if !pricedCount}
        <p class="empty">
          {noShop
            ? 'No priced flights here — this app is not connected to a fare shop.'
            : 'No priced flights for this search. Use the booking sites above.'}
        </p>
        {#if data.connection_hints.length}
          <h2 class="section-title">Cities historically beyond {data.destination.iata}</h2>
          <p class="note">OpenFlights routes from {data.destination.iata}, not live fares. Hidden-city inversions need a shop key.</p>
          <table class="mini">
            <thead>
              <tr>
                <th>City</th>
                <th>Why listed</th>
              </tr>
            </thead>
            <tbody>
              {#each data.connection_hints as h}
                <tr>
                  <td>{h.dest_name}</td>
                  <td class="note">{h.source === 'amadeus' ? 'Live destination list' : 'Historical route'}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
      {/if}
    {:else if tab === 'hidden'}
      <p class="note">A cheaper ticket that continues past {data.destination.iata}. You would get off there. Airlines prohibit this.</p>
      {#if data.hidden_city.length}
        {#each data.hidden_city as match}
          <HiddenCityCard {match} />
        {/each}
      {:else}
        <p class="empty">
          {noShop
            ? 'Hidden-city prices also come from Amadeus or Duffel. Without those keys we can only list historical cities beyond the destination.'
            : 'No cheaper through-tickets on this date.'}
        </p>
      {/if}
      {#if data.connection_hints.length}
        <h2 class="section-title">Cities beyond {data.destination.iata}</h2>
        <table class="mini">
          <thead>
            <tr>
              <th>City</th>
              <th>Why listed</th>
            </tr>
          </thead>
          <tbody>
            {#each data.connection_hints as h}
              <tr>
                <td>{h.dest_name}</td>
                <td class="note">{h.source === 'amadeus' ? 'Live destination list' : 'Historical route'}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
    {:else if tab === 'book'}
      <BookerList links={data.bookers} />
    {:else}
      <TrafficPanel traffic={data.traffic_origin} label={data.origin.city} />
      <TrafficPanel traffic={data.traffic_destination} label={data.destination.city} />
      {#if data.board_origin.length}
        <h2 class="section-title">Departures at {data.origin.iata}</h2>
        <table class="mini">
          <thead>
            <tr>
              <th>Flight</th>
              <th>To</th>
              <th>Time</th>
              <th>Status</th>
            </tr>
          </thead>
          <tbody>
            {#each data.board_origin as f}
              <tr>
                <td>{f.flight_number}</td>
                <td>{f.dest}</td>
                <td>{f.estimated || f.scheduled}</td>
                <td>{f.status}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
    {/if}
  {/if}
</div>
