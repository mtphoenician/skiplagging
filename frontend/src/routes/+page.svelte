<script lang="ts">
  import DealCard from '$lib/components/DealCard.svelte';
  import FlightArc from '$lib/components/FlightArc.svelte';
  import SearchForm from '$lib/components/SearchForm.svelte';
  import { defaultDate, fetchDefaults, fetchDeals } from '$lib/api';
  import type { Cabin, HiddenDeal } from '$lib/types';

  let origin = $state('');
  let destination = $state('');
  let date = $state(defaultDate());
  let adults = $state(1);
  let cabin = $state<Cabin>('ECONOMY');
  let include_nearby = $state(true);
  let preview = $state<HiddenDeal[]>([]);

  $effect(() => {
    fetchDeals({ limit: 4 })
      .then((rows) => {
        preview = rows;
      })
      .catch(() => {});
  });

  $effect(() => {
    fetchDefaults()
      .then((d) => {
        if (!origin) origin = d.origin.iata;
        if (!destination) destination = d.destination.iata;
      })
      .catch(() => {});
  });
</script>

<svelte:head>
  <title>Skiplagging — find cheaper flights</title>
</svelte:head>

<div class="wrap">
  <header class="hero">
    <p class="eyebrow">A → B, sometimes via C</p>
    <h1>Find cheaper flights</h1>
    <p class="lede">A ticket that continues past your city can cost less than one that ends there.</p>
    <FlightArc a={origin || 'A'} b={destination || 'B'} c="C" />
  </header>

  <SearchForm bind:origin bind:destination bind:date bind:adults bind:cabin bind:include_nearby />

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
      <p>Two airports and a date. Nearby fields are included.</p>
    </article>
    <article>
      <b><span class="step">2</span> Compare</b>
      <p>A hidden-city ticket if it is cheaper than the honest fare. Otherwise the cheapest flight, then the shortest layover.</p>
    </article>
    <article>
      <b><span class="step">3</span> Book there</b>
      <p>Google Flights, Kayak, or an airline. We never issue a ticket.</p>
    </article>
  </section>
</div>
