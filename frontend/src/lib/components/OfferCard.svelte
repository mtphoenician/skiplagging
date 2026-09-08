<script lang="ts">
  import { airlineName, duration, hm, itineraryBookers, money, outboundEndIndex, plusDays, stopovers } from '$lib/api';
  import type { Cabin, Offer } from '$lib/types';

  let {
    offer,
    featured = false,
    cheapest = false,
    fastest = false,
    best = false,
    tag = '',
    names = {},
    fromCity = '',
    toCity = '',
    adults = 1,
    cabin = 'ECONOMY'
  }: {
    offer: Offer;
    featured?: boolean;
    cheapest?: boolean;
    fastest?: boolean;
    best?: boolean;
    tag?: string;
    names?: Record<string, string>;
    fromCity?: string;
    toCity?: string;
    adults?: number;
    cabin?: Cabin;
  } = $props();

  const first = $derived(offer.segments[0]);
  const outEnd = $derived(outboundEndIndex(offer));
  const destSeg = $derived(offer.segments[outEnd]);
  const outboundSegs = $derived(offer.segments.slice(0, outEnd + 1));
  const inboundSegs = $derived(offer.segments.slice(outEnd + 1));
  const days = $derived(first && destSeg ? plusDays(first.dep, destSeg.arr) : '');
  const waits = $derived(stopovers(offer));
  const inboundWaits = $derived(
    inboundSegs.length > 1 ? stopovers(offer, outEnd + 1, offer.segments.length - 1) : []
  );
  const outMins = $derived(outboundSegs.reduce((n, s) => n + (s.duration_min || 0), 0));
  const inMins = $derived(inboundSegs.reduce((n, s) => n + (s.duration_min || 0), 0));
  const shownOutMins = $derived.by(() => {
    if (first && destSeg) {
      const a = Date.parse(first.dep);
      const b = Date.parse(destSeg.arr);
      if (!Number.isNaN(a) && !Number.isNaN(b) && b > a) return Math.round((b - a) / 60000);
    }
    return inboundSegs.length ? outMins : offer.duration_min;
  });
  const shownInMins = $derived.by(() => {
    if (!inboundSegs.length) return 0;
    const a0 = inboundSegs[0];
    const a1 = inboundSegs[inboundSegs.length - 1];
    const a = Date.parse(a0.dep);
    const b = Date.parse(a1.arr);
    if (!Number.isNaN(a) && !Number.isNaN(b) && b > a) return Math.round((b - a) / 60000);
    return inMins;
  });
  const isSelf = $derived(offer.kind === 'self-transfer');
  const carriers = $derived.by(() => {
    const codes: string[] = [];
    for (const s of offer.segments) {
      const c = (s.carrier || '').toUpperCase();
      if (c && !codes.includes(c)) codes.push(c);
    }
    return codes.map((c) => airlineName(c, names)).filter(Boolean);
  });
  const fromLabel = $derived(
    first ? (fromCity ? `${first.origin} ${fromCity}` : first.origin) : ''
  );
  const toLabel = $derived(destSeg ? (toCity ? `${destSeg.dest} ${toCity}` : destSeg.dest) : '');
  const tickets = $derived(offer.separate_tickets ?? []);
</script>

