<script lang="ts">
  import DealCard from '$lib/components/DealCard.svelte';
  import { fetchDeals } from '$lib/api';
  import type { HiddenDeal } from '$lib/types';

  let deals = $state<HiddenDeal[]>([]);
  let error = $state('');
  let loading = $state(true);

  $effect(() => {
    loading = true;
    fetchDeals({ limit: 80 })
      .then((rows) => {
        deals = rows;
      })
      .catch((e: Error) => {
        error = e.message;
      })
      .finally(() => {
        loading = false;
      });
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
    </div>
  </header>

  {#if loading}
    <p class="empty">Loading saved deals…</p>
  {:else if error}
    <p class="empty">{error}</p>
  {:else if !deals.length}
    <p class="empty">
      No published inversions. This list is live shop inventory only — Duffel test tokens and mock
      fixtures are not shown. Add a <code>duffel_live_…</code> token (or Amadeus production), then run
      <code>python -m app.engines.discover --wipe</code> from the API folder.
    </p>
  {:else}
    <p class="note">{deals.length} saved deal{deals.length === 1 ? '' : 's'}, highest saving first.</p>
    <div class="deal-list">
      {#each deals as deal}
        <DealCard {deal} />
      {/each}
    </div>
  {/if}
</div>
