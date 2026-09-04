"""Load official public reference databases into PostgreSQL.

These are the source databases. Postgres is only the store.

- OurAirports countries.csv / regions.csv / airports.csv / runways.csv / navaids.csv
  Public domain, regenerated nightly: https://ourairports.com/data/
- GeoNames countryInfo.txt
  Capital, ISO3, currency, population, languages (CC BY): https://download.geonames.org/export/dump/
- OpenFlights airlines.dat / routes.dat
  Historical (~2014–2017), not a timetable.
"""

from __future__ import annotations

import csv
import io
from datetime import datetime, timezone

import httpx
from sqlalchemy import delete, text
from sqlalchemy.ext.asyncio import AsyncSession

from app.db.session import init_db, session_factory
from app.db.tables import (
    AirlineRow,
    AirportRow,
    ContinentRow,
    CountryRow,
    NavaidRow,
    RegionRow,
    RouteRow,
    RunwayRow,
)

OURAIRPORTS = "https://davidmegginson.github.io/ourairports-data"
GEONAMES_COUNTRIES = "https://download.geonames.org/export/dump/countryInfo.txt"
OPENFLIGHTS_AIRLINES = "https://raw.githubusercontent.com/jpatokal/openflights/master/data/airlines.dat"
OPENFLIGHTS_ROUTES = "https://raw.githubusercontent.com/jpatokal/openflights/master/data/routes.dat"

# OurAirports + GeoNames share this 7-code list. Codes themselves come from the dumps;
# names exist only here because neither file ships a continents.csv.
_CONTINENT_NAMES = {
    "AF": "Africa",
    "AN": "Antarctica",
    "AS": "Asia",
    "EU": "Europe",
    "NA": "North America",
    "OC": "Oceania",
    "SA": "South America",
}
TYPE_RANK = {"large_airport": 0, "medium_airport": 1, "small_airport": 2}
ROUTE_NOTE = (
    "OpenFlights Airline Route Mapper extract. Dataset last published around 2014–2017; "
    "not a current schedule, not availability, not a fare."
)
AIRLINE_NOTE = (
    "OpenFlights airlines dump (last material update ~2017). Use IATA/ICAO as identifiers, "
    "not as proof the carrier still operates a given route."
)


def _get(url: str) -> str:
    with httpx.Client(timeout=180.0, follow_redirects=True) as client:
        r = client.get(url)
        r.raise_for_status()
        return r.text


def _int(v: str | None) -> int | None:
    if v is None or v == "" or v == "\\N":
        return None
    try:
        return int(float(v))
    except ValueError:
        return None


def _float(v: str | None) -> float | None:
    if v is None or v == "" or v == "\\N":
        return None
    try:
        return float(v)
    except ValueError:
        return None


def _geonames(text: str) -> dict[str, dict]:
    out: dict[str, dict] = {}
    for line in text.splitlines():
        if not line or line.startswith("#"):
            continue
        cols = line.split("\t")
        if len(cols) < 15:
            continue
        iso2 = cols[0].strip().upper()
        if len(iso2) != 2:
            continue
        out[iso2] = {
            "iso3": cols[1].strip().upper() or None,
            "iso_numeric": cols[2].strip() or None,
            "name": cols[4].strip(),
            "capital": cols[5].strip(),
            "area_km2": _float(cols[6]),
            "population": _int(cols[7]),
            "continent": cols[8].strip().upper(),
            "tld": cols[9].strip(),
            "currency_code": cols[10].strip().upper(),
            "currency_name": cols[11].strip(),
            "phone": cols[12].strip(),
            "languages": cols[15].strip() if len(cols) > 15 else "",
            "geoname_id": _int(cols[16]) if len(cols) > 16 else None,
        }
    return out


