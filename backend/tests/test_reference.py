"""Live-database tests. Fail if ingest was skipped or data is still a hand list."""

from __future__ import annotations

import importlib.util

import pytest
from fastapi.testclient import TestClient

from app.db import repo
from app.db import session as db_session
from app.db.session import session_factory
from app.main import app
from app.providers.bookers import booker_links


@pytest.fixture(autouse=True)
async def _reset_engine():
    if db_session._engine is not None:
        await db_session._engine.dispose()
        db_session._engine = None
        db_session._factory = None
    yield
    if db_session._engine is not None:
        await db_session._engine.dispose()
        db_session._engine = None
        db_session._factory = None


@pytest.fixture
def client():
    with TestClient(app) as c:
        yield c


def test_hardcoded_graph_and_demo_are_gone():
    from pathlib import Path

    root = Path(__file__).resolve().parents[1] / "app"
    assert not (root / "graph" / "airports.py").exists()
    assert not (root / "graph" / "hubs.py").exists()
    assert not (root / "providers" / "demo.py").exists()
    # leftover empty package dirs must not provide a data module
    spec = importlib.util.find_spec("app.graph.airports")
    assert spec is None
    spec = importlib.util.find_spec("app.providers.demo")
    assert spec is None


def test_bookers_have_no_carrier_website_roster():
    src = open(booker_links.__code__.co_filename, encoding="utf-8").read()
    for banned in ("aa.com", "delta.com", "united.com", "lufthansa.com", "emirates.com"):
        assert banned not in src
    links = booker_links("XXX", "YYY", "2026-11-02", 1, [("ZZ", "Zed Air")])
    hosts = {u.url for u in links}
    assert any("XXX" in u and "YYY" in u and "2026-11-02" in u for u in hosts)
    assert any("Zed" in u for u in hosts)
    assert any(b.layer == "meta-search" for b in links)
    assert any(b.layer == "ota" and b.issues_ticket for b in links)
    names = {b.name for b in links}
    assert {
        "Google Flights",
        "Kayak",
        "Skyscanner",
        "Momondo",
        "Cheapflights",
        "Wego",
        "Expedia",
        "Booking.com",
        "Trip.com",
        "Priceline",
        "Kiwi.com",
        "CheapOair",
        "eDreams",
        "Traveloka",
        "MakeMyTrip",
        "Despegar",
        "Skiplagged.com",
    } <= names
    assert any("booking.com" in b.url and "XXX" in b.url and "YYY" in b.url for b in links)
    assert any("trip.com" in b.url and "xxx" in b.url and "yyy" in b.url for b in links)
    assert any("kiwi.com" in b.url and "XXX" in b.url for b in links)
    assert any("momondo.com" in b.url for b in links)


@pytest.mark.asyncio
async def test_reference_tables_are_populated():
    async with session_factory()() as session:
        counts = await repo.counts(session)
    assert counts["countries"] >= 200
    assert counts["regions"] >= 1000
    assert counts["airports"] >= 3000
    assert counts["runways"] >= 1000
    assert counts["continents"] >= 5
    assert counts["routes"] >= 10000


@pytest.mark.asyncio
async def test_continents_and_countries_come_from_tables():
    async with session_factory()() as session:
        names = await repo.continent_names(session)
        us = await repo.get_country(session, "US")
        fr = await repo.get_country(session, "FR")
        regions = await repo.regions_for(session, "US")
    assert names
    assert us and us.name and us.iso3 and us.capital and us.currency_code
    assert us.continent_name == names.get(us.continent)
    assert fr and fr.capital
    assert len(regions) >= 50


@pytest.mark.asyncio
async def test_airport_search_ranks_cities_not_substrings():
    async with session_factory()() as session:
        paris = await repo.search_airports(session, "paris")
        nyc = await repo.search_airports(session, "nyc")
        tokyo = await repo.search_airports(session, "tokyo")
        sao = await repo.search_airports(session, "sao paulo")
        rome = await repo.search_airports(session, "rome")
        nice = await repo.search_airports(session, "nice")
        london = await repo.search_airports(session, "london")
        ny = await repo.search_airports(session, "new york")
        stub = await repo.search_airports(session, "par")
    assert paris[0].type == "city" and paris[0].iata == "PAR"
    assert {"CDG", "ORY"} <= {a.iata for a in paris[1:4]}
    assert all("parish" not in (a.region_name or "").lower() or a.city.lower().startswith("paris") for a in paris)
    assert stub[0].type == "city" and stub[0].iata == "PAR"
    assert nyc[0].type == "city" and nyc[0].iata == "NYC"
    assert {a.iata for a in nyc[1:4]} >= {"JFK", "LGA", "EWR"}
    assert tokyo[0].type == "city" and tokyo[0].iata == "TYO"
    assert {a.iata for a in tokyo[1:3]} >= {"HND", "NRT"}
    assert sao[0].type == "city" and sao[0].iata == "SAO"
    assert {a.iata for a in sao[1:3]} >= {"GRU", "CGH"}
    assert rome[0].type == "city" and rome[0].iata == "ROM"
    assert nice[0].iata == "NCE" and nice[0].type != "city"
    assert london[0].type == "city" and london[0].iata == "LON"
    assert london[0].members and "LHR" in london[0].members
    assert ny[0].type == "city" and ny[0].iata == "NYC"


