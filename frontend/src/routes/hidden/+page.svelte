<script lang="ts">
  import DealCard from '$lib/components/DealCard.svelte';
  import { fetchDeals, isAbortError } from '$lib/api';
  import type { HiddenDeal } from '$lib/types';

  let deals = $state.raw<HiddenDeal[]>([]);
  let error = $state('');
  let loading = $state(true);

  $effect(() => {
    loading = true;
    const ac = new AbortController();
    fetchDeals({ limit: 80, signal: ac.signal })
      .then((rows) => {
        deals = rows;
      })
      .catch((e: Error) => {
        if (!isAbortError(e)) error = e.message;
      })
      .finally(() => {
        if (!ac.signal.aborted) loading = false;
      });
    return () => ac.abort();
  });
</script>

<svelte:head>
  <title>Hidden-city deals — Skiplagging</title>
</svelte:head>

<div class="wrap page">
  <header class="page-head">
    <div>
      <p class="eyebrow">A → B, ticketed to C</p>
      <h1>Hidden-city deals</h1>
      <p class="lede">
        Prefound inversions: a ticket that continues past your city costs less than one that ends there. The crossed
        price is the cheapest honest alternative. Airlines prohibit getting off early. We do not book.
      </p>
      <p class="note">
        <a class="text-link" href="/">Search a pair</a>
        — one-way dates; a back date is a second one-way home, not a round-trip.
      </p>
    </div>
  </header>

  {#if loading}
    <p class="empty">Loading saved deals…</p>
  {:else if error}
    <p class="empty">{error}</p>
  {:else if !deals.length}
    <p class="empty">
      No published inversions. This list is live shop inventory only — Duffel test tokens and mock
      fixtures are not shown. Add a <code>duffel_live_…</code> token, then run
      <code>cargo run --bin discover -- --wipe</code> from the API folder.
    </p>
  {:else}
    <p class="note">
      {deals.length} saved deal{deals.length === 1 ? '' : 's'}, soonest-to-vanish first — hidden-city
      inventory dies when the unused leg fills.
    </p>
    <div class="deal-list">
      {#each deals as deal (deal.id)}
        <DealCard {deal} />
      {/each}
    </div>
  {/if}
</div>
