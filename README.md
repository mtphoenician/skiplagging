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

Hidden-city rows exist only when a shop API returns **P(A,B,C) < P(A,B)** and the first sector is A→B. OpenFlights spokes are labeled **connection hints**, not savings.

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

Copy `.env.example` to `.env`. Without Amadeus/Duffel keys you still get the real airport table, OpenSky traffic, connection hints, and live booker links. You will **not** get invented prices.

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
- `POST /search` — all layers for A, B, date (persisted in `searches` / `offers`)
- `GET /track/{iata}` — OpenSky box + tracker links
- `GET /board/{iata}` — AeroDataBox FIDS if keyed

Airline conditions treat hidden-city / point-beyond ticketing as prohibited. This repo does not book, and does not advise on avoiding carrier review.