@pytest.mark.asyncio
async def test_country_lookup_is_exact_for_codes():
    async with session_factory()() as session:
        us = await repo.search_countries(session, "US")
        usa = await repo.search_countries(session, "USA")
        fr = await repo.search_countries(session, "FR")
        france = await repo.search_countries(session, "france")
        cote = await repo.search_countries(session, "cote")
        tokyo = await repo.search_countries(session, "tokyo")
        united = await repo.search_countries(session, "united")
        de = await repo.search_countries(session, "deutschland")
        ivory = await repo.search_countries(session, "ivory coast")
        uk = await repo.search_countries(session, "UK")
    assert len(us) == 1 and us[0].iso2 == "US"
    assert len(usa) == 1 and usa[0].iso3 == "USA"
    assert len(fr) == 1 and fr[0].iso2 == "FR"
    assert [c.iso2 for c in france] == ["FR"]
    assert cote and cote[0].iso2 == "CI"
    assert tokyo and tokyo[0].iso2 == "JP"
    assert {c.iso2 for c in united} >= {"US", "GB", "AE"}
    assert len(de) == 1 and de[0].iso2 == "DE"
    assert len(ivory) == 1 and ivory[0].iso2 == "CI"
    assert len(uk) == 1 and uk[0].iso2 == "GB"


@pytest.mark.asyncio
async def test_airport_search_by_country_and_region_names():
    async with session_factory()() as session:
        by_name = await repo.search_airports(session, "united states")
        by_region = await repo.search_airports(session, "illinois")
        france = await repo.search_airports(session, "france")
        pair = await repo.default_pair(session)
    assert by_name
    assert all(a.country_name for a in by_name)
    assert all(a.country == "US" for a in by_name)
    assert france and all(a.country == "FR" for a in france)
    assert {a.iata for a in france} & {"CDG", "ORY"}
    assert by_region
    assert any(a.region_name.lower() == "illinois" for a in by_region)
    origin, dest = pair
    assert origin and dest and origin.iata != dest.iata
    assert origin.country_name and dest.city


@pytest.mark.asyncio
async def test_airport_detail_has_real_runways():
    async with session_factory()() as session:
        # Pick a large US airport from the route graph, not a hardcoded code.
        origin, _ = await repo.default_pair(session)
        assert origin
        detail = await repo.get_airport(session, origin.iata, detail=True)
    assert detail
    assert detail.runways
    longest = max(r.length_ft or 0 for r in detail.runways)
    assert longest > 3000


@pytest.mark.asyncio
async def test_hidden_city_candidates_are_route_rows():
    async with session_factory()() as session:
        _, dest = await repo.default_pair(session)
        assert dest
        spokes = await repo.destinations_from(session, dest.iata, limit=12)
    assert spokes
    assert all(len(code) == 3 and airline for code, airline, _n in spokes)


def test_health_and_defaults_and_search(client: TestClient):
    health = client.get("/health").json()
    assert health["ok"] is True
    assert health["tables"]["countries"] >= 200
    assert health["database"] == "postgresql"

    defaults = client.get("/defaults")
    assert defaults.status_code == 200
    body = defaults.json()
    o, d = body["origin"]["iata"], body["destination"]["iata"]
    assert o != d
    assert body["origin"]["country_name"]

    countries = client.get("/countries", params={"q": body["origin"]["country_name"][:6]})
    assert countries.status_code == 200
    assert countries.json()
    us = client.get("/countries", params={"q": "US"})
    assert us.status_code == 200
    assert [c["iso2"] for c in us.json()] == ["US"]
    all_countries = client.get("/countries")
    assert all_countries.status_code == 200
    assert len(all_countries.json()) >= 200

    search = client.post(
        "/search",
        json={"origin": o, "destination": d, "date": "2026-11-02", "include_nearby": False},
    )
    assert search.status_code == 200
    data = search.json()
    assert data["origin"]["iata"] == o
    assert data["destination"]["country_name"]
    assert data["bookers"]
    assert all("aa.com" not in b["url"] for b in data["bookers"])
    assert data["connection_hints"]
    assert all(h["source"] in {"openflights-routes", "amadeus"} for h in data["connection_hints"])
    assert data["search_id"]


def test_track_uses_opensky_or_explains_gap(client: TestClient):
    defaults = client.get("/defaults").json()
    iata = defaults["origin"]["iata"]
    r = client.get(f"/track/{iata}")
    assert r.status_code == 200
    traffic = r.json()
    assert traffic["source"] == "opensky"
    assert traffic["trackers"]
    assert traffic["layer"] == "live-track"
    # Live ADS-B is best-effort; either aircraft or an explicit note.
    assert traffic["aircraft"] or traffic["note"]
