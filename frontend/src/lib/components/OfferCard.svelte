<script lang="ts">
  import { airlineName, duration, hm, layoverMinutes, money } from '$lib/api';
  import type { Offer } from '$lib/types';

  let {
    offer,
    featured = false,
    names = {}
  }: {
    offer: Offer;
    featured?: boolean;
    names?: Record<string, string>;
  } = $props();
  const wait = $derived(layoverMinutes(offer));
  const carrier = $derived(airlineName(offer.carrier, names));
  const first = $derived(offer.segments[0]);
  const last = $derived(offer.segments[offer.segments.length - 1]);
  const path = $derived(offer.segments.map((s) => s.origin).concat(last ? [last.dest] : []).join(' → '));
</script>

<article class="offer" class:pick={featured}>
  <div class="row">
    <div class="offer-main">
      {#if carrier}
        <p class="airline-name">{carrier}</p>
      {/if}
      {#if first && last}
        <p class="flight-times">{hm(first.dep)} → {hm(last.arr)}</p>
      {/if}
      <p class="flight-path">{path}</p>
      {#if offer.stops > 0}
        <div class="legs">
          {#each offer.segments as s}
            <span>{s.origin}→{s.dest} {hm(s.dep)}–{hm(s.arr)}</span>
          {/each}
        </div>
      {/if}
      <div class="seg" style="margin-top:8px">
        <span>{duration(offer.duration_min)}</span>
        <span>{offer.stops === 0 ? 'Nonstop' : `${offer.stops} stop${offer.stops === 1 ? '' : 's'}`}</span>
        {#if wait}
          <span>{duration(wait)} layover</span>
        {/if}
        <span>{offer.first_flight}</span>
      </div>
    </div>
    <div class="price">{money(offer.price, offer.currency)}</div>
  </div>
</article>
