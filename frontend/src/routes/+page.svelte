<script lang="ts">
  import { goto } from '$app/navigation';
  import AirportInput from '$lib/components/AirportInput.svelte';
  import RouteDiagram from '$lib/components/RouteDiagram.svelte';
  import { defaultDate } from '$lib/api';
  import type { Cabin } from '$lib/types';

  let origin = $state('ORD');
  let destination = $state('DCA');
  let date = $state(defaultDate());
  let adults = $state(1);
  let cabin = $state<Cabin>('ECONOMY');
  let include_nearby = $state(true);
  let live = $state(false);

  function submit(e: Event) {
    e.preventDefault();
    const q = new URLSearchParams({
      origin,
      destination,
      date,
      adults: String(adults),
      cabin,
      nearby: include_nearby ? '1' : '0',
      live: live ? '1' : '0'
    });
    goto(`/results?${q.toString()}`);
  }
</script>

<svelte:head>
  <title>Skiplagging — hidden-city fare search</title>
</svelte:head>

<div class="wrap">
  <header class="hero">
    <p class="kicker">A → B → C when P(A,B,C) &lt; P(A,B)</p>
    <h1>Find every way to buy the seat you actually want.</h1>
    <p class="lede">
      Nonstops, honest connections, nearby runways, and hidden-city inversions — ranked with the operational costs
      the fare board leaves out. The ticket is an origin-destination product, not two disposable flights.
    </p>
    <RouteDiagram a={origin || 'A'} b={destination || 'B'} c="C" />
  </header>

  <form class="search-card" onsubmit={submit}>
    <div class="search-grid">
      <AirportInput bind:value={origin} label="From A" placeholder="ORD" />
      <AirportInput bind:value={destination} label="True destination B" placeholder="DCA" />
      <div class="field">
        <label for="date">Date</label>
        <input id="date" type="date" bind:value={date} required />
      </div>
      <div class="field">
        <label for="cabin">Cabin</label>
        <select id="cabin" bind:value={cabin}>
          <option value="ECONOMY">Economy</option>
          <option value="PREMIUM_ECONOMY">Premium</option>
          <option value="BUSINESS">Business</option>
          <option value="FIRST">First</option>
        </select>
      </div>
      <button class="go" type="submit">Shop all ways</button>
    </div>
    <div class="toggles">
      <label><input type="checkbox" bind:checked={include_nearby} /> Nearby airports</label>
      <label><input type="number" min="1" max="9" bind:value={adults} style="width:3.4rem;margin-right:6px" /> Adults</label>
      <label><input type="checkbox" bind:checked={live} /> Live GDS (Amadeus keys)</label>
    </div>
  </form>

  <section class="grid-3">
    <article class="tile">
      <p class="kicker">01</p>
      <h3>Every channel</h3>
      <p>Local nonstop, connecting service that actually ends at B, metro-airport swaps, then through-market A→C via B.</p>
    </article>
    <article class="tile">
      <p class="kicker">02</p>
      <h3>The inversion</h3>
      <p>Keep only itineraries whose first sector is the A→B flight you would have bought, and where the through fare is cheaper.</p>
    </article>
    <article class="tile">
      <p class="kicker">03</p>
      <h3>Net, not sticker</h3>
      <p>S_net subtracts extra fees and expected disruption / enforcement cost. A $200 gap is not a $200 saving.</p>
    </article>
  </section>
</div>
