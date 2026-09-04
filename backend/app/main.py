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
from app.engines.shop import search_all_ways
from app.models import Airport, Country, Region, SearchQuery, SearchResponse
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
        ap = await repo.get_airport(session, iata, detail=True)
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


@app.post("/search", response_model=SearchResponse)
async def search(query: SearchQuery) -> SearchResponse:
    key = search_key(
        query.origin,
        query.destination,
        query.date,
        query.adults,
        query.cabin,
        query.include_nearby,
        False,
    )
    cached = app.state.cache.get(key)
    if cached is not None:
        return cached
    async with session_factory()() as session:
        try:
            result = await search_all_ways(query, app.state.settings, app.state.http, session)
        except ValueError as exc:
            raise HTTPException(status_code=400, detail=str(exc)) from exc
    app.state.cache[key] = result
    return result
