from __future__ import annotations

from contextlib import asynccontextmanager

import httpx
import orjson
from fastapi import FastAPI, HTTPException, Query
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import ORJSONResponse

from app.cache import build_cache, search_key
from app.config import get_settings
from app.engines.shop import search_all_ways
from app.graph.airports import AIRPORTS
from app.models import Airport, SearchQuery, SearchResponse


@asynccontextmanager
async def lifespan(app: FastAPI):
    settings = get_settings()
    app.state.settings = settings
    app.state.cache = build_cache(settings)
    app.state.http = httpx.AsyncClient(
        timeout=httpx.Timeout(20.0, connect=5.0),
        limits=httpx.Limits(max_connections=40, max_keepalive_connections=20),
    )
    yield
    await app.state.http.aclose()


app = FastAPI(
    title="Skiplagging",
    description="Hidden-city fare discovery and multi-channel ticket shopping.",
    version="0.1.0",
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
    return {
        "ok": True,
        "live_ready": s.amadeus_enabled,
        "airports": len(AIRPORTS),
    }


@app.get("/airports", response_model=list[Airport])
async def airports(q: str = Query("", max_length=48)) -> list[Airport]:
    needle = q.strip().lower()
    values = list(AIRPORTS.values())
    if not needle:
        return values
    hits = [
        a
        for a in values
        if needle in a.iata.lower()
        or needle in a.city.lower()
        or needle in a.name.lower()
        or needle in a.country.lower()
    ]
    hits.sort(key=lambda a: (a.iata != needle.upper(), a.city))
    return hits[:40]


@app.post("/search", response_model=SearchResponse)
async def search(query: SearchQuery) -> SearchResponse:
    key = search_key(
        query.origin,
        query.destination,
        query.date,
        query.adults,
        query.cabin,
        query.include_nearby,
        query.live,
    )
    cached = app.state.cache.get(key)
    if cached is not None:
        return cached
    try:
        result = await search_all_ways(query, app.state.settings, app.state.http)
    except ValueError as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc
    app.state.cache[key] = result
    return result


def dumps(v) -> bytes:
    return orjson.dumps(v)
