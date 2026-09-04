<script lang="ts">
  import FlightArc from '$lib/components/FlightArc.svelte';
  import SearchForm from '$lib/components/SearchForm.svelte';
  import { defaultDate, fetchDefaults } from '$lib/api';
  import type { Cabin } from '$lib/types';

  let origin = $state('');
  let destination = $state('');
  let date = $state(defaultDate());
  let adults = $state(1);
  let cabin = $state<Cabin>('ECONOMY');
  let include_nearby = $state(true);

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

  <section class="how">
    <article>
      <b><span class="step">1</span> Search</b>
      <p>Two airports and a date. Nearby fields are included.</p>
    </article>
    <article>
      <b><span class="step">2</span> Compare</b>
      <p>Direct, connecting, and cheaper tickets one city further.</p>
    </article>
    <article>
      <b><span class="step">3</span> Book there</b>
      <p>Google Flights, Kayak, or an airline. We never issue a ticket.</p>
    </article>
  </section>
</div>
