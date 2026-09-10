<script lang="ts">
  import { buildSearchHref, duration, money, timeRange, windowChip } from '$lib/api';
  import PriceSpark from '$lib/components/PriceSpark.svelte';
  import type { HiddenDeal } from '$lib/types';

  let { deal, compact = false }: { deal: HiddenDeal; compact?: boolean } = $props();
  let open = $state(false);
  const through = $derived(deal.through_offer);
  const getOff = $derived(deal.dest);
  const href = $derived(
    buildSearchHref({
      origin: deal.origin,
      destination: deal.dest,
      date: deal.date,
      adults: 1,
      cabin: 'ECONOMY',
      nearby: false,
      mode: 'hidden'
    })
  );
  const bookers = $derived((deal.bookers ?? []).filter((b) => !b.id.startsWith('carrier-')).slice(0, 4));
  const risk = $derived(deal.risk);
  const warnings = $derived(deal.warnings ?? []);
  const win = $derived(deal.window);
  const urgencyClass = $derived(windowChip(win?.urgency));
</script>

<article class="hc deal-card">
  <a class="deal-link" {href}>
    <div class="row">
      <div>
        <div class="chips">
          <span class="chip hot">Get off in {getOff}</span>
          {#if win}
            <span class="chip {urgencyClass}">{win.label}</span>
          {/if}
        </div>
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
      <div class="hc-side">
        <PriceSpark
          window={win}
          currency={deal.currency}
          was={deal.honest_price}
          compact={compact}
        />
        <div class="price">
          <span class="price-was">{money(deal.honest_price, deal.currency)}</span>
          {money(deal.through_price, deal.currency)}
          <small>Save {money(deal.saving, deal.currency)}</small>
        </div>
      </div>
    </div>
  </a>
  {#if !compact && win?.headline}
    <p class="window-note">{win.headline}</p>
  {/if}
  {#if !compact && bookers.length}
    <div class="book-row" style="margin-top:12px">
      {#each bookers as b}
        <a class="book-btn" href={b.url} target="_blank" rel="noreferrer">{b.name}</a>
      {/each}
    </div>
  {/if}
  {#if risk}
    {#if warnings.length && (!compact || open)}
      <ul class="hc-warnings">
        {#each warnings as w}
          <li>{w}</li>
        {/each}
      </ul>
    {/if}
    <div class="risk">
      <button class="ghost" type="button" onclick={() => (open = !open)}>
        {open ? 'Hide risks' : 'Risks of getting off early'}
      </button>
      {#if open}
        <p class="note" style="margin:10px 0 8px">{risk.headline}</p>
        <div class="risk-grid">
          {#each risk.items as item}
            <div>
              <div class="chip bad">{item.severity}</div>
              <strong style="display:block;margin:6px 0 4px">{item.label}</strong>
              <p class="note" style="margin:0">{item.why}</p>
            </div>
          {/each}
        </div>
      {:else if compact}
        <p class="note" style="margin:10px 0 0">{risk.headline}</p>
      {/if}
    </div>
  {/if}
</article>

<style>
  .hc-side {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 8px;
    flex: 0 0 auto;
  }
  .window-note {
    margin: 10px 0 0;
    color: var(--muted);
    font-size: 0.84rem;
    line-height: 1.4;
  }
  .hc-warnings {
    margin: 12px 0 0;
    padding-left: 18px;
    color: var(--muted);
    font-size: 0.85rem;
  }
</style>