def _best_airports(text: str) -> dict[str, dict]:
    reader = csv.DictReader(io.StringIO(text))
    best: dict[str, dict] = {}
    for row in reader:
        iata = (row.get("iata_code") or "").strip().upper()
        if len(iata) != 3 or not iata.isalpha():
            continue
        lat, lon = _float(row.get("latitude_deg")), _float(row.get("longitude_deg"))
        if lat is None or lon is None:
            continue
        typ = row.get("type") or ""
        scheduled = (row.get("scheduled_service") or "").lower() == "yes"
        rank = (0 if scheduled else 1, TYPE_RANK.get(typ, 9))
        cand = {
            "iata": iata,
            "icao": (row.get("icao_code") or row.get("gps_code") or "").strip().upper() or None,
            "ident": (row.get("ident") or "").strip() or None,
            "name": (row.get("name") or iata).strip(),
            "municipality": (row.get("municipality") or "").strip(),
            "iso_country": (row.get("iso_country") or "").strip().upper(),
            "iso_region": (row.get("iso_region") or "").strip(),
            "continent": (row.get("continent") or "").strip().upper(),
            "lat": lat,
            "lon": lon,
            "elevation_ft": _int(row.get("elevation_ft")),
            "type": typ or "unknown",
            "scheduled_service": scheduled,
            "wikipedia": (row.get("wikipedia_link") or "").strip() or None,
            "home_link": (row.get("home_link") or "").strip() or None,
            "gps_code": (row.get("gps_code") or "").strip().upper() or None,
            "local_code": (row.get("local_code") or "").strip() or None,
            "keywords": (row.get("keywords") or "").strip() or None,
            "rank": rank,
        }
        prev = best.get(iata)
        if prev is None or cand["rank"] < prev["rank"]:
            best[iata] = cand
    return best


