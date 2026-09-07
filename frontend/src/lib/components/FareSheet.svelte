<script lang="ts">
  import { airlineName, itineraryBookers, money } from '$lib/api';
  import type { Offer } from '$lib/types';

  const SKIP_CARRIER = new Set(['ZZ', 'XX', 'YY']);

  let {
    offer,
    routeOffers = [],
    names = {},
    date,
    adults = 1,
    onPick
  }: {
    offer: Offer;
    routeOffers?: Offer[];
    names?: Record<string, string>;
    date: string;
    adults?: number;
    onPick?: (offer: Offer) => void;
  } = $props();

  const origin = $derived(offer.segments[0]?.origin || '');
  const dest = $derived(offer.segments[offer.segments.length - 1]?.dest || '');
  const confirms = $derived(itineraryBookers(origin, dest, date, adults, offer.currency));
  const airlineFares = $derived(
    [
      ...routeOffers
        .filter((o) => o.price != null && o.currency === offer.currency && o.carrier && !SKIP_CARRIER.has(o.carrier))
        .reduce((map, o) => {
          const prev = map.get(o.carrier);
          if (!prev || (o.price ?? 1e12) < (prev.price ?? 1e12)) map.set(o.carrier, o);
          return map;
        }, new Map<string, Offer>())
        .entries()
    ].sort((a, b) => (a[1].price || 0) - (b[1].price || 0))
  );

  function stopsLabel(o: Offer) {
    return o.stops === 0 ? 'Nonstop' : `${o.stops} stop${o.stops === 1 ? '' : 's'}`;
  }
</script>

<section class="fare-sheet">
  <p class="pick-kicker">Prices by airline</p>
  <p class="note" style="margin:6px 0 0">
    The card above is the recommended ticket. Each row below is that airline’s cheapest fare on this route.
  </p>
  {#if airlineFares.length}
    <ul class="quote-list">
      {#each airlineFares as [code, o], i}
        <li class:best={i === 0} class:on={o.id === offer.id || o.carrier === offer.carrier}>
          <button class="quote-hit" type="button" onclick={() => onPick?.(o)}>
            <div>
              <strong>{airlineName(code, names)}</strong>
              <span class="note">{stopsLabel(o)} · {o.first_flight}</span>
            </div>
            <div class="quote-price">
              <b>{money(o.price, o.currency)}</b>
              {#if i === 0}
                <small>Cheapest</small>
              {/if}
            </div>
          </button>
        </li>
      {/each}
    </ul>
  {:else}
    <p class="note">No shopped fares for this route yet.</p>
  {/if}

  {#if confirms.length && origin && dest}
    <p class="note" style="margin:14px 0 8px">
      Confirm {origin}–{dest} on another site. We do not copy prices from those pages.
    </p>
    <div class="book-row">
      {#each confirms as b}
        <a class="book-btn" href={b.url} target="_blank" rel="noopener noreferrer">{b.name}</a>
      {/each}
    </div>
  {/if}
</section>
