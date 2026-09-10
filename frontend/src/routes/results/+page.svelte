<script lang="ts">
  import { page } from '$app/state';
  import HiddenCityCard from '$lib/components/HiddenCityCard.svelte';
  import OfferCard from '$lib/components/OfferCard.svelte';
  import SearchForm from '$lib/components/SearchForm.svelte';
  import TrafficPanel from '$lib/components/TrafficPanel.svelte';
  import { duration, expandSearch, itineraryBookers, mergeSearch, money, outboundDest, searchFares } from '$lib/api';
  import type { Cabin, Offer, SearchQuery, SearchResponse } from '$lib/types';

  const params = $derived(page.url.searchParams);
  let data = $state<SearchResponse | null>(null);
  let error = $state('');
  let loading = $state(true);
  let deepPending = $state<string[]>([]);
  let deepSelf = $state(false);
  let deepNote = $state('');
  let tab = $state<'flights' | 'hidden' | 'live' | 'debug'>('flights');
  let sort = $state<'cheapest' | 'best' | 'fastest'>('cheapest');
  let maxStops = $state<number>(-1);

  let origin = $state('');
  let destination = $state('');
  let date = $state('');
  let return_date = $state('');
  let adults = $state(1);
  let cabin = $state<Cabin>('ECONOMY');
  let include_nearby = $state(true);
  let rechecking = $state(false);
  let run: { cancel: boolean } | null = null;

  function cacheNote(raw: string): string {
    if (raw.startsWith('http-cache')) {
      return 'Same search returned from memory. Recheck to shop again.';
    }
    if (raw === 'index') return 'Priced from the offer index this request.';
    if (raw === 'index+live') return 'Offer index plus a live shop this request.';
    if (raw === 'miss') return 'Live shop this request.';
    return '';
  }

  function searchQuery(): SearchQuery {
    return {
      origin,
      destination,
      date,
      return_date: return_date || null,
      adults,
      cabin,
      currency: 'USD',
      include_nearby,
      allow_synthetic: false
    };
  }

  function beginSearch(query: SearchQuery, bypass: boolean) {
    if (run) run.cancel = true;
    const thisRun = { cancel: false };
    run = thisRun;
    if (bypass) {
      rechecking = true;
    } else {
      loading = true;
      error = '';
      data = null;
      deepPending = [];
      deepSelf = false;
      deepNote = '';
    }

    searchFares(query, { bypassCache: bypass })
      .then((fast) => {
        if (thisRun.cancel) return;
        data = fast;
        loading = false;
        const pending = fast.search_debug?.pending_candidates ?? [];
        const pendingSelf = Boolean(fast.search_debug?.pending_self_transfer);
        if (!pending.length && !pendingSelf) {
          rechecking = false;
          return;
        }
        deepPending = pending;
        deepSelf = pendingSelf && !pending.length;
        const before = fast.hidden_if_cheaper?.through_offer.price ?? null;
        const beforeCount = fast.hidden_city.length;
        const beforeSelf = fast.channels.find((c) => c.kind === 'self-transfer')?.offers[0]?.price ?? null;
        return expandSearch(query, fast.search_debug?.expanded_destinations ?? [])
          .then((deep) => {
            if (thisRun.cancel) return;
            const merged = mergeSearch(fast, deep);
            data = merged;
            const after = merged.hidden_if_cheaper?.through_offer.price ?? null;
            const probed = deep.search_debug?.expanded_destinations?.length ?? pending.length;
            const afterSelf = merged.channels.find((c) => c.kind === 'self-transfer')?.offers[0]?.price ?? null;
            if (after != null && (before == null || after < before)) {
              deepNote = 'We found a cheaper option.';
            } else if (afterSelf != null && (beforeSelf == null || afterSelf < beforeSelf)) {
              deepNote = 'We found a cheaper self-transfer (separate tickets).';
            } else if (merged.hidden_city.length > beforeCount) {
              deepNote = `${merged.hidden_city.length - beforeCount} more hidden-city ticket(s) found.`;
            } else {
              deepNote = probed
                ? `Checked ${probed} more destination(s). Nothing cheaper.`
                : 'Deep search finished. Nothing cheaper.';
            }
          })
          .catch(() => {
            if (!thisRun.cancel) deepNote = 'Deep search did not finish. Results above are the fast pass.';
          })
          .finally(() => {
            if (!thisRun.cancel) {
              deepPending = [];
              deepSelf = false;
              rechecking = false;
            }
          });
      })
      .catch((e: Error) => {
        if (!thisRun.cancel) {
          error = e.message;
          loading = false;
          rechecking = false;
        }
      });
  }

  function recheckSearch() {
    if (loading || rechecking) return;
    beginSearch(data?.query ?? searchQuery(), true);
  }

  $effect(() => {
    const o = (params.get('origin') || '').toUpperCase();
    const d = (params.get('destination') || '').toUpperCase();
    const dt = params.get('date') || '';
    const ret = params.get('return') || '';
    const ad = Number(params.get('adults') || 1);
    const cb = (params.get('cabin') || 'ECONOMY') as Cabin;
    const near = params.get('nearby') !== '0';

    origin = o;
    destination = d;
    date = dt;
    return_date = ret;
    adults = ad;
    cabin = cb;
    include_nearby = near;

    if (!/^(CITY-)?[A-Z]{3}$/.test(o) || !/^(CITY-)?[A-Z]{3}$/.test(d) || !dt) {
      if (run) run.cancel = true;
      error = 'Choose two airports and a date.';
      loading = false;
      data = null;
      return;
    }

    beginSearch(
      {
        origin: o,
        destination: d,
        date: dt,
        return_date: ret || null,
        adults: ad,
        cabin: cb,
        currency: 'USD',
        include_nearby: near,
        allow_synthetic: false
      },
      false
    );

    return () => {
      if (run) run.cancel = true;
    };
  });

  const names = $derived(data?.airline_names ?? {});
  const noShop = $derived(
    Boolean(
      data &&
        data.cheapest_any == null &&
        data.data_gaps.some((g) => g.toLowerCase().includes('no duffel token') || g.toLowerCase().includes('mock is off'))
    )
  );

  function isLiveLayerGap(gap: string): boolean {
    const l = gap.toLowerCase();
    return (
      l.includes('opensky') ||
      l.includes('rapidapi') ||
      l.includes('aerodatabox') ||
      l.includes('fids') ||
      l.includes('live transponder') ||
      l.includes('live aircraft') ||
      l.includes('live positions') ||
      l.includes('tracker links')
    );
  }

  function isSandboxGap(gap: string): boolean {
    const l = gap.toLowerCase();
    return l.includes('duffel_test') || l.includes('your duffel key is sandbox');
  }

  function isDuffelFailedGap(gap: string): boolean {
    return gap.toLowerCase().includes('duffel shop failed');
  }

  function isEmptyMarketGap(gap: string): boolean {
    const l = gap.toLowerCase();
    return l.includes('no priced offers') || l.includes('not a live empty market');
  }

  const shopGaps = $derived((data?.data_gaps ?? []).filter((g) => g.trim() && !isLiveLayerGap(g)));
  const liveGaps = $derived((data?.data_gaps ?? []).filter((g) => isLiveLayerGap(g)));
  const sandboxGap = $derived(shopGaps.find(isSandboxGap) || '');
  const duffelFailedGap = $derived(shopGaps.find(isDuffelFailedGap) || '');
  const shopNotes = $derived(shopGaps.filter((g) => g !== sandboxGap && g !== duffelFailedGap));
  const roundTrip = $derived(Boolean(data?.query.return_date));
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
  // Page-level booker links always search the honest city pair A → B. The
  // hidden-city card carries its own links to the ticketed A → C trip.
  const confirms = $derived.by(() => {
    if (!data) return [];
    const fromApi = (data.bookers ?? []).filter((b) => !b.id.startsWith('carrier-')).slice(0, 5);
    if (fromApi.length) return fromApi;
    return itineraryBookers(
      data.origin.iata,
      data.destination.iata,
      data.query.date,
      data.query.adults,
      data.honest_pick?.offer.currency || data.query.currency,
      data.query.cabin,
      data.query.return_date
    );
  });
  const confirmNote = $derived(
    data
      ? `Compare ${data.origin.iata} → ${data.destination.iata} on other sites. We do not copy their prices.`
      : ''
  );
  const selfTransfers = $derived(data?.channels.find((c) => c.kind === 'self-transfer')?.offers ?? []);
  const cheapestSelf = $derived(selfTransfers[0] ?? null);
  const showSelf = $derived.by(() => {
    const self = cheapestSelf;
    if (self?.price == null) return false;
    const honest = data?.honest_pick?.offer.price;
    return honest == null || self.price < honest;
  });
  const hasPriced = $derived(
    Boolean(data?.honest_pick || data?.best_pick || data?.hidden_if_cheaper || listedCount || showSelf)
  );
  const emptyFlightsReason = $derived.by(() => {
    if (!data || hasPriced) return '';
    if (noShop) return 'No priced flights here — this app is not connected to a fare shop.';
    if (duffelFailedGap) return duffelFailedGap;
    const empty = shopGaps.find(isEmptyMarketGap);
    if (empty) return empty;
    if (sandboxGap) {
      return 'No live fares in this result. The Duffel key is sandbox; live_mode=false offers are dropped.';
    }
    return 'No priced flight on this city pair for that date. Try another date or nearby airports.';
  });

  const hiddenSaving = $derived.by(() => {
    const h = data?.hidden_if_cheaper;
    const honest = data?.honest_pick?.offer.price;
    if (!h || honest == null || h.through_offer.price == null) return 0;
    return honest - h.through_offer.price;
  });

  // Comparator list: every priced flight to B in one list, like a metasearch.
  // Hidden-city tickets are not in here; they are a separate, labeled addition.
  const allOffers = $derived.by(() => {
    const seen = new Set<string>();
    const out: Offer[] = [];
    for (const ch of lists) {
      for (const o of ch.offers) {
        if (o.price == null || seen.has(o.id)) continue;
        seen.add(o.id);
        out.push(o);
      }
    }
    return out;
  });
  const minPrice = $derived(Math.min(...allOffers.map((o) => o.price ?? Infinity)));
  const minDuration = $derived(Math.min(...allOffers.map((o) => o.duration_min || Infinity)));
  const shown = $derived.by(() => {
    const rows = allOffers.filter((o) => maxStops < 0 || o.stops <= maxStops);
    const by: Record<typeof sort, (a: Offer, b: Offer) => number> = {
      cheapest: (a, b) => (a.price ?? 1e9) - (b.price ?? 1e9) || a.duration_min - b.duration_min,
      fastest: (a, b) => a.duration_min - b.duration_min || (a.price ?? 1e9) - (b.price ?? 1e9),
      best: (a, b) => a.stops - b.stops || (a.price ?? 1e9) - (b.price ?? 1e9) || a.duration_min - b.duration_min
    };
    return [...rows].sort(by[sort]);
  });
  const bestId = $derived(
    allOffers.length
      ? [...allOffers].sort(
          (a, b) => a.stops - b.stops || (a.price ?? 1e9) - (b.price ?? 1e9) || a.duration_min - b.duration_min
        )[0].id
      : ''
  );
  const stopCounts = $derived({
    nonstop: allOffers.filter((o) => o.stops === 0).length,
    one: allOffers.filter((o) => o.stops <= 1).length
  });

  function tagFor(o: Offer): string {
    if (o.kind === 'nearby') return 'Nearby airport';
    return '';
  }

  function cities(offer: Offer): { fromCity: string; toCity: string } {
    if (!data) return { fromCity: '', toCity: '' };
    const dest = outboundDest(offer);
    return {
      fromCity: offer.segments[0]?.origin === data.origin.iata ? data.origin.city : '',
      toCity: dest === data.destination.iata ? data.destination.city : ''
    };
  }
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
    bind:return_date
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
      <div class="results-head-row">
        <div>
          <h1>{data.origin.city} → {data.destination.city}</h1>
          <p class="meta">
            {data.origin.type === 'city' ? `${data.origin.city} (all)` : data.origin.iata}–{data.destination.type === 'city'
              ? `${data.destination.city} (all)`
              : data.destination.iata} · {data.query.date}{data.query.return_date ? `–${data.query.return_date}` : ''}
            · {data.query.adults} adult{data.query.adults === 1 ? '' : 's'}
          </p>
        </div>
        <button
          class="ghost"
          type="button"
          onclick={recheckSearch}
          disabled={rechecking || deepPending.length > 0 || deepSelf}
        >
          {rechecking ? 'Rechecking…' : 'Recheck this search'}
        </button>
      </div>
      {#if data.search_debug?.cache}
        <p class="meta cache-line">
          <span class="chip">{data.search_debug.cache}</span>
          {cacheNote(data.search_debug.cache)}
        </p>
      {/if}
    </header>

    {#if sandboxGap}
      <p class="banner">{sandboxGap}</p>
    {:else if noShop}
      <p class="banner">
        No fare-shop keys are configured, so this app cannot price a ticket.
      </p>
    {:else if duffelFailedGap}
      <p class="banner">{duffelFailedGap}</p>
    {/if}
    {#if shopNotes.length}
      <div class="shop-gaps">
        {#each shopNotes as gap}
          <p class="note">{gap}</p>
        {/each}
      </div>
    {/if}

    {#if deepPending.length}
      <p class="note deep-status" aria-live="polite">
        Fares below are live. Still checking {deepPending.length} more ticketed destination{deepPending.length === 1 ? '' : 's'}
        ({deepPending.join(', ')}) for a cheaper through-ticket…
      </p>
    {:else if deepSelf}
      <p class="note deep-status" aria-live="polite">
        Fares below are live. Still checking extra self-transfer connections…
      </p>
    {:else if deepNote}
      <p class="note deep-status" aria-live="polite">{deepNote}</p>
    {/if}

    <!-- Comparator first: the cheapest and best regular tickets to B. -->
    {#if data.honest_pick || data.best_pick}
      <div class="picks-row">
        {#if data.honest_pick}
          <div>
            <p class="pick-kicker">{data.honest_pick.reason}</p>
            <OfferCard
              offer={data.honest_pick.offer}
              featured
              cheapest={!showSelf}
              names={names}
              adults={data.query.adults}
              cabin={data.query.cabin}
              {...cities(data.honest_pick.offer)}
            />
          </div>
        {/if}
        {#if showHonest && data.best_pick}
          <div>
            <p class="pick-kicker">{data.best_pick.reason}</p>
            <OfferCard
              offer={data.best_pick.offer}
              best
              names={names}
              adults={data.query.adults}
              cabin={data.query.cabin}
              {...cities(data.best_pick.offer)}
            />
          </div>
        {:else if !data.honest_pick && data.best_pick}
          <div>
            <p class="pick-kicker">{data.best_pick.reason}</p>
            <OfferCard
              offer={data.best_pick.offer}
              featured
              best
              names={names}
              adults={data.query.adults}
              cabin={data.query.cabin}
              {...cities(data.best_pick.offer)}
            />
          </div>
        {/if}
      </div>
    {/if}

    {#if showSelf && cheapestSelf}
      <p class="pick-kicker">Cheaper with separate tickets — self-transfer</p>
      <OfferCard
        offer={cheapestSelf}
        cheapest
        names={names}
        adults={data.query.adults}
        cabin={data.query.cabin}
        {...cities(cheapestSelf)}
      />
    {/if}

    <!-- Addition: a hidden-city ticket, only when one exists and it saves money. -->
    {#if data.hidden_if_cheaper}
      <p class="pick-kicker">
        Also: hidden-city ticket{hiddenSaving > 0 ? ` — saves ${money(hiddenSaving, data.hidden_if_cheaper.currency)}` : ''}
      </p>
      <HiddenCityCard match={data.hidden_if_cheaper} names={names} />
    {/if}

    {#if !hasPriced && emptyFlightsReason && !sandboxGap && !noShop && !duffelFailedGap && !shopGaps.some(isEmptyMarketGap)}
      <p class="empty">{emptyFlightsReason}</p>
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
      {#if allOffers.length}
        <div class="toolbar">
          <div class="seg-ctl" role="group" aria-label="Sort">
            <button class:on={sort === 'cheapest'} type="button" onclick={() => (sort = 'cheapest')}>
              Cheapest {money(minPrice, allOffers[0].currency)}
            </button>
            <button class:on={sort === 'best'} type="button" onclick={() => (sort = 'best')}>Best</button>
            <button class:on={sort === 'fastest'} type="button" onclick={() => (sort = 'fastest')}>
              Fastest {duration(minDuration)}
            </button>
          </div>
          <div class="seg-ctl" role="group" aria-label="Stops">
            <button class:on={maxStops < 0} type="button" onclick={() => (maxStops = -1)}>Any stops</button>
            <button class:on={maxStops === 0} type="button" disabled={!stopCounts.nonstop} onclick={() => (maxStops = 0)}>
              Nonstop
            </button>
            <button class:on={maxStops === 1} type="button" disabled={!stopCounts.one} onclick={() => (maxStops = 1)}>
              ≤ 1 stop
            </button>
          </div>
        </div>
        {#if shown.length}
          {#each shown as offer (offer.id)}
            <OfferCard
              {offer}
              cheapest={offer.price === minPrice}
              fastest={offer.duration_min === minDuration}
              best={offer.id === bestId}
              tag={tagFor(offer)}
              names={names}
              adults={data.query.adults}
              cabin={data.query.cabin}
              {...cities(offer)}
            />
          {/each}
        {:else}
          <p class="empty">No flight matches that stop filter.</p>
        {/if}
      {:else}
        <p class="empty">{emptyFlightsReason || 'No priced flights on this city pair for that date.'}</p>
      {/if}
    {:else if tab === 'hidden'}
      <p class="note">
        {#if roundTrip}
          Hidden-city is one-way only. This search is comparing honest round-trip tickets.
        {:else}
          A cheaper ticket that continues past your city. You would get off at your stop. Airlines prohibit this.
        {/if}
      </p>
      {#if hiddenList.length}
        {#each hiddenList as match}
          <HiddenCityCard {match} names={names} />
        {/each}
      {:else}
        <p class="empty">
          {roundTrip
            ? 'No hidden-city row on a round-trip search.'
            : 'No through-ticket cheaper than the cheapest honest fare on this date.'}
        </p>
      {/if}
    {:else if tab === 'live'}
      {#if liveGaps.length}
        <div class="shop-gaps">
          {#each liveGaps as gap}
            <p class="note">{gap}</p>
          {/each}
        </div>
      {/if}
      <TrafficPanel traffic={data.traffic_origin} label={data.origin.city} />
      <TrafficPanel traffic={data.traffic_destination} label={data.destination.city} />
      {#if data.board_origin?.length}
        <h2 class="section-title">FIDS board ({data.origin.iata})</h2>
        <p class="note">Scheduled / estimated / actual — not a fare.</p>
        <table class="mini">
          <thead>
            <tr>
              <th>Flight</th>
              <th>To</th>
              <th>Scheduled</th>
              <th>Status</th>
            </tr>
          </thead>
          <tbody>
            {#each data.board_origin.slice(0, 12) as f}
              <tr>
                <td class="mono">{f.flight_number}</td>
                <td>{f.dest || '—'}</td>
                <td>{f.scheduled || f.estimated || '—'}</td>
                <td>{f.status || '—'}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
    {:else if tab === 'debug'}
      <p class="note">
        Hidden-city means a complete ticket continues past your city. We never turn that ticket into a fake local fare.
      </p>
      {#if data.data_gaps.length}
        <h2 class="section-title">Shop notes (data_gaps)</h2>
        {#each data.data_gaps as gap}
          <p class="note">{gap}</p>
        {/each}
      {/if}
      {#if data.search_debug}
        <h2 class="section-title">Search planner</h2>
        <p class="note">
          Providers: {data.search_debug.providers.join(', ') || 'none'} · Standard query:
          {data.search_debug.standard_query} · Pass: {data.search_debug.mode ?? 'fast'} · Cache: {data.search_debug.cache}
          · Reused offers: {data.search_debug.reused_from_index ?? 0}
          {data.search_debug.reused_honest ? ' · Honest A→B reused (<60s live index)' : ''}
          {data.search_debug.fx_live === false ? ' · FX unverified; non-USD omitted' : ''}
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
            stats / edges / index are tickets this app priced. openflights is a frozen 2014–2017 prior, not a timetable.
          </p>
          <div class="table-scroll">
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
          </div>
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
  .results-head-row {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px 16px;
  }
  .cache-line {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin: 8px 0 0;
  }
  .toolbar {
    display: flex;
    flex-wrap: wrap;
    gap: 10px 18px;
    margin: 4px 0 10px;
  }
  .seg-ctl {
    display: inline-flex;
    max-width: 100%;
    border: 1px solid var(--line);
    border-radius: 999px;
    overflow-x: auto;
    -webkit-overflow-scrolling: touch;
    scrollbar-width: none;
    background: var(--surface);
  }
  .seg-ctl::-webkit-scrollbar {
    display: none;
  }
  .seg-ctl button {
    flex: 0 0 auto;
    border: 0;
    background: transparent;
    color: var(--muted);
    padding: 7px 12px;
    font-size: 0.85rem;
    font-weight: 600;
    white-space: nowrap;
  }
  .seg-ctl button.on {
    background: var(--ink);
    color: #fff;
  }
  .seg-ctl button:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .deep-status {
    margin: 10px 0 4px;
    padding: 8px 12px;
    border: 1px dashed var(--line-2);
    border-radius: 10px;
  }
  .shop-gaps {
    margin: 8px 0 6px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .shop-gaps .note {
    margin: 0;
    padding: 8px 12px;
    border: 1px solid var(--line);
    border-radius: 10px;
    background: var(--surface);
  }
  .cand {
    width: 100%;
    min-width: 640px;
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