async def ingest(session: AsyncSession) -> dict[str, int]:
    now = datetime.now(timezone.utc)
    oa_countries = _get(f"{OURAIRPORTS}/countries.csv")
    oa_regions = _get(f"{OURAIRPORTS}/regions.csv")
    oa_airports = _get(f"{OURAIRPORTS}/airports.csv")
    oa_runways = _get(f"{OURAIRPORTS}/runways.csv")
    oa_navaids = _get(f"{OURAIRPORTS}/navaids.csv")
    geo = _geonames(_get(GEONAMES_COUNTRIES))
    airlines_dat = _get(OPENFLIGHTS_AIRLINES)
    routes_dat = _get(OPENFLIGHTS_ROUTES)

    parsed = _best_airports(oa_airports)

    await session.execute(delete(NavaidRow))
    await session.execute(delete(RunwayRow))
    await session.execute(delete(RouteRow))
    await session.execute(delete(AirlineRow))
    await session.execute(delete(AirportRow))
    await session.execute(delete(RegionRow))
    await session.execute(delete(CountryRow))
    await session.execute(delete(ContinentRow))
    await session.commit()

    countries: dict[str, CountryRow] = {}
    for row in csv.DictReader(io.StringIO(oa_countries)):
        iso2 = (row.get("code") or "").strip().upper()
        if len(iso2) != 2:
            continue
        g = geo.get(iso2, {})
        sources = "ourairports"
        if g:
            sources = "ourairports+geonames"
        countries[iso2] = CountryRow(
            iso2=iso2,
            iso3=g.get("iso3"),
            iso_numeric=g.get("iso_numeric"),
            name=(row.get("name") or g.get("name") or iso2).strip(),
            continent=(row.get("continent") or g.get("continent") or "").strip().upper(),
            capital=g.get("capital") or "",
            currency_code=g.get("currency_code") or "",
            currency_name=g.get("currency_name") or "",
            tld=g.get("tld") or "",
            phone=g.get("phone") or "",
            languages=g.get("languages") or "",
            population=g.get("population"),
            area_km2=g.get("area_km2"),
            geoname_id=g.get("geoname_id"),
            wikipedia=(row.get("wikipedia_link") or "").strip() or None,
            sources=sources,
            ingested_at=now,
        )
    # GeoNames-only territories missing from OurAirports
    for iso2, g in geo.items():
        if iso2 in countries:
            continue
        countries[iso2] = CountryRow(
            iso2=iso2,
            iso3=g.get("iso3"),
            iso_numeric=g.get("iso_numeric"),
            name=g.get("name") or iso2,
            continent=g.get("continent") or "",
            capital=g.get("capital") or "",
            currency_code=g.get("currency_code") or "",
            currency_name=g.get("currency_name") or "",
            tld=g.get("tld") or "",
            phone=g.get("phone") or "",
            languages=g.get("languages") or "",
            population=g.get("population"),
            area_km2=g.get("area_km2"),
            geoname_id=g.get("geoname_id"),
            wikipedia=None,
            sources="geonames",
            ingested_at=now,
        )
    session.add_all(list(countries.values()))
    codes = sorted({c.continent for c in countries.values() if c.continent})
    session.add_all(
        [
            ContinentRow(code=code, name=_CONTINENT_NAMES.get(code, code), source="ourairports+geonames")
            for code in codes
        ]
    )
    await session.commit()

    regions: dict[str, RegionRow] = {}
    for row in csv.DictReader(io.StringIO(oa_regions)):
        code = (row.get("code") or "").strip()
        if not code:
            continue
        regions[code] = RegionRow(
            code=code,
            local_code=(row.get("local_code") or "").strip(),
            name=(row.get("name") or code).strip(),
            iso_country=(row.get("iso_country") or "").strip().upper(),
            continent=(row.get("continent") or "").strip().upper(),
            wikipedia=(row.get("wikipedia_link") or "").strip() or None,
            source="ourairports",
        )
    session.add_all(list(regions.values()))
    await session.commit()

    airport_rows = []
    ident_to_iata: dict[str, str] = {}
    for a in parsed.values():
        country = countries.get(a["iso_country"])
        region = regions.get(a["iso_region"])
        airport_rows.append(
            AirportRow(
                iata=a["iata"],
                icao=a["icao"],
                ident=a["ident"],
                name=a["name"],
                municipality=a["municipality"],
                iso_country=a["iso_country"],
                iso_region=a["iso_region"],
                continent=a["continent"] or (country.continent if country else ""),
                country_name=country.name if country else a["iso_country"],
                region_name=region.name if region else "",
                lat=a["lat"],
                lon=a["lon"],
                elevation_ft=a["elevation_ft"],
                type=a["type"],
                scheduled_service=a["scheduled_service"],
                wikipedia=a["wikipedia"],
                home_link=a["home_link"],
                gps_code=a["gps_code"],
                local_code=a["local_code"],
                keywords=a["keywords"],
                source="ourairports",
                ingested_at=now,
            )
        )
        if a["ident"]:
            ident_to_iata[a["ident"]] = a["iata"]
    session.add_all(airport_rows)
    await session.commit()
    iata_set = set(parsed)

    runways: list[RunwayRow] = []
    for row in csv.DictReader(io.StringIO(oa_runways)):
        ident = (row.get("airport_ident") or "").strip()
        iata = ident_to_iata.get(ident)
        if not iata:
            continue
        rid = _int(row.get("id"))
        if rid is None:
            continue
        runways.append(
            RunwayRow(
                id=rid,
                airport_iata=iata,
                airport_ident=ident,
                length_ft=_int(row.get("length_ft")),
                width_ft=_int(row.get("width_ft")),
                surface=(row.get("surface") or "").strip(),
                lighted=(row.get("lighted") or "0") == "1",
                closed=(row.get("closed") or "0") == "1",
                le_ident=(row.get("le_ident") or "").strip(),
                he_ident=(row.get("he_ident") or "").strip(),
                source="ourairports",
            )
        )
    for i in range(0, len(runways), 3000):
        session.add_all(runways[i : i + 3000])
        await session.commit()

    navaids: list[NavaidRow] = []
    for row in csv.DictReader(io.StringIO(oa_navaids)):
        assoc = (row.get("associated_airport") or "").strip()
        if not assoc or assoc not in ident_to_iata:
            continue
        nid = _int(row.get("id"))
        if nid is None:
            continue
        navaids.append(
            NavaidRow(
                id=nid,
                ident=(row.get("ident") or "").strip(),
                name=(row.get("name") or "").strip(),
                type=(row.get("type") or "").strip(),
                frequency_khz=_int(row.get("frequency_khz")),
                iso_country=(row.get("iso_country") or "").strip().upper(),
                associated_airport=assoc or None,
                lat=_float(row.get("latitude_deg")),
                lon=_float(row.get("longitude_deg")),
                source="ourairports",
            )
        )
    for i in range(0, len(navaids), 3000):
        session.add_all(navaids[i : i + 3000])
        await session.commit()

    airlines: list[AirlineRow] = []
    for line in airlines_dat.splitlines():
        try:
            cols = next(csv.reader([line]))
        except Exception:
            continue
        if len(cols) < 8:
            continue
        iata = cols[3].strip().upper()
        icao = cols[4].strip().upper()
        if iata in {"", "\\N", "N/A"}:
            iata = None
        if icao in {"", "\\N", "N/A"}:
            icao = None
        if not iata and not icao:
            continue
        airlines.append(
            AirlineRow(
                iata=iata,
                icao=icao,
                name=cols[1].strip() or (iata or icao or "unknown"),
                country=cols[6].strip() if len(cols) > 6 else "",
                active=(cols[7].strip().upper() == "Y") if len(cols) > 7 else True,
                source="openflights",
                source_note=AIRLINE_NOTE,
            )
        )
    session.add_all(airlines)
    await session.commit()

    seen: set[tuple[str, str, str]] = set()
    routes: list[RouteRow] = []
    for line in routes_dat.splitlines():
        cols = line.split(",")
        if len(cols) < 6:
            continue
        airline = cols[0].strip().upper()
        origin = cols[2].strip().upper()
        dest = cols[4].strip().upper()
        if len(origin) != 3 or len(dest) != 3:
            continue
        if origin not in iata_set or dest not in iata_set or origin == dest:
            continue
        if len(airline) > 3 or airline in {"", "\\N"}:
            continue
        key = (airline, origin, dest)
        if key in seen:
            continue
        seen.add(key)
        routes.append(
            RouteRow(
                airline_iata=airline,
                origin_iata=origin,
                dest_iata=dest,
                codeshare=(len(cols) > 6 and cols[6].strip() == "Y"),
                stops=_int(cols[7]) or 0 if len(cols) > 7 else 0,
                equipment=cols[8].strip() if len(cols) > 8 else "",
                source="openflights",
                source_note=ROUTE_NOTE,
            )
        )
    for i in range(0, len(routes), 2000):
        session.add_all(routes[i : i + 2000])
        await session.commit()

    for table in (
        "continents",
        "countries",
        "regions",
        "airports",
        "runways",
        "navaids",
        "airlines",
        "routes",
    ):
        await session.execute(text(f"ANALYZE {table}"))
    await session.commit()

    return {
        "continents": 7,
        "countries": int((await session.execute(text("SELECT count(*) FROM countries"))).scalar_one()),
        "regions": int((await session.execute(text("SELECT count(*) FROM regions"))).scalar_one()),
        "airports": int((await session.execute(text("SELECT count(*) FROM airports"))).scalar_one()),
        "runways": int((await session.execute(text("SELECT count(*) FROM runways"))).scalar_one()),
        "navaids": int((await session.execute(text("SELECT count(*) FROM navaids"))).scalar_one()),
        "airlines": int((await session.execute(text("SELECT count(*) FROM airlines"))).scalar_one()),
        "routes": int((await session.execute(text("SELECT count(*) FROM routes"))).scalar_one()),
    }


async def main() -> None:
    await init_db()
    async with session_factory()() as session:
        print(await ingest(session))


if __name__ == "__main__":
    import asyncio

    asyncio.run(main())
