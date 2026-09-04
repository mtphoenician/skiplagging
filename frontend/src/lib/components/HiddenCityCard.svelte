<script lang="ts">
  import { duration, hm, money } from '$lib/api';
  import type { HiddenCityMatch } from '$lib/types';

  let { match }: { match: HiddenCityMatch } = $props();
  let open = $state(false);
  const t = $derived(match.through_offer);
  const getOff = $derived(t.segments[0]?.dest || '');
</script>

<article class="hc">
  <div class="row">
    <div>
      <span class="chip hot">Get off in {getOff}</span>
      {#if match.first_flight_match}
        <span class="chip good">Same first flight</span>
      {/if}
      <h3 style="margin:10px 0 8px;font-size:1.15rem">
        Ticket to {match.hidden_city} · leave at {getOff}
      </h3>
      <div class="seg">
        {#each t.segments as s}
          <span><strong>{s.flight_number}</strong> {s.origin}→{s.dest} {hm(s.dep)}–{hm(s.arr)}</span>
        {/each}
      </div>
      <div class="seg" style="margin-top:6px">
        {duration(t.duration_min)} · {t.carrier}
      </div>
      {#if match.bookers?.length}
        <div class="book-row" style="margin-top:12px">
          {#each match.bookers.slice(0, 4) as b}
            <a class="book-btn" href={b.url} target="_blank" rel="noreferrer">{b.name}</a>
          {/each}
        </div>
      {/if}
    </div>
    <div class="price">
      {money(t.price, match.currency)}
      <small>Save {money(match.gross_saving, match.currency)} vs {money(match.local_offer.price, match.currency)}</small>
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
