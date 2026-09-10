<script lang="ts">
  import { airlineName, duration, hm, money, refreshOffer, timeRange } from '$lib/api';
  import type { HiddenCityMatch, Offer } from '$lib/types';

  let { match, names = {} }: { match: HiddenCityMatch; names?: Record<string, string> } = $props();
  let open = $state(false);
  let refreshNote = $state('');
  let checking = $state(false);
  let quoted = $state<Offer | null>(null);
  const t = $derived(quoted ?? match.through_offer);
  const first = $derived(t.segments[0]);
  const last = $derived(t.segments[t.segments.length - 1]);
  const exitIdx = $derived(match.exit_segment_index ?? 0);
  const getOff = $derived(match.intended_destination || t.segments[exitIdx]?.dest || '');
  const ticketed = $derived(match.ticketed_destination || last?.dest || match.hidden_city);
  const unused = $derived(t.segments.slice(exitIdx + 1));
  const carrier = $derived(airlineName(t.carrier, names));
  const path = $derived(t.segments.map((s) => s.origin).concat(last ? [last.dest] : []).join(' → '));
  const warnings = $derived(match.warnings?.length ? match.warnings : []);
  const shownPrice = $derived(t.price);
  const shownSaving = $derived(
    match.local_offer.price != null && shownPrice != null
      ? match.local_offer.price - shownPrice
      : match.gross_saving
  );

  async function onRefresh() {
    checking = true;
    refreshNote = 'Checking whether this ticket still stops at ' + getOff + '…';
    try {
      const r = await refreshOffer(match.through_offer, getOff);
      if (!r.valid) {
        quoted = null;
        refreshNote = r.reason || 'This hidden-city result is no longer valid.';
        return;
      }
      if (r.offer) quoted = r.offer;
      refreshNote =
        r.offer?.price != null
          ? `Still a hidden-city ticket via ${getOff}. Fresh price ${money(r.offer.price, r.offer.currency)}.`
          : 'Still available.';
    } catch (e) {
      refreshNote = e instanceof Error ? e.message : 'Refresh failed';
    } finally {
      checking = false;
    }
  }
</script>

<article class="hc">
  <div class="row">
    <div>
      <div class="chips">
        <span class="chip hot">Hidden city</span>
        <span class="chip good">Get off in {getOff}</span>
        {#if match.first_flight_match}
          <span class="chip good">Same first flight</span>
        {/if}
      </div>
      {#if carrier}
        <p class="airline-name">{carrier}</p>
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
          {#each match.bookers.filter((b) => !b.id.startsWith('carrier-')).slice(0, 5) as b}
            <a class="book-btn" href={b.url} target="_blank" rel="noreferrer">{b.name}</a>
          {/each}
        </div>
      {/if}
    </div>
    <div class="price">
      <span class="price-was">{money(match.local_offer.price, match.currency)}</span>
      {money(shownPrice, t.currency || match.currency)}
      <small>Save {money(shownSaving, t.currency || match.currency)}</small>
      <button class="ghost recheck" type="button" onclick={onRefresh} disabled={checking}>
        {checking ? 'Checking…' : 'Recheck this ticket'}
      </button>
      {#if refreshNote}
        <p class="note recheck-note">{refreshNote}</p>
      {/if}
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
  .recheck {
    margin-top: 10px;
    font-size: 0.8rem;
  }
  .recheck-note {
    margin: 6px 0 0;
    max-width: 12rem;
    text-align: right;
  }
  @media (max-width: 720px) {
    .recheck-note {
      max-width: none;
      text-align: left;
    }
  }
</style>
