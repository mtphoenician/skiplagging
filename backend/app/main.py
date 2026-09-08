from __future__ import annotations

from contextlib import asynccontextmanager

import httpx
from fastapi import FastAPI, HTTPException, Query
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import ORJSONResponse

from app.cache import build_cache, search_key
from app.catalog import sources_payload
from app.config import get_settings
from app.db import repo
from app.db.session import init_db, session_factory
from app.engines.candidates import graph_snapshot
from app.engines.hidden import detect_hidden_city, refresh_keeps_hidden_city
from app.engines.shop import search_all_ways
from app.models import (
    Airport,
    Country,
    HiddenDeal,
    OfferRefreshRequest,
    OfferRefreshResponse,
    Region,
    SearchExpandRequest,
    SearchQuery,
    SearchResponse,
)
from app.providers.mock import MockProvider
from app.providers.aerodatabox import AeroDataBoxProvider
from app.providers.opensky import OpenSkyProvider


@asynccontextmanager
async def lifespan(app: FastAPI):
    settings = get_settings()
    app.state.settings = settings
    app.state.cache = build_cache(settings)
    app.state.http = httpx.AsyncClient(
        timeout=httpx.Timeout(25.0, connect=6.0),
        limits=httpx.Limits(max_connections=40, max_keepalive_connections=20),
    )
    await init_db()
    yield
    await app.state.http.aclose()


app = FastAPI(
    title="Skiplagging",
    description="Hidden-city discovery with a real airport database and strictly separated shop / book / track layers.",
    version="0.2.0",
    default_response_class=ORJSONResponse,
    lifespan=lifespan,
)

_settings = get_settings()
app.add_middleware(
    CORSMiddleware,
    allow_origins=_settings.origin_list,
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)


@app.get("/health")
async def health() -> dict:
    s = app.state.settings
    async with session_factory()() as session:
        db = await repo.counts(session)
    return {
        "ok": True,
        "database": "postgresql",
        "shop": {"amadeus": s.amadeus_enabled, "duffel": s.duffel_enabled},
        "track": {"opensky": True, "aerodatabox": s.aerodatabox_enabled},
        "tables": db,
    }


@app.get("/sources")
async def sources() -> dict:
    s = app.state.settings
    return {
        "layers": [
            "reference — OurAirports countries/regions/airports/runways/navaids + GeoNames countryInfo",
            "historical-route-map — who used to fly B→C (OpenFlights, stale)",
            "schedule-status — FIDS board (AeroDataBox if keyed)",
            "priced-offer — GDS/NDC shop (Amadeus/Duffel if keyed)",
            "order-pnr — not implemented; we never create a reservation",
            "live-track — ADS-B state vectors (OpenSky)",
            "meta-search / ota / airline-direct — booker deep-links only",
        ],
        "sources": sources_payload(
            {
                "amadeus": s.amadeus_enabled,
                "duffel": s.duffel_enabled,
                "aerodatabox": s.aerodatabox_enabled,
            }
        ),
    }


@app.get("/defaults")
async def defaults() -> dict:
    async with session_factory()() as session:
        origin, dest = await repo.default_pair(session)
    if not origin or not dest:
        raise HTTPException(status_code=503, detail="Route table is empty. Run python -m app.db.ingest")
    return {"origin": origin.model_dump(), "destination": dest.model_dump()}


@app.get("/continents")
async def continents() -> list[dict]:
    async with session_factory()() as session:
        return await repo.list_continents(session)


@app.get("/countries", response_model=list[Country])
async def countries(q: str = Query("", max_length=48)) -> list[Country]:
    async with session_factory()() as session:
        return await repo.search_countries(session, q)


@app.get("/countries/{iso2}", response_model=Country)
async def country(iso2: str) -> Country:
    async with session_factory()() as session:
        row = await repo.get_country(session, iso2)
    if not row:
        raise HTTPException(status_code=404, detail="Unknown ISO 3166-1 alpha-2 in OurAirports/GeoNames tables")
    return row


@app.get("/countries/{iso2}/regions", response_model=list[Region])
async def country_regions(iso2: str) -> list[Region]:
    async with session_factory()() as session:
        return await repo.regions_for(session, iso2)


@app.get("/airports", response_model=list[Airport])
async def airports(q: str = Query("", max_length=48)) -> list[Airport]:
    async with session_factory()() as session:
        hits = await repo.search_airports(session, q)
        if not hits and not q:
            raise HTTPException(
                status_code=503,
                detail="Airport table is empty. From backend/: python -m app.db.ingest",
            )
        return hits


@app.get("/airports/{iata}", response_model=Airport)
async def airport_detail(iata: str) -> Airport:
    async with session_factory()() as session:
        ap = await repo.get_place(session, iata, detail=True)
    if not ap:
        raise HTTPException(status_code=404, detail="Unknown IATA in OurAirports table")
    return ap


@app.get("/track/{iata}")
async def track(iata: str) -> dict:
    async with session_factory()() as session:
        ap = await repo.get_airport(session, iata)
    if not ap:
        raise HTTPException(status_code=404, detail="Unknown IATA in OurAirports table")
    traffic = await OpenSkyProvider(app.state.settings, app.state.http).traffic_near(ap)
    if traffic.aircraft:
        async with session_factory()() as session:
            await repo.persist_tracks(
                session, ap.iata, [a.model_dump() for a in traffic.aircraft], traffic.api_time
            )
    return traffic.model_dump()


