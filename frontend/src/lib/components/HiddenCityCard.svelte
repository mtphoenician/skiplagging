<script lang="ts">
  import { airlineName, duration, hm, money, refreshOffer, timeRange } from '$lib/api';
  import type { HiddenCityMatch } from '$lib/types';

  let { match, names = {} }: { match: HiddenCityMatch; names?: Record<string, string> } = $props();
  let open = $state(false);
  let refreshNote = $state('');
  const t = $derived(match.through_offer);
  const first = $derived(t.segments[0]);
  const last = $derived(t.segments[t.segments.length - 1]);
  const exitIdx = $derived(match.exit_segment_index ?? 0);
  const getOff = $derived(match.intended_destination || t.segments[exitIdx]?.dest || '');
  const ticketed = $derived(match.ticketed_destination || last?.dest || match.hidden_city);
  const unused = $derived(t.segments.slice(exitIdx + 1));
  const carrier = $derived(airlineName(t.carrier, names));
  const path = $derived(t.segments.map((s) => s.origin).concat(last ? [last.dest] : []).join(' → '));
  const warnings = $derived(match.warnings?.length ? match.warnings : []);

  async function onRefresh() {
    refreshNote = 'Checking…';
    try {
      const r = await refreshOffer(t, getOff);
      if (!r.valid) {
        refreshNote = r.reason || 'This hidden-city result is no longer valid.';
        return;
      }
      refreshNote = r.offer?.price != null ? `Fresh price ${r.offer.price} ${r.offer.currency}` : 'Still available.';
    } catch (e) {
      refreshNote = e instanceof Error ? e.message : 'Refresh failed';
    }
  }
</script>

<article class="hc">
  <div class="row">
    <div>
      <span class="chip hot">Hidden city</span>
      <span class="chip good">Get off in {getOff}</span>
      {#if match.first_flight_match}
        <span class="chip good">Same first flight</span>
      {/if}
      {#if carrier}
        <p class="airline-name" style="margin-top:10px">{carrier}</p>
      {/if}
      {#if first && last}
        <p class="flight-times">{hm(first.dep)} → {hm(last.arr)}</p>
      {/if}
      <p class="flight-path">{path}</p>
      <p class="note" style="margin:4px 0 8px">
        Ticketed destination: {match.hidden_city_name || ticketed}. Intended stop: {getOff}.
        {#if unused.length}
          You would not fly {unused.map((s) => `${s.origin}→${s.dest}`).join(', ')}.
        {/if}
      </p>
      <div class="seg">
        {#each t.segments as s, i}
          <span class:unused={i > exitIdx}
            ><strong>{s.flight_number}</strong> {s.origin}→{s.dest} {timeRange(s.dep, s.arr, s.duration_min)}</span
          >
        {/each}
      </div>
      <div class="seg" style="margin-top:6px">
        {duration(t.duration_min)}
      </div>
      {#if match.bookers?.length}
        <div class="book-row" style="margin-top:12px">
          {#each match.bookers.filter((b) => ['google-flights', 'kayak', 'skyscanner', 'booking-com', 'expedia'].includes(b.id)).slice(0, 5) as b}
            <a class="book-btn" href={b.url} target="_blank" rel="noreferrer">{b.name}</a>
          {/each}
          <button class="book-btn" type="button" onclick={onRefresh}>Refresh price</button>
        </div>
      {/if}
      {#if refreshNote}
        <p class="note">{refreshNote}</p>
      {/if}
    </div>
    <div class="price">
      <span class="price-was">{money(match.local_offer.price, match.currency)}</span>
      {money(t.price, match.currency)}
      <small>Save {money(match.gross_saving, match.currency)}</small>
    </div>
  </div>

  <ul class="hc-warnings">
    {#each warnings as w}
      <li>{w}</li>
    {/each}
  </ul>

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

<style>
  .unused {
    opacity: 0.55;
    text-decoration: line-through;
  }
  .hc-warnings {
    margin: 12px 0 0;
    padding-left: 18px;
    color: var(--muted);
    font-size: 0.85rem;
  }
</style>