<article class="offer" class:pick={featured}>
  <div class="row">
    <div class="offer-main">
      {#if isSelf || cheapest || fastest || best || tag || offer.return_date}
        <div class="chips">
          {#if cheapest}<span class="chip good">Cheapest</span>{/if}
          {#if best}<span class="chip good">Best</span>{/if}
          {#if fastest}<span class="chip">Fastest</span>{/if}
          {#if offer.return_date}<span class="chip">Round trip</span>{/if}
          {#if isSelf}<span class="chip hot">Self-transfer hack</span>{/if}
          {#if tag}<span class="chip">{tag}</span>{/if}
        </div>
      {/if}
      {#if carriers.length}
        <p class="airline-name">{carriers.join(', ')}</p>
      {/if}
      {#if first && destSeg}
        <p class="flight-times">
          {hm(first.dep)} – {hm(destSeg.arr)}{#if days}<sup class="day-plus">{days}</sup>{/if}
        </p>
      {/if}
      <p class="flight-path">{fromLabel} – {toLabel}</p>
      {#if outboundSegs.length}
        {#if waits.length}
          <p class="stops-count">{offer.stops} stop{offer.stops === 1 ? '' : 's'}{inboundSegs.length ? ' outbound' : ''}</p>
          <div class="legs">
            {#each waits as stop}
              <span class:xfer={stop.selfTransfer}>
                {stop.code}
                {#if stop.minutes}
                  {duration(stop.minutes)}
                  {stop.selfTransfer ? 'self-transfer' : 'stopover'}
                {/if}
              </span>
            {/each}
          </div>
        {/if}
        <div class="legs flights">
          {#each outboundSegs as s}
            <span>{s.flight_number} {s.origin}→{s.dest} {hm(s.dep)}–{hm(s.arr)}</span>
          {/each}
        </div>
      {/if}
      {#if inboundSegs.length}
        <p class="stops-count">Return {offer.return_date}</p>
        {#if inboundWaits.length}
          <div class="legs">
            {#each inboundWaits as stop}
              <span class:xfer={stop.selfTransfer}>
                {stop.code}
                {#if stop.minutes}
                  {duration(stop.minutes)}
                  {stop.selfTransfer ? 'self-transfer' : 'stopover'}
                {/if}
              </span>
            {/each}
          </div>
        {/if}
        <div class="legs flights">
          {#each inboundSegs as s}
            <span>{s.flight_number} {s.origin}→{s.dest} {hm(s.dep)}–{hm(s.arr)}</span>
          {/each}
        </div>
      {/if}
      <div class="seg" style="margin-top:8px">
        <span>{duration(shownOutMins)}</span>
        {#if inboundSegs.length && shownInMins}
          <span>Return {duration(shownInMins)}</span>
        {/if}
        {#if outboundSegs.length === 1}
          <span>Nonstop</span>
        {/if}
      </div>
      {#if isSelf && offer.note}
        <p class="note" style="margin:8px 0 0">{offer.note}</p>
      {/if}
      {#if isSelf && tickets.length}
        <div class="ticket-buys">
          {#each tickets as t, i}
            <div class="ticket-buy">
              <span>Ticket {i + 1}: {t.origin}→{t.dest} {money(t.price, t.currency)}</span>
              {#if t.date}
                <div class="book-row">
                  {#each itineraryBookers(t.origin, t.dest, t.date, adults, t.currency || offer.currency, cabin).slice(0, 3) as b}
                    <a class="book-btn" href={b.url} target="_blank" rel="noopener noreferrer">{b.name}</a>
                  {/each}
                </div>
              {/if}
            </div>
          {/each}
        </div>
      {/if}
    </div>
    <div class="price">{money(offer.price, offer.currency)}</div>
  </div>
</article>

<style>
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin: 0 0 8px;
  }
  .day-plus {
    font-size: 0.55em;
    font-weight: 700;
    margin-left: 2px;
  }
  .stops-count {
    margin: 8px 0 0;
    font-size: 0.92rem;
    font-weight: 650;
  }
  .legs span.xfer {
    background: #fff1e0;
    color: #c45c12;
    padding: 2px 7px;
    border-radius: 6px;
    font-weight: 700;
  }
  .legs.flights {
    color: var(--muted);
    font-size: 0.82rem;
  }
  .ticket-buys {
    margin-top: 10px;
    display: grid;
    gap: 8px;
  }
  .ticket-buy {
    font-size: 0.85rem;
  }
  .ticket-buy .book-row {
    margin-top: 4px;
  }
</style>
