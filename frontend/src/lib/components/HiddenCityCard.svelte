<script lang="ts">
  import { duration, hm, money } from '$lib/api';
  import type { HiddenCityMatch } from '$lib/types';

  let { match }: { match: HiddenCityMatch } = $props();
  let open = $state(false);
  const t = $derived(match.through_offer);
</script>

<article class="hc">
  <div class="row">
    <div>
      <span class="chip hot">Hidden city · {match.hidden_city_name}</span>
      {#if match.first_flight_match}
        <span class="chip">same first flight</span>
      {/if}
      <span class="chip">one-way only</span>
      <span class="chip">carry-on only</span>
      <h3 class="serif" style="margin:12px 0 6px;font-size:1.35rem">
        {t.segments[0].origin} → {t.segments[0].dest}
        <span style="color:var(--mist);font-size:0.9rem"> then unused {t.segments[1]?.origin}→{t.segments[1]?.dest}</span>
      </h3>
      <div class="seg">
        {#each t.segments as s}
          <span><strong>{s.flight_number}</strong> {s.origin}→{s.dest} {hm(s.dep)}–{hm(s.arr)}</span>
        {/each}
      </div>
      <div class="seg" style="margin-top:6px">
        {duration(t.duration_min)} · {t.fare_basis} · {t.carrier}
      </div>
    </div>
    <div>
      <div class="price">
        {money(t.price, match.currency)}
        <small>
          {money(match.gross_saving, match.currency)} / {match.saving_pct}% vs local
          {money(match.local_offer.price, match.currency)}
        </small>
        <small style="color:var(--mist)">
          est. net {money(match.risk.net_saving_estimate, match.currency)} after disruption &amp; enforcement costs
        </small>
      </div>
    </div>
  </div>

  <div class="risk">
    <p style="margin:0 0 8px">{match.risk.headline} · fragility {100 - match.risk.score}/100</p>
    <button class="chip" type="button" onclick={() => (open = !open)}>{open ? 'Hide' : 'Show'} operational risks</button>
    {#if open}
      <div class="risk-grid" style="margin-top:12px">
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
