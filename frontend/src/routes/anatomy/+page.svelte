<svelte:head>
  <title>Anatomy — Skiplagging</title>
</svelte:head>

<div class="wrap anatomy">
  <p class="kicker">Technical · economic · operational</p>
  <h1>A technical, economic and operational anatomy of hidden-city ticketing</h1>
  <p class="lede">
    Skiplagging is consumer arbitrage against origin-destination price discrimination: buy A→B→C because it is cheaper
    than A→B, fly the first sector, and intentionally abandon the rest. The inversion is not a computer error. It is
    what market-based airline pricing looks like when two O-D products share an aircraft.
  </p>

  <div class="prose">
    <h2>The inequality</h2>
    <p class="eq">S = P<sub>AB</sub> − P<sub>ABC</sub> when P(A,B,C) &lt; P(A,B)</p>
    <p>
      A is the ticketed origin, B the connection and the traveller’s true destination, C the ticketed final city. The
      useful condition is not merely that A→C is cheap. The first sector must be the A→B flight the traveller would
      otherwise have booked.
    </p>
    <p>
      IATA uses the same architecture to explain why an indirect journey may legitimately cost less than one of its
      constituent sectors. London–New York is a different competitive product from Paris–New York even when both
      itineraries share a Paris–New York flight. Airlines price transportation markets, not the sum of leg costs.
    </p>

    <h2>How this engine searches</h2>
    <p>A conventional engine solves min P(A,B,d). A hidden-city engine also evaluates destinations C<sub>i</sub> such that</p>
    <p class="eq">A → f → B → C<sub>i</sub> and P(A,B,C<sub>i</sub>,d) &lt; P(A,B,d)</p>
    <ol>
      <li>Shop the local A→B market (nonstop and connecting).</li>
      <li>Shop nearby metro airports — another honest way to buy the trip.</li>
      <li>Rank C candidates beyond B from hub-and-spoke structure and documented inversions.</li>
      <li>Shop A→C in parallel; keep itineraries whose first sector is the desired A→B movement.</li>
      <li>Rank positive fare gaps, then estimate S<sub>net</sub>.</li>
    </ol>
    <p>
      Availability(A,B) need not equal Availability(A,B | A→B→C). Married segments and O-D inventory let a carrier
      expose K-class on a connection while the local shop only sees M. Fare rules — eligibility, seasonality, flight
      application, advance purchase, transfers, combinability — can open an A–C fare fence that the A–B market never
      sees. Dynamic offer engines may then adjust the constructed fare using load factor, channel, and request context.
      None of that requires a bug.
    </p>

    <h2>Published illustrations — not live tickets</h2>
    <table>
      <thead>
        <tr>
          <th>Search</th>
          <th>Physical flights</th>
          <th>Cited fare</th>
          <th>Intention</th>
        </tr>
      </thead>
      <tbody>
        <tr>
          <td>ORD→DCA</td>
          <td>Same first AA flight</td>
          <td>US$387</td>
          <td>Fly to Washington</td>
        </tr>
        <tr>
          <td>ORD→DCA→BOS</td>
          <td>Same first flight + onward</td>
          <td>US$178</td>
          <td>Exit at DCA</td>
        </tr>
        <tr>
          <td>JFK→SFO</td>
          <td>The Zaman 2013 observation</td>
          <td>~US$300</td>
          <td>Fly to San Francisco</td>
        </tr>
        <tr>
          <td>JFK→SFO→SEA</td>
          <td>Connection in SFO</td>
          <td>~US$170</td>
          <td>Exit at SFO</td>
        </tr>
      </tbody>
    </table>
    <p>
      Luttmann and Gaggero’s late-2019 US sample (473,000+ fares) finds hidden-city opportunities concentrated near
      departure, tied to competition on both the local and through markets, and associated with large hub-and-spoke
      carriers. That is evidence of a pricing phenomenon, not a count of people who skipped.
    </p>

    <h2>PNR, coupon, fare component</h2>
    <p>
      The reservation (PNR) records what was booked. The e-ticket holds sequential coupons. After A→B is flown, B→C
      remains an unused coupon on the same document — statuses such as open, checked-in, lifted, flown, exchanged,
      refunded, suspended. Coupons are not used out of sequence. A fare component is the itinerary between
      fare-construction points; A–B–C is often one through product delivered by two flight segments. Paying for “both
      flights” is the wrong ontology.
    </p>
    <p>
      That is why hidden-city is structurally a one-way technique. On A→B→C / C→B→A, skipping outbound B→C can collapse
      the return. IATA treats sequential use as integral to the end-to-end product.
    </p>

    <h2>Why airlines do not impose P(A,B,C) ≥ P(A,B)</h2>
    <p>
      They could try. Commercially it would force the through market to inherit the hub’s local monopoly price, and the
      carrier would lose A–C passengers to anyone selling the competitive fare. The durable trade-off is: protect A–B
      segmentation versus compete for A–C traffic. Married segments, coupon sequencing, dynamic offers and behavioural
      review can make arbitrage harder. They cannot make two rationally different markets have the same equilibrium
      price.
    </p>

    <h2>Risk is the rest of the product</h2>
    <table>
      <thead>
        <tr>
          <th>Risk</th>
          <th>Why</th>
          <th>Severity</th>
        </tr>
      </thead>
      <tbody>
        <tr>
          <td>Checked bag continues to C</td>
          <td>Handling follows the ticketed itinerary</td>
          <td>High</td>
        </tr>
        <tr>
          <td>Gate-checked carry-on</td>
          <td>Overhead or operational limits</td>
          <td>High</td>
        </tr>
        <tr>
          <td>Reroute avoids B</td>
          <td>Carrier protects ticketed C in irregular operations</td>
          <td>Very high</td>
        </tr>
        <tr>
          <td>Later coupons cancelled</td>
          <td>Sequential-use controls</td>
          <td>Very high</td>
        </tr>
        <tr>
          <td>International documents for C</td>
          <td>Passenger is processed as bound for C</td>
          <td>High</td>
        </tr>
        <tr>
          <td>Repricing / refusal</td>
          <td>AA, DL, LH materials allow fare-difference collection and refusal</td>
          <td>Medium–high</td>
        </tr>
      </tbody>
    </table>
    <p class="eq">
      Value = fare saving − lost robustness − operational constraints − enforcement exposure
    </p>
    <p>
      Current American, Delta and Lufthansa conditions expressly allow forms of cancellation, repricing, refusal of
      carriage or baggage, or collection of fare differences when ticket sequencing or point-beyond practices are
      detected. This page does not analyse jurisdiction or court cases.
    </p>

    <h2>Economics of the “loss”</h2>
    <p>
      A displayed $180 saving is not an $180 airline loss. The counterfactual matters: the passenger might have paid
      the local fare, bought a competitor, taken a train, delayed, or not travelled. Only the first case is full fare
      leakage. Skiplagged’s 2024 platform figures — about 294,908 travellers and US$53 million claimed savings, ~$180
      each — are company-reported, not an audited industry census. There is no robust global count of intentional skips.
    </p>
    <p>
      The deeper airline objection is not the empty B→C seat. It is that hidden-city use collapses the fare fence
      between high-WTP local passengers and price-sensitive through-market passengers, and distorts no-show and
      inventory forecasts.
    </p>

    <h2>What this software will not do</h2>
    <p>
      It will not book tickets, instruct anyone to skip a coupon, or suggest how to avoid carrier review. It shops
      public fare space, labels inversions, and prices the fragility the sticker omits. The same physical seat can have
      two economic values. That is the whole phenomenon.
    </p>
  </div>
</div>
