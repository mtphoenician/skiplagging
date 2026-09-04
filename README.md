# Skiplagging

**A Technical, Economic and Operational Anatomy of Hidden-City Ticketing**

A Svelte search surface and a Python fare shop that looks at every ordinary way to buy a seat to B — nonstop, connecting, nearby airports — and then ranks hidden-city inversions where

\[
P(A,B,C) < P(A,B)
\]

on an itinerary whose first sector is the A→B flight you actually want.

This is fare discovery and systems explanation. It does not book tickets, and it does not treat a through fare as two disposable flights. Carriers prohibit hidden-city / point-beyond ticketing. Sequential coupons, baggage, irregular operations and fare-difference collection can erase the displayed gap. Read `/anatomy` before treating any row as a plan.

## Stack

| Layer | Choice | Why |
| --- | --- | --- |
| Shop API | FastAPI + `orjson` + async `httpx` | Bounded fan-out over C candidates, connection pooling, TTL cache |
| Engine | In-process O-D graph + concurrent gather | No pandas; hub spokes and seeded inversions ranked in memory |
| Live GDS | Optional Amadeus Flight Offers Search | Same parser, same ranking, same risk layer |
| UI | SvelteKit | Search, results, long-form anatomy |

Without API keys the **demo engine** is on. It is not a live ticket cache. It reproduces the O-D mechanism (hub local power versus competitive through markets), including the published ORD–DCA–BOS and JFK–SFO–SEA quotations, so the search and risk layers are usable immediately.

## Run

```bash
# backend
cd backend
python3 -m venv .venv
source .venv/bin/activate
pip install -r requirements.txt
uvicorn app.main:app --reload --app-dir . --port 8000

# frontend
cd frontend
npm install
npm run dev
```

Open [http://localhost:5173](http://localhost:5173). Try `ORD → DCA` or `JFK → SFO`.

Copy `.env.example` to `.env` and add Amadeus Self-Service credentials to shop live GDS offers. Toggle **Live GDS** on the search form.

```bash
AMADEUS_CLIENT_ID=
AMADEUS_CLIENT_SECRET=
AMADEUS_HOSTNAME=test
```

## What the shop returns

1. **Nonstop A→B** — the local origin-destination product.
2. **Connecting to B** — itineraries that actually end at the intended city.
3. **Nearby airports** — same metro, different runway.
4. **Hidden-city A→B→C** — through fares cheaper than local A–B, preferably the same first flight.

Each inversion carries a risk matrix (baggage, gate-check, reroute, coupon sequence, documents, enforcement) and an estimated

\[
S_{\mathrm{net}} = P_{AB}-P_{ABC}-\text{fees}-E[\text{disruption}]-E[\text{enforcement}].
\]

Those expected costs are ranking aids, not a prediction of any airline’s action.

## Architecture

```
frontend (SvelteKit :5173)
    └── /api → FastAPI :8000
            ├── /airports
            ├── /search
            └── engines/shop.py
                    ├── DemoProvider   (instant O-D model)
                    ├── AmadeusProvider (optional live)
                    └── risk.py
```

Candidate C cities are not a brute-force world dump. They are ranked from fortress-hub structure and historically cited inversions (Luttmann–Gaggero-style US hubs; Zaman’s JFK–SFO–SEA), then shopped concurrently with a semaphore.

## Disclaimer

Airline conditions of carriage treat hidden-city and point-beyond ticketing as prohibited booking practices. This repository does not provide legal analysis, booking, or guidance on avoiding carrier review. Use it to understand why a journey with more flying can sell for less money.
