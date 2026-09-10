<script lang="ts">
  import DealCard from '$lib/components/DealCard.svelte';
  import FlightArc from '$lib/components/FlightArc.svelte';
  import SearchForm from '$lib/components/SearchForm.svelte';
  import { defaultDate, fetchDeals } from '$lib/api';
  import type { Cabin, HiddenDeal, SearchMode } from '$lib/types';

  let origin = $state('');
  let destination = $state('');
  let date = $state(defaultDate());
  let adults = $state(1);
  let cabin = $state<Cabin>('ECONOMY');
  let include_nearby = $state(false);
  let mode = $state<SearchMode>('hidden');
  let return_date = $state('');
  let preview = $state<HiddenDeal[]>([]);

  $effect(() => {
    fetchDeals({ limit: 4 })
      .then((rows) => {
        preview = rows;
      })
      .catch(() => {});
  });
</script>

<svelte:head>
  <title>Skiplagging — find hidden-city fares</title>
</svelte:head>

<div class="wrap">
  <header class="hero">
    <p class="eyebrow">Ticketed past your city, when that ticket is cheaper</p>
    <h1>Find a hidden-city fare</h1>
    <p class="lede">
      {#if mode === 'compare'}
        A round-trip PNR is never hidden-city. Uncheck that option to shop two one-ways — outbound and home each get their
        own through-ticket search.
      {:else}
        We shop one-way A→B, then look for a cheaper complete ticket A→B→C. You would get off at B. A back date is a second
        one-way home. Airlines prohibit getting off early.
      {/if}
    </p>
    <FlightArc a={origin || 'A'} b={destination || 'B'} c="C" />
  </header>

  <SearchForm bind:origin bind:destination bind:date bind:return_date bind:adults bind:cabin bind:include_nearby bind:mode />

  {#if preview.length}
    <section class="deal-preview">
      <div class="deal-head">
        <div>
          <p class="eyebrow">Saved inversions</p>
          <h2>Best hidden-city deals</h2>
        </div>
        <a class="text-link" href="/hidden">Browse all</a>
      </div>
      <div class="deal-list compact">
        {#each preview as deal}
          <DealCard {deal} compact />
        {/each}
      </div>
    </section>
  {/if}

  <section class="how">
    <article>
      <b><span class="step">1</span> Search</b>
      <p>Two airports and a one-way date. A back date shops a second one-way home — never one round-trip ticket.</p>
    </article>
    <article>
      <b><span class="step">2</span> Find the inversion</b>
      <p>We keep the cheapest ticket that actually ends at your city, then hunt for a cheaper through-ticket. The deal is shown first when it saves money.</p>
    </article>
    <article>
      <b><span class="step">3</span> Book there</b>
      <p>Google Flights, Kayak, Skyscanner, and other bookers. We never issue a ticket.</p>
    </article>
  </section>
</div>
