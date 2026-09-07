<script lang="ts">
  import { duration, hm, layoverMinutes, money } from '$lib/api';
  import type { Offer } from '$lib/types';

  let {
    offer,
    featured = false,
    selected = false,
    onclick
  }: {
    offer: Offer;
    featured?: boolean;
    selected?: boolean;
    onclick?: () => void;
  } = $props();
  const wait = $derived(layoverMinutes(offer));
</script>

<article class="offer" class:pick={featured} class:selected>
  <button class="offer-hit" type="button" {onclick}>
    <div class="row">
      <div>
        <div class="seg">
          {#each offer.segments as s}
            <span>
              <strong>{s.flight_number}</strong>
              {s.origin}→{s.dest}
              {hm(s.dep)}–{hm(s.arr)}
            </span>
          {/each}
        </div>
        <div class="seg" style="margin-top:8px">
          <span>{duration(offer.duration_min)}</span>
          <span>{offer.stops === 0 ? 'Nonstop' : `${offer.stops} stop${offer.stops === 1 ? '' : 's'}`}</span>
          {#if wait}
            <span>{duration(wait)} layover</span>
          {/if}
          <span class="chip">{offer.source}</span>
        </div>
      </div>
      <div class="price">{money(offer.price, offer.currency)}</div>
    </div>
  </button>
</article>
