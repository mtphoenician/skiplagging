<script lang="ts">
  import { page } from '$app/state';
  import HiddenCityCard from '$lib/components/HiddenCityCard.svelte';
  import OfferCard from '$lib/components/OfferCard.svelte';
  import SearchForm from '$lib/components/SearchForm.svelte';
  import TrafficPanel from '$lib/components/TrafficPanel.svelte';
  import { itineraryBookers, searchFares } from '$lib/api';
  import type { Cabin, SearchResponse } from '$lib/types';

  const params = $derived(page.url.searchParams);
  let data = $state<SearchResponse | null>(null);
  let error = $state('');
  let loading = $state(true);
  let tab = $state<'flights' | 'hidden' | 'live' | 'debug'>('flights');

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

  const names = $derived(data?.airline_names ?? {});
  const noShop = $derived(Boolean(data && data.cheapest_any == null && data.data_gaps.length));
  const lists = $derived(data?.channels.filter((c) => c.kind !== 'hidden-city') ?? []);
  const featuredIds = $derived(
    new Set([data?.honest_pick?.offer.id, data?.best_pick?.offer.id].filter(Boolean) as string[])
  );
  const restLists = $derived(
    lists.map((c) => ({ ...c, offers: c.offers.filter((o) => !featuredIds.has(o.id)) }))
  );
  const listedCount = $derived(restLists.reduce((n, c) => n + c.offers.length, 0));
  const moreHidden = $derived(
    (data?.hidden_city ?? []).filter((m) => m.id !== data?.hidden_if_cheaper?.id)
  );
  const pick = $derived(data?.honest_pick?.offer ?? data?.best_pick?.offer ?? null);
  const confirms = $derived.by(() => {
    if (!pick || !data) return [];
    const o = pick.segments[0]?.origin || '';
    const d = pick.segments[pick.segments.length - 1]?.dest || '';
    return itineraryBookers(o, d, data.query.date, data.query.adults, pick.currency);
  });
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
    </header>

    {#if noShop}
      <p class="banner">
        No fare-shop keys are configured, so this app cannot price a ticket.
      </p>
    {/if}

    {#if data.hidden_if_cheaper}
      <p class="pick-kicker">Cheaper hidden-city ticket</p>
      <HiddenCityCard match={data.hidden_if_cheaper} names={names} />
    {/if}

    {#if data.honest_pick || data.best_pick}
      <div class="picks-row">
        {#if data.honest_pick}
          <div>
            <p class="pick-kicker">{data.honest_pick.reason}</p>
            <OfferCard offer={data.honest_pick.offer} featured names={names} />
          </div>
        {/if}
        {#if data.best_pick}
          <div>
            <p class="pick-kicker">{data.best_pick.reason}</p>
            <OfferCard offer={data.best_pick.offer} names={names} />
          </div>
        {/if}
      </div>
    {/if}

    {#if !data.honest_pick && !data.best_pick && !data.hidden_if_cheaper && !listedCount}
      <p class="empty">
        {noShop
          ? 'No priced flights here — this app is not connected to a fare shop.'
          : 'No priced flight on this city pair for that date. Try another date or nearby airports.'}
      </p>
    {/if}

    {#if confirms.length}
      <p class="note" style="margin:14px 0 8px">Confirm on another site. We do not copy their prices.</p>
      <div class="book-row">
        {#each confirms as b}
          <a class="book-btn" href={b.url} target="_blank" rel="noopener noreferrer">{b.name}</a>
        {/each}
      </div>
    {/if}

    <div class="tabs">
      <button class:on={tab === 'flights'} type="button" onclick={() => (tab = 'flights')}>
        All flights {listedCount ? `(${listedCount})` : ''}
      </button>
      <button class:on={tab === 'hidden'} type="button" onclick={() => (tab = 'hidden')}>
        Hidden city {data.hidden_city.length ? `(${data.hidden_city.length})` : ''}
      </button>
      <button class:on={tab === 'live'} type="button" onclick={() => (tab = 'live')}>Live</button>
      <button class:on={tab === 'debug'} type="button" onclick={() => (tab = 'debug')}>Debug</button>
    </div>

    {#if tab === 'flights'}
      {#if listedCount}
        {#each restLists as ch}
          {#if ch.offers.length}
            <h2 class="section-title">
              {ch.kind === 'nonstop' ? 'Nonstop' : ch.kind === 'connecting' ? 'Connecting' : 'Nearby airports'}
            </h2>
            {#each ch.offers as offer}
              <OfferCard {offer} names={names} />
            {/each}
          {/if}
        {/each}
      {:else}
        <p class="empty">No other priced flights beyond the one above.</p>
      {/if}
    {:else if tab === 'hidden'}
      <p class="note">
        A cheaper ticket that continues past your city. You would get off at your stop. Airlines prohibit this.
      </p>
      {#if moreHidden.length}
        {#each moreHidden as match}
          <HiddenCityCard {match} names={names} />
        {/each}
      {:else if data.hidden_if_cheaper}
        <p class="empty">The cheaper hidden-city ticket is above. No other inversion on this date.</p>
      {:else}
        <p class="empty">No through-ticket cheaper than the cheapest honest fare on this date.</p>
      {/if}
    {:else if tab === 'live'}
      <TrafficPanel traffic={data.traffic_origin} label={data.origin.city} />
      <TrafficPanel traffic={data.traffic_destination} label={data.destination.city} />
    {:else if tab === 'debug'}
      <p class="note">
        Hidden-city means a complete ticket continues past your city. We never turn that ticket into a fake local fare.
      </p>
      {#if data.search_debug}
        <h2 class="section-title">Search planner</h2>
        <p class="note">
          Providers: {data.search_debug.providers.join(', ') || 'none'} · Standard query:
          {data.search_debug.standard_query} · Cache: {data.search_debug.cache}
          · Reused offers: {data.search_debug.reused_from_index ?? 0}
        </p>
        {#if data.search_debug.expanded_destinations.length}
          <p class="note">Live expansion: {data.search_debug.expanded_destinations.join(', ')}</p>
        {/if}
        {#if data.search_debug.skipped_expansion?.length}
          <p class="note">Skipped (already indexed through your city): {data.search_debug.skipped_expansion.join(', ')}</p>
        {/if}
        {#each data.search_debug.rejected as row}
          <p class="note">{row}</p>
        {/each}
      {:else}
        <p class="empty">No planner trace on this response.</p>
      {/if}
    {/if}
  {/if}
</div>
