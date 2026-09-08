<script lang="ts">
  import FlightArc from '$lib/components/FlightArc.svelte';
</script>

<svelte:head>
  <title>How it works — Skiplagging</title>
</svelte:head>

<div class="wrap page">
  <header class="page-head anatomy-hero">
    <div>
      <p class="eyebrow">A → B, sometimes via C</p>
      <h1>How hidden-city fares work</h1>
      <p class="lede">
        Buy a cheaper ticket that continues past your city, fly the first leg, and skip the rest. Airlines prohibit this.
        We show the price gap and the risks — we do not book it.
      </p>
    </div>
    <FlightArc a="A" b="B" c="C" />
  </header>

  <p class="eq">Saving = price to your city − price of a ticket that goes one city further</p>

  <div class="anatomy-grid">
    <section class="anatomy-card">
      <h2>The idea</h2>
      <p>
        You want A→B. Sometimes a complete A→B→C ticket is cheaper than a ticket that ends at B. You would get off at
        B. That only works as a one-way, carry-on trip. Checked bags go to C. Missed connections and irregular operations
        can send you past B.
      </p>
    </section>
    <section class="anatomy-card">
      <h2>How we search</h2>
      <ol>
        <li>Price A→B (nonstop and connecting).</li>
        <li>Optionally price nearby airports when the city pair is thin.</li>
        <li>Find cities C that historically connect from B, then learn from tickets we actually priced.</li>
        <li>Price complete A→C trips and keep those that stop at your city B on the way.</li>
        <li>Show a hidden-city fare only when that through-ticket is cheaper than the cheapest honest A→B, after converting every quote to USD.</li>
        <li>
          Separately, stitch independently priced A→H and H→B tickets into a self-transfer row when the layover is long
          enough and the combined trip is at most five flights. That is not hidden-city: you are going to B, on two or three tickets.
        </li>
      </ol>
    </section>
  </div>

  <div class="anatomy-grid">
    <section class="anatomy-card">
      <h2>Published examples — not live tickets</h2>
      <table class="mini">
        <thead>
          <tr>
            <th>Search</th>
            <th>Cited fare</th>
            <th>Intention</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td>ORD→DCA</td>
            <td>US$387</td>
            <td>Fly to Washington</td>
          </tr>
          <tr>
            <td>ORD→DCA→BOS</td>
            <td>US$178</td>
            <td>Exit at DCA</td>
          </tr>
          <tr>
            <td>JFK→SFO</td>
            <td>~US$300</td>
            <td>Fly to San Francisco</td>
          </tr>
          <tr>
            <td>JFK→SFO→SEA</td>
            <td>~US$170</td>
            <td>Exit at SFO</td>
          </tr>
        </tbody>
      </table>
    </section>
    <section class="anatomy-card">
      <h2>Main risks</h2>
      <table class="mini">
        <thead>
          <tr>
            <th>Risk</th>
            <th>Why it matters</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td>Bags go to C</td>
            <td>Handling follows the ticket, not your plan</td>
          </tr>
          <tr>
            <td>Reroute skips B</td>
            <td>The airline protects the ticketed last city</td>
          </tr>
          <tr>
            <td>Return cancelled</td>
            <td>Skipping a coupon can void the rest of the ticket</td>
          </tr>
          <tr>
            <td>Fare difference</td>
            <td>Carriers may reprice or refuse travel</td>
          </tr>
        </tbody>
      </table>
    </section>
  </div>

  <p class="note anatomy-note">
    Prices appear only from Amadeus or Duffel. Without those keys, we still send you to Google Flights and Kayak. This
    page does not analyse court cases or tell anyone how to avoid airline review.
  </p>
</div>
