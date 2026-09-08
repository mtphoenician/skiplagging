<script lang="ts">
  import { page } from '$app/state';
  import HiddenCityCard from '$lib/components/HiddenCityCard.svelte';
  import OfferCard from '$lib/components/OfferCard.svelte';
  import SearchForm from '$lib/components/SearchForm.svelte';
  import TrafficPanel from '$lib/components/TrafficPanel.svelte';
  import { expandSearch, itineraryBookers, mergeSearch, searchFares } from '$lib/api';
  import type { Cabin, SearchQuery, SearchResponse } from '$lib/types';

  const params = $derived(page.url.searchParams);
  let data = $state<SearchResponse | null>(null);
  let error = $state('');
  let loading = $state(true);
  let deepPending = $state<string[]>([]);
  let deepNote = $state('');
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
    deepPending = [];
    deepNote = '';

    if (!/^(CITY-)?[A-Z]{3}$/.test(o) || !/^(CITY-)?[A-Z]{3}$/.test(d) || !dt) {
      error = 'Choose two airports and a date.';
      loading = false;
      return;
    }

    let cancelled = false;
    const query: SearchQuery = {
      origin: o,
      destination: d,
      date: dt,
      adults: ad,
      cabin: cb,
      currency: 'USD',
      include_nearby: near,
      allow_synthetic: false
    };
    // Two-speed search: show the fast pass, then let the deep pass improve it.
    searchFares(query)
      .then((fast) => {
        if (cancelled) return;
        data = fast;
        loading = false;
        const pending = fast.search_debug?.pending_candidates ?? [];
        if (!pending.length) return;
        deepPending = pending;
        const before = fast.hidden_if_cheaper?.through_offer.price ?? null;
        const beforeCount = fast.hidden_city.length;
        return expandSearch(query, fast.search_debug?.expanded_destinations ?? [])
          .then((deep) => {
            if (cancelled) return;
            const merged = mergeSearch(fast, deep);
            data = merged;
            const after = merged.hidden_if_cheaper?.through_offer.price ?? null;
            const probed = deep.search_debug?.expanded_destinations?.length ?? pending.length;
            if (after != null && (before == null || after < before)) {
              deepNote = 'We found a cheaper option.';
            } else if (merged.hidden_city.length > beforeCount) {
              deepNote = `${merged.hidden_city.length - beforeCount} more hidden-city ticket(s) found.`;
            } else {
              deepNote = probed
                ? `Checked ${probed} more destination(s). Nothing cheaper.`
                : 'Deep search finished. Nothing cheaper.';
            }
          })
          .catch(() => {
            if (!cancelled) deepNote = 'Deep search did not finish. Results above are the fast pass.';
          })
          .finally(() => {
            if (!cancelled) deepPending = [];
          });
      })
      .catch((e: Error) => {
        if (!cancelled) {
          error = e.message;
          loading = false;
        }
      });

    return () => {
      cancelled = true;
    };
  });

  const names = $derived(data?.airline_names ?? {});
  const noShop = $derived(Boolean(data && data.cheapest_any == null && data.data_gaps.length));
  const lists = $derived(data?.channels.filter((c) => c.kind !== 'hidden-city') ?? []);
  const listedCount = $derived(lists.reduce((n, c) => n + c.offers.length, 0));
  const hiddenList = $derived(data?.hidden_city ?? []);
  const showHonest = $derived(
    Boolean(
      data?.honest_pick &&
        data.best_pick &&
        data.honest_pick.offer.id !== data.best_pick.offer.id
    )
  );
  const pick = $derived(
    data?.hidden_if_cheaper?.through_offer ?? data?.best_pick?.offer ?? data?.honest_pick?.offer ?? null
  );
  const confirmIds = ['google-flights', 'kayak', 'skyscanner', 'booking-com', 'expedia'];
  const confirms = $derived.by(() => {
    if (!data) return [];
    const source = data.hidden_if_cheaper?.bookers?.length
      ? data.hidden_if_cheaper.bookers
      : (data.bookers ?? []);
    const fromApi = source.filter((b) => confirmIds.includes(b.id));
    if (fromApi.length) return fromApi;
    const o = pick?.segments[0]?.origin || data.origin.iata;
    const d = pick?.segments[pick.segments.length - 1]?.dest || data.destination.iata;
    return itineraryBookers(
      o,
      d,
      data.query.date,
      data.query.adults,
      pick?.currency || data.query.currency,
      data.query.cabin
    );
  });
  const confirmNote = $derived.by(() => {
    if (!data) return '';
    const hidden = data.hidden_if_cheaper;
    if (!hidden) return 'Confirm this city-pair on another site. We do not copy their prices.';
    const ticketed = hidden.ticketed_destination || hidden.hidden_city;
    const origin = hidden.through_offer.segments[0]?.origin || data.origin.iata;
    return `These links search the ticketed trip ${origin} → ${ticketed}, not ${data.origin.iata} → ${data.destination.iata}. Hidden-city is the full itinerary.`;
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

    {#if deepPending.length}
      <p class="note deep-status" aria-live="polite">
        Fares below are live. Still checking {deepPending.length} more ticketed destination{deepPending.length === 1 ? '' : 's'}
        ({deepPending.join(', ')}) for a cheaper through-ticket…
      </p>
    {:else if deepNote}
      <p class="note deep-status" aria-live="polite">{deepNote}</p>
    {/if}

    {#if data.hidden_if_cheaper}
      <p class="pick-kicker">Best — cheapest hidden-city</p>
      <HiddenCityCard match={data.hidden_if_cheaper} names={names} />
    {/if}

    {#if data.best_pick || showHonest}
      <div class="picks-row">
        {#if data.best_pick}
          <div>
            <p class="pick-kicker">{data.best_pick.reason}</p>
            <OfferCard offer={data.best_pick.offer} featured names={names} />
          </div>
        {/if}
        {#if showHonest && data.honest_pick}
          <div>
            <p class="pick-kicker">{data.honest_pick.reason}</p>
            <OfferCard offer={data.honest_pick.offer} names={names} />
          </div>
        {/if}
      </div>
    {:else if data.honest_pick}
      <div class="picks-row">
        <div>
          <p class="pick-kicker">{data.honest_pick.reason}</p>
          <OfferCard offer={data.honest_pick.offer} featured names={names} />
        </div>
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
      <p class="note" style="margin:14px 0 8px">{confirmNote}</p>
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
        Hidden city {hiddenList.length ? `(${hiddenList.length})` : ''}
      </button>
      <button class:on={tab === 'live'} type="button" onclick={() => (tab = 'live')}>Live</button>
      <button class:on={tab === 'debug'} type="button" onclick={() => (tab = 'debug')}>Debug</button>
    </div>

    {#if tab === 'flights'}
      {#if listedCount}
        {#each lists as ch}
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
        <p class="empty">No priced flights on this city pair for that date.</p>
      {/if}
    {:else if tab === 'hidden'}
      <p class="note">
        A cheaper ticket that continues past your city. You would get off at your stop. Airlines prohibit this.
      </p>
      {#if hiddenList.length}
        {#each hiddenList as match}
          <HiddenCityCard {match} names={names} />
        {/each}
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
          {data.search_debug.standard_query} · Pass: {data.search_debug.mode ?? 'fast'} · Cache: {data.search_debug.cache}
          · Reused offers: {data.search_debug.reused_from_index ?? 0}
          · Paid supplier calls: {data.search_debug.provider_calls ?? 0}
          (${(data.search_debug.provider_cost_usd ?? 0).toFixed(3)})
        </p>
        {#if data.search_debug.expanded_destinations.length}
          <p class="note">Live expansion: {data.search_debug.expanded_destinations.join(', ')}</p>
        {/if}
        {#if data.search_debug.pending_candidates?.length}
          <p class="note">Deep pass will check: {data.search_debug.pending_candidates.join(', ')}</p>
        {/if}
        {#if data.search_debug.skipped_expansion?.length}
          <p class="note">Skipped (already indexed through your city): {data.search_debug.skipped_expansion.join(', ')}</p>
        {/if}
        {#if data.search_debug.candidates?.length}
          <h2 class="section-title">Candidate destinations</h2>
          <p class="note">
            Score = 0.30 connection + 0.25 savings probability + 0.20 expected saving + 0.10 freshness + 0.10 hub + 0.05 supplier.
            Learned from tickets we actually priced; nothing here is a summed leg price.
          </p>
          <table class="cand">
            <thead>
              <tr>
                <th>C</th><th>Score</th><th>Source</th><th>Checks</th><th>Via your city</th><th>Cheaper</th><th>Median saving</th><th>Shopped</th>
              </tr>
            </thead>
            <tbody>
              {#each data.search_debug.candidates as c}
                <tr class:on={c.selected}>
                  <td>{c.code}</td>
                  <td>{c.score.toFixed(3)}</td>
                  <td>{c.source}</td>
                  <td>{c.observations}</td>
                  <td>{c.successful_connections}</td>
                  <td>{c.cheaper_than_direct_count}</td>
                  <td>{c.median_saving ? c.median_saving.toFixed(0) : '—'}</td>
                  <td>{c.selected ? 'yes' : ''}</td>
                </tr>
              {/each}
            </tbody>
          </table>
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

<style>
  .deep-status {
    margin: 10px 0 4px;
    padding: 8px 12px;
    border: 1px dashed var(--line-2);
    border-radius: 10px;
  }
  .cand {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.85rem;
    margin: 8px 0 16px;
  }
  .cand th,
  .cand td {
    text-align: left;
    padding: 6px 8px;
    border-bottom: 1px solid var(--line);
  }
  .cand th {
    color: var(--muted);
    font-weight: 600;
  }
  .cand tr.on td {
    background: rgba(201, 163, 106, 0.14);
  }
</style>
