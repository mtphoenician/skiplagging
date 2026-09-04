<script lang="ts">
  import { duration, hm, money } from '$lib/api';
  import type { Offer } from '$lib/types';

  let { offer }: { offer: Offer } = $props();
</script>

<article class="offer">
  <div class="row">
    <div>
      <div class="chip">{offer.kind}</div>
      <div class="chip">{offer.source}</div>
      <div class="chip">{offer.fare_basis} · {offer.segments[0]?.rbd}</div>
      <div class="seg" style="margin-top:12px">
        {#each offer.segments as s, i}
          <span>
            <strong>{s.flight_number}</strong>
            {s.origin}→{s.dest}
            {hm(s.dep)}–{hm(s.arr)}
          </span>
          {#if i < offer.segments.length - 1}
            <span>·</span>
          {/if}
        {/each}
      </div>
      <div class="seg" style="margin-top:6px">{duration(offer.duration_min)} · {offer.stops} stop{offer.stops === 1 ? '' : 's'}</div>
    </div>
    <div>
      <div class="price">{money(offer.price, offer.currency)}</div>
    </div>
  </div>
</article>
