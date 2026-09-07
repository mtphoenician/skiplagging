<script lang="ts">
  import { duration, money, timeRange } from '$lib/api';
  import type { HiddenDeal } from '$lib/types';

  let { deal, compact = false }: { deal: HiddenDeal; compact?: boolean } = $props();
  const through = $derived(deal.through_offer);
  const getOff = $derived(through?.segments[0]?.dest || deal.dest);
  const href = $derived(`/results?origin=${deal.origin}&destination=${deal.dest}&date=${deal.date}&nearby=0`);
</script>

<article class="hc deal-card">
  <a class="deal-link" {href}>
    <div class="row">
      <div>
        <span class="chip hot">Get off in {getOff}</span>
        <h3>
          {deal.origin_city || deal.origin} → {deal.dest_city || deal.dest}
        </h3>
        <p class="note" style="margin:0 0 8px">
          Ticket continues to {deal.hidden_city_name || deal.hidden_city}
        </p>
        {#if through?.segments?.length && !compact}
          <div class="seg">
            {#each through.segments as s}
              <span><strong>{s.flight_number}</strong> {s.origin}→{s.dest} {timeRange(s.dep, s.arr, s.duration_min)}</span>
            {/each}
          </div>
          <div class="seg" style="margin-top:6px">
            {duration(through.duration_min)} · {deal.date}
          </div>
        {:else}
          <div class="seg">{deal.first_flight} · {deal.date}</div>
        {/if}
      </div>
      <div class="price">
        <span class="price-was">{money(deal.honest_price, deal.currency)}</span>
        {money(deal.through_price, deal.currency)}
        <small>Save {money(deal.saving, deal.currency)}</small>
      </div>
    </div>
  </a>
  {#if !compact && deal.bookers?.length}
    <div class="book-row" style="margin-top:12px">
      {#each deal.bookers.slice(0, 4) as b}
        <a class="book-btn" href={b.url} target="_blank" rel="noreferrer">{b.name}</a>
      {/each}
    </div>
  {/if}
</article>
