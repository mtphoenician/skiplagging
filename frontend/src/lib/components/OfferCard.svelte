<script lang="ts">
  import { duration, hm, money } from '$lib/api';
  import type { Offer } from '$lib/types';

  let { offer }: { offer: Offer } = $props();
</script>

<article class="offer">
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
        <span class="chip">{offer.source}</span>
      </div>
    </div>
    <div class="price">{money(offer.price, offer.currency)}</div>
  </div>
</article>
