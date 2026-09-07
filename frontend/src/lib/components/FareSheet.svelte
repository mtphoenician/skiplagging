<script lang="ts">
  import { money } from '$lib/api';
  import type { BookerLink, Offer } from '$lib/types';

  const CONFIRM = new Set(['google-flights', 'kayak', 'skyscanner', 'booking-com', 'expedia']);
  const SKIP_CARRIER = new Set(['ZZ', 'XX', 'YY']);

  let {
    offer,
    quotes,
    bookers,
    date
  }: {
    offer: Offer;
    quotes: Offer[];
    bookers: BookerLink[];
    date: string;
  } = $props();

  const priced = $derived(
    [...quotes]
      .filter((q) => q.price != null && q.currency === offer.currency)
      .sort((a, b) => (a.price || 0) - (b.price || 0))
  );
  const best = $derived(priced[0]);
  const confirms = $derived(
    bookers.filter((b) => {
      if (b.id.startsWith('carrier-')) {
        const code = b.id.slice(8);
        return !SKIP_CARRIER.has(code);
      }
      return CONFIRM.has(b.id);
    })
  );
  const flightQ = $derived(
    encodeURIComponent(
      `flights from ${offer.segments[0]?.origin || ''} to ${offer.segments[offer.segments.length - 1]?.dest || ''} on ${date} one way ${offer.first_flight}`
    )
  );
  const precise = $derived(`https://www.google.com/travel/flights?q=${flightQ}&hl=en&gl=us&curr=USD`);

  function label(source: string) {
    if (source === 'duffel') return 'Duffel (NDC shop)';
    if (source === 'amadeus') return 'Amadeus (GDS shop)';
    return source;
  }
</script>

<section class="fare-sheet">
  <p class="pick-kicker">Where this itinerary is priced</p>
  {#if priced.length}
    <ul class="quote-list">
      {#each priced as q, i}
        <li class:best={i === 0}>
          <div>
            <strong>{label(q.source)}</strong>
            <span class="note">{q.validating_airline && !SKIP_CARRIER.has(q.validating_airline) ? q.validating_airline : q.carrier} · {q.channel}</span>
          </div>
          <div class="quote-price">
            {#if best && q.id !== best.id && q.price != null && best.price != null && q.price > best.price}
              <span class="price-was">{money(q.price, q.currency)}</span>
            {/if}
            <b>{money(q.price, q.currency)}</b>
            {#if i === 0}
              <small>Lowest shopped fare</small>
            {/if}
          </div>
        </li>
      {/each}
    </ul>
  {:else}
    <p class="note">No licensed shop returned a fare for this exact itinerary.</p>
  {/if}

  {#if confirms.length}
    <p class="note" style="margin:14px 0 8px">
      Confirm the same flights on a booker. Those sites do not give this app a price feed, so they appear only as a
      hand-off — not as a number we invented.
    </p>
    <div class="book-row">
      <a class="book-btn" href={precise} target="_blank" rel="noreferrer">Google Flights (this flight)</a>
      {#each confirms.filter((b) => b.id !== 'google-flights') as b}
        <a class="book-btn" href={b.url} target="_blank" rel="noreferrer">{b.name}</a>
      {/each}
    </div>
  {/if}
</section>