@app.get("/board/{iata}")
async def board(iata: str) -> dict:
    s = app.state.settings
    if not s.aerodatabox_enabled:
        return {
            "airport": iata.upper(),
            "source": None,
            "flights": [],
            "gap": "RAPIDAPI_KEY not set. AeroDataBox FIDS is a schedule/status board, not a fare shop.",
        }
    flights = await AeroDataBoxProvider(s, app.state.http).board(iata.upper(), "Departure")
    return {"airport": iata.upper(), "source": "aerodatabox", "layer": "schedule-status", "flights": flights}


@app.get("/deals", response_model=list[HiddenDeal])
async def deals(
    limit: int = Query(48, ge=1, le=120),
    origin: str = Query("", max_length=3),
    dest: str = Query("", max_length=3),
) -> list[HiddenDeal]:
    async with session_factory()() as session:
        return await repo.list_hidden_deals(session, limit=limit, origin=origin, dest=dest)


def _key(query: SearchQuery) -> tuple:
    return search_key(
        query.origin,
        query.destination,
        query.date,
        query.adults,
        query.cabin,
        query.include_nearby,
        False,
        query.currency,
    )


@app.post("/search", response_model=SearchResponse)
async def search(query: SearchQuery) -> SearchResponse:
    """Fast pass: cache → index → direct A→B → top candidates. Returns `pending_candidates`
    for the deep pass so the browser can show fares before every probe has run."""
    key = _key(query)
    cached = app.state.cache.get(key)
    if cached is not None:
        if cached.search_debug:
            cached.search_debug.cache = "fresh"
        return cached
    async with session_factory()() as session:
        try:
            result = await search_all_ways(query, app.state.settings, app.state.http, session, mode="fast")
        except ValueError as exc:
            raise HTTPException(status_code=400, detail=str(exc)) from exc
    app.state.cache[key] = result
    return result


@app.post("/search/expand", response_model=SearchResponse)
async def search_expand(body: SearchExpandRequest) -> SearchResponse:
    """Deep pass: reuse everything the fast pass indexed, shop the remaining ranked
    candidates (minus `exclude`, the ones already probed), and replace the cached result."""
    key = _key(body.query)
    async with session_factory()() as session:
        try:
            result = await search_all_ways(
                body.query,
                app.state.settings,
                app.state.http,
                session,
                mode="deep",
                exclude=body.exclude,
            )
        except ValueError as exc:
            raise HTTPException(status_code=400, detail=str(exc)) from exc
    app.state.cache[key] = result
    return result


@app.post("/offers/refresh", response_model=OfferRefreshResponse)
async def refresh_offer(body: OfferRefreshRequest) -> OfferRefreshResponse:
    mock = MockProvider()
    fresh = await mock.refresh_offer(body.offer)
    if fresh is None:
        return OfferRefreshResponse(
            offer=None,
            valid=False,
            reason="Booking integration not configured for this provider. Confirm the fare on the booker link.",
        )
    intended = body.intended_destination
    if detect_hidden_city(body.offer, intended):
        if not refresh_keeps_hidden_city(body.offer, fresh, intended):
            return OfferRefreshResponse(
                offer=fresh,
                valid=False,
                reason="Refreshed itinerary no longer passes through the intended city.",
            )
    return OfferRefreshResponse(offer=fresh, valid=True, reason="")


@app.get("/debug/route-graph")
async def debug_route_graph(
    origin: str = Query("", max_length=3),
    intended: str = Query("", max_length=3),
    limit: int = Query(100, ge=1, le=500),
) -> dict:
    """What the planner has learned: (A, B, C) statistics and the flights seen inside tickets."""
    async with session_factory()() as session:
        stats = await repo.route_stats_snapshot(session, origin=origin, intended=intended, limit=limit)
        edges = await repo.route_edges_snapshot(session, origin=origin, limit=limit)
    return {
        "hidden_city_route_stats": stats,
        "route_edges": edges,
        "session_edges": graph_snapshot(),
        "note": "Learned from priced itineraries we actually saw. Not a licensed schedule, never a summed price.",
    }


@app.get("/debug/budget")
async def debug_budget(
    hours: int = Query(24, ge=1, le=24 * 30),
    origin: str = Query("", max_length=3),
    dest: str = Query("", max_length=3),
) -> dict:
    """Search budget: paid provider calls, cost, calls per search, and per-route provider scores."""
    async with session_factory()() as session:
        summary = await repo.budget_summary(session, hours=hours)
        if origin and dest:
            summary["provider_scores"] = await repo.provider_route_scores(session, origin, dest)
    summary["cost_per_call_usd"] = app.state.settings.search_cost_usd
    return summary


@app.get("/debug/price-history")
async def debug_price_history(fingerprint: str = Query(..., min_length=3, max_length=240)) -> dict:
    """Append-only fare observations for one itinerary fingerprint."""
    async with session_factory()() as session:
        rows = await repo.price_history(session, fingerprint)
    return {"fingerprint": fingerprint, "observations": rows}
