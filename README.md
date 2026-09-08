# Skiplagging

Hidden-city fare discovery with a **PostgreSQL** airport/route database and layers that are not mixed up.

| Layer | What it is | What it is not | Source |
| --- | --- | --- | --- |
| Reference | Where the airport is | A flight, a fare, a booking | [OurAirports](https://ourairports.com/data/) nightly CSV |
| Historical route map | Who used to publish A→B | Current schedule or inventory | [OpenFlights](https://openflights.org/data.php) (~2014–2017, stale) |
| Live track | Transponder state (ADS-B/MLAT/…) | A ticket or a future flight | [OpenSky](https://opensky-network.org) + FR24 / FlightAware links |
| Schedule / FIDS | Scheduled, estimated, actual board | A fare or ADS-B | [AeroDataBox](https://aerodatabox.com) if `RAPIDAPI_KEY` |
| Priced offer | Shopped itinerary + tax + RBD | A PNR | [Amadeus](https://developers.amadeus.com) GDS and/or [Duffel](https://duffel.com/docs) NDC |
| Booker | Outbound search URL | A reservation this app created | Google Flights, Kayak, Skyscanner (meta); Expedia (OTA); airline.com |
| Order / PNR | Reservation + coupons | — | **Not implemented.** We never call Create Orders. |

Hidden-city rows are **complete priced tickets** that continue past the intended city (ticketed `C`, intended stop `B`). The engine never rewrites `A→B→C` into a fake `A→B` fare, and never prices a hidden-city trip by adding `P(A,B)+P(B,C)`.

Offers are stored and indexed by **ticketed destination and connection airports**. A later search for A→B can reuse a previously shopped A→B→C ticket and skip shopping C again. Fresh A→B results in the offer index skip a live shop; cheaper hidden-city already on hand skips destination expansion. Live traffic never blocks priced results. Sources are partner adapters only (mock, Duffel, Amadeus) — this app does not collect airline websites. OpenFlights spokes are **connection hints**, not savings.

## Buy the fares, build the search

The supplier (Duffel/Amadeus) is asked only for ordinary A→C offers. Everything that decides *which* C to pay for, and what we learn from the answer, is ours:

| Module | Where | What it owns |
| --- | --- | --- |
| Search planner | `engines/shop.py` | Two-speed search: `POST /search` (index → direct A→B → top candidates) then `POST /search/expand` (remaining ranked candidates, reusing the index) |
| CandidateGenerator | `engines/candidates.py` | `score = 0.30·connection + 0.25·savings probability + 0.20·expected saving + 0.10·freshness + 0.10·hub + 0.05·supplier`; shop those ≥ 0.35 up to `MAX_HIDDEN_CANDIDATES`, always 3 on a cold start, never one that failed 5 checks |
| Route learner | `engines/learn.py` | Every response updates `fare_observations` (append-only, never overwritten), `route_edges` (flights seen inside tickets) and `hidden_city_route_stats` (A, B, C → observations, via-B successes, cheaper-than-direct count, avg/median/max saving, freshness, score) |
| Search budget | `engines/budget.py` | Every paid supplier call is timed and logged to `provider_calls`; `GET /debug/budget` gives cost, calls per search and `ProviderScore(route, provider)` |
| Hidden-city detector, dedupe, rank | `engines/hidden.py`, `engines/shop.py` | Complete tickets only; same currency; never `P(A,B)+P(B,C)` |

Inspect what the planner has learned at `GET /debug/route-graph?origin=JFK&intended=ORD` and a fare's history at `GET /debug/price-history?fingerprint=…`. The database starts empty; each search makes the next one cheaper. Redis, OAG/Cirium schedules and a second GDS are later phases — the interfaces above do not change for them.

## Database

Local PostgreSQL (`skiplagging`). Create once, then ingest official files:

```bash
createdb skiplagging
cd backend
source .venv/bin/activate
pip install -r requirements.txt
python -m app.db.ingest
```

That loads the official gazetteers, not a hand-built list:

- OurAirports `countries.csv`, `regions.csv`, `airports.csv`, `runways.csv`, `navaids.csv` (nightly, public domain)
- GeoNames `countryInfo.txt` (ISO3, capital, currency, population, languages)
- OpenFlights airlines/routes (historical, stale)

Browse them at `/places`. `GET /countries`, `/countries/US/regions`, `/airports/ORD` include runways.

## Run

```bash
# API
cd backend && source .venv/bin/activate
uvicorn app.main:app --reload --port 8000

# UI
cd frontend && npm install && npm run dev
```

[http://localhost:5173](http://localhost:5173) · [Sources](http://localhost:5173/sources)

Copy `.env.example` to `.env`. `MOCK_ENABLED` defaults on. Search **JFK → ORD** to see the fixture:

- Standard JFK→ORD **$240**
- Hidden city JFK→ORD→DEN **$170** (get off in ORD, ticketed DEN)
- Hidden city JFK→ORD→SEA **$185**
- JFK→DFW→LAX is rejected (does not pass through ORD)

Without Amadeus/Duffel keys, other city pairs stay empty except those mock fixtures. Live shops never invent segment-sum prices.

```
AMADEUS_CLIENT_ID=
AMADEUS_CLIENT_SECRET=
DUFFEL_TOKEN=
RAPIDAPI_KEY=
```

## API

- `GET /health` — DB counts and which shop/track keys are on
- `GET /sources` — full capability matrix
- `GET /airports?q=` — OurAirports
- `POST /search` — fast pass: all layers for A, B, date; returns `search_debug.pending_candidates` for the deep pass
- `POST /search/expand` — deep pass: `{query, exclude}`; shops the remaining ranked candidates and replaces the cached result
- `POST /offers/refresh` — reprice a mock offer; invalidates hidden-city if the path no longer stops at B
- `GET /debug/route-graph` — learned `hidden_city_route_stats` and `route_edges` (filter `origin`, `intended`)
- `GET /debug/budget` — paid supplier calls, cost, calls per search; add `origin`+`dest` for per-route provider scores
- `GET /debug/price-history?fingerprint=` — append-only fare observations for one itinerary
- `GET /track/{iata}` — OpenSky box + tracker links
- `GET /board/{iata}` — AeroDataBox FIDS if keyed

Airline conditions treat hidden-city / point-beyond ticketing as prohibited. This repo does not book, and does not advise on avoiding carrier review.
