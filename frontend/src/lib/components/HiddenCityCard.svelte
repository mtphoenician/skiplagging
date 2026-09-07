<script lang="ts">
  import { airlineName, duration, money, timeRange } from '$lib/api';
  import type { HiddenCityMatch } from '$lib/types';

  let { match, names = {} }: { match: HiddenCityMatch; names?: Record<string, string> } = $props();
  let open = $state(false);
  const t = $derived(match.through_offer);
  const getOff = $derived(t.segments[0]?.dest || '');
  const carrier = $derived(airlineName(t.carrier, names));
</script>

<article class="hc">
  <div class="row">
    <div>
      <span class="chip hot">Get off in {getOff}</span>
      {#if match.first_flight_match}
        <span class="chip good">Same first flight</span>
      {/if}
      <h3 style="margin:10px 0 8px;font-size:1.15rem">
        {carrier || 'Airline'} ticket to {match.hidden_city_name || match.hidden_city} · leave at {getOff}
      </h3>
      <div class="seg">
        {#each t.segments as s}
          <span><strong>{s.flight_number}</strong> {s.origin}→{s.dest} {timeRange(s.dep, s.arr, s.duration_min)}</span>
        {/each}
      </div>
      <div class="seg" style="margin-top:6px">
        {duration(t.duration_min)}
      </div>
      {#if match.bookers?.length}
        <div class="book-row" style="margin-top:12px">
          {#each match.bookers.filter((b) => ['google-flights', 'kayak', 'skyscanner', 'booking-com'].includes(b.id)).slice(0, 4) as b}
            <a class="book-btn" href={b.url} target="_blank" rel="noreferrer">{b.name}</a>
          {/each}
        </div>
      {/if}
    </div>
    <div class="price">
      <span class="price-was">{money(match.local_offer.price, match.currency)}</span>
      {money(t.price, match.currency)}
      <small>Save {money(match.gross_saving, match.currency)}</small>
    </div>
  </div>

  <div class="risk">
    <button class="ghost" type="button" onclick={() => (open = !open)}>
      {open ? 'Hide risks' : 'Risks of getting off early'}
    </button>
    {#if open}
      <p class="note" style="margin:10px 0 8px">{match.risk.headline}</p>
      <div class="risk-grid">
        {#each match.risk.items as item}
          <div>
            <div class="chip bad">{item.severity}</div>
            <strong style="display:block;margin:6px 0 4px">{item.label}</strong>
            <p class="note" style="margin:0">{item.why}</p>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</article>
