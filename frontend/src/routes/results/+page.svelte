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
  let tab = $state<'flights' | 'hidden' | 'live'>('flights');

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
        if (r.hidden_city.length && !r.honest_pick) tab = 'hidden';
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
  const connecting = $derived(data?.channels.find((c) => c.kind === 'connecting')?.offers[0] ?? null);
  const showConnecting = $derived(
    Boolean(connecting && connecting.id !== data?.honest_pick?.offer.id)
  );
  const pick = $derived(data?.honest_pick?.offer ?? connecting);
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

    {#if data.honest_pick}
      <p class="pick-kicker">{data.hidden_if_cheaper ? 'Honest ticket' : data.honest_pick.reason}</p>
      <OfferCard offer={data.honest_pick.offer} featured={!data.hidden_if_cheaper} names={names} />
    {/if}

    {#if showConnecting && connecting}
      <p class="pick-kicker">Cheapest connecting</p>
      <OfferCard offer={connecting} names={names} />
    {/if}

    {#if !data.honest_pick && !data.hidden_if_cheaper && !connecting}
      <p class="empty">
        {noShop
          ? 'No priced flights here — this app is not connected to a fare shop.'
          : 'No flight on this city pair in the shop. Duffel test often prices other airports; try another date.'}
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
      <button class:on={tab === 'hidden'} type="button" onclick={() => (tab = 'hidden')}>
        Hidden city {data.hidden_city.length ? `(${data.hidden_city.length})` : ''}
      </button>
      <button class:on={tab === 'live'} type="button" onclick={() => (tab = 'live')}>Live</button>
    </div>

    {#if tab === 'hidden'}
      <p class="note">A cheaper ticket that continues past your city. You would get off there. Airlines prohibit this.</p>
      {#if data.hidden_city.length}
        {#each data.hidden_city as match}
          <HiddenCityCard {match} names={names} />
        {/each}
      {:else}
        <p class="empty">No cheaper through-ticket on this date.</p>
      {/if}
    {:else if tab === 'live'}
      <TrafficPanel traffic={data.traffic_origin} label={data.origin.city} />
      <TrafficPanel traffic={data.traffic_destination} label={data.destination.city} />
    {/if}
  {/if}
</div>
