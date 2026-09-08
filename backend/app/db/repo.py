from __future__ import annotations

import re
import unicodedata
from math import asin, cos, radians, sin, sqrt

from sqlalchemy import Select, and_, case, func, or_, select, text
from sqlalchemy.ext.asyncio import AsyncSession

from app.metros import METROS, catalog_code_for_member, normalize_place_id
from datetime import datetime, timedelta, timezone

from app.db.tables import (
    AirlineRow,
    AirportRow,
    ContinentRow,
    CountryRow,
    FareObservationRow,
    HiddenCityRouteStatRow,
    NavaidRow,
    OfferObservationRow,
    OfferRow,
    ProviderCallRow,
    RegionRow,
    RouteEdgeRow,
    RouteRow,
    RunwayRow,
    HiddenDealRow,
    SearchRow,
    TrackRow,
)
from app.models import Airport, BookerLink, Country, HiddenDeal, HiddenCityMatch, Navaid, Offer, Region, Runway


_CONTINENTS: dict[str, str] | None = None


async def continent_names(session: AsyncSession) -> dict[str, str]:
    global _CONTINENTS
    if _CONTINENTS is not None:
        return _CONTINENTS
    rows = (await session.execute(select(ContinentRow))).scalars().all()
    _CONTINENTS = {r.code: r.name for r in rows}
    return _CONTINENTS


async def airports_by_iata(session: AsyncSession, codes: list[str]) -> dict[str, Airport]:
    clean = list({c.upper()[:3] for c in codes if c})
    if not clean:
        return {}
    names = await continent_names(session)
    rows = (
        await session.execute(select(AirportRow).where(AirportRow.iata.in_(clean)))
    ).scalars().all()
    return {r.iata: _to_airport(r, continents=names) for r in rows}


def _to_airport(row: AirportRow, currency: str = "", continents: dict[str, str] | None = None) -> Airport:
    return Airport(
        iata=row.iata,
        place_id=row.iata,
        icao=row.icao,
        ident=row.ident,
        name=row.name,
        city=_city_label(row.municipality, row.name),
        country=row.iso_country,
        country_name=row.country_name or row.iso_country,
        region_code=row.iso_region,
        region_name=row.region_name,
        continent=row.continent,
        continent_name=(continents or {}).get(row.continent, row.continent),
        currency_code=currency,
        lat=row.lat,
        lon=row.lon,
        metro=row.municipality or row.iata,
        type=row.type,
        scheduled_service=row.scheduled_service,
        elevation_ft=row.elevation_ft,
        wikipedia=row.wikipedia,
        source="ourairports",
        hub_carriers=[],
    )


async def get_airport(session: AsyncSession, iata: str, detail: bool = False) -> Airport | None:
    row = await session.get(AirportRow, iata.upper())
    if not row:
        return None
    currency = ""
    if row.iso_country:
        country = await session.get(CountryRow, row.iso_country)
        if country:
            currency = country.currency_code
    names = await continent_names(session)
    ap = _to_airport(row, currency, names)
    if detail:
        ap.runways = await list_runways(session, row.iata)
        ap.navaids = await list_navaids(session, row.ident)
    return ap


async def get_place(session: AsyncSession, code: str, detail: bool = False) -> Airport | None:
    pid = normalize_place_id(code)
    if pid.startswith("CITY-"):
        return await get_metro(session, pid.split("-", 1)[1])
    metro = await get_metro(session, pid)
    if metro and metro.place_id == pid:
        return metro
    return await get_airport(session, pid, detail=detail)


async def get_metro(session: AsyncSession, code: str) -> Airport | None:
    code = code.upper()
    spec = METROS.get(code)
    names = await continent_names(session)
    if spec:
        city, iso2, members, _aliases = spec
        rows = await _existing_airports(session, members)
        if len(rows) < 2:
            return None
        return _metro_place(code, city, iso2, rows, names, session_country=await session.get(CountryRow, iso2))
    row = await session.get(AirportRow, code)
    if not row or not row.scheduled_service:
        return None
    cluster = await _city_cluster(session, row)
    if len(cluster) < 2:
        return None
    catalog = catalog_code_for_member(row.iata)
    if catalog:
        return await get_metro(session, catalog)
    return _metro_place(
        cluster[0].iata,
        _city_label(cluster[0].municipality, cluster[0].name),
        cluster[0].iso_country,
        cluster,
        names,
        session_country=await session.get(CountryRow, cluster[0].iso_country),
        force_city_prefix=True,
    )


async def _existing_airports(session: AsyncSession, codes: tuple[str, ...] | list[str]) -> list[AirportRow]:
    rows: list[AirportRow] = []
    for code in codes:
        row = await session.get(AirportRow, code)
        if row and row.scheduled_service:
            rows.append(row)
    return rows


async def _city_cluster(session: AsyncSession, seed: AirportRow) -> list[AirportRow]:
    city = _fold(_city_label(seed.municipality, seed.name))
    near = await nearby_airports(session, seed.iata, max_km=80.0)
    rows = [seed]
    have = {seed.iata}
    for ap in near:
        if ap.iata in have:
            continue
        if (ap.type or "") not in {"large_airport", "medium_airport"} or not ap.scheduled_service:
            continue
        raw = await session.get(AirportRow, ap.iata)
        if not raw:
            continue
        same_city = _fold(_city_label(raw.municipality, raw.name)) == city and raw.iso_country == seed.iso_country
        catalog = catalog_code_for_member(seed.iata)
        in_same_metro = catalog and catalog_code_for_member(raw.iata) == catalog
        if same_city or in_same_metro:
            rows.append(raw)
            have.add(raw.iata)
    rows.sort(key=lambda r: (0 if r.type == "large_airport" else 1, r.iata))
    return rows


def _metro_place(
    code: str,
    city: str,
    iso2: str,
    rows: list[AirportRow],
    continents: dict[str, str],
    session_country: CountryRow | None = None,
    force_city_prefix: bool = False,
) -> Airport:
    members = [r.iata for r in rows]
    airport_collision = any(r.iata == code for r in rows)
    place_id = f"CITY-{code}" if airport_collision or force_city_prefix else code
    lat = sum(r.lat for r in rows) / len(rows)
    lon = sum(r.lon for r in rows) / len(rows)
    return Airport(
        iata=code,
        place_id=place_id,
        name="All airports",
        city=city,
        country=iso2,
        country_name=(session_country.name if session_country else "") or iso2,
        continent=rows[0].continent,
        continent_name=continents.get(rows[0].continent, rows[0].continent),
        currency_code=session_country.currency_code if session_country else "",
        lat=lat,
        lon=lon,
        metro=city,
        type="city",
        scheduled_service=True,
        source="metro",
        members=members,
    )


def _metro_matches(needle: str, code: str, city: str, aliases: tuple[str, ...]) -> bool:
    q = _fold(needle)
    if not q:
        return False
    if q == _fold(code) or q == _fold(city):
        return True
    if len(q) >= 3 and (_has_phrase(city, needle) or _fold(city).startswith(q)):
        return True
    if any(q == _fold(a) for a in aliases):
        return True
    return len(q) >= 3 and any(_fold(a).startswith(q) or _has_phrase(a, needle) for a in aliases)


_TYPE_W = {
    "large_airport": 45,
    "medium_airport": 18,
    "small_airport": 0,
    "seaplane_base": -10,
    "heliport": -35,
    "balloonport": -40,
    "closed": -80,
}


def _fold(s: str) -> str:
    s = unicodedata.normalize("NFKD", s or "")
    s = "".join(ch for ch in s if not unicodedata.combining(ch))
    return s.lower().strip()


def _city_label(municipality: str, name: str) -> str:
    m = (municipality or "").strip()
    if "(" in m:
        m = m.split("(", 1)[0].strip()
    if "," in m:
        head, tail = m.split(",", 1)
        if len(head) >= 3:
            m = head.strip()
    return m or name


def _tokens(s: str) -> list[str]:
    return re.findall(r"[a-z0-9]+", _fold(s))


def _has_phrase(hay: str, needle: str) -> bool:
    """True when needle is a whole word/phrase, not a substring of another word."""
    n = _fold(needle)
    h = _fold(hay)
    if not n or not h:
        return False
    if h == n or h.startswith(n + " ") or h.startswith(n + "(") or h.startswith(n + ","):
        return True
    return bool(re.search(rf"(^|[^a-z0-9]){re.escape(n)}([^a-z0-9]|$)", h))


def _prefix_token(hay: str, needle: str) -> bool:
    n = _fold(needle)
    if len(n) < 4:
        return False
    return any(t.startswith(n) and 1 <= len(t) - len(n) <= 2 for t in _tokens(hay))


def _keyword_parts(raw: str | None) -> list[str]:
    if not raw:
        return []
    return [p.strip() for p in raw.replace(";", ",").split(",") if p.strip()]


def _score_airport(row: AirportRow, needle: str) -> int:
    q = _fold(needle)
    if not q:
        return 0
    iata = (row.iata or "").lower()
    icao = (row.icao or "").lower()
    city = row.municipality or ""
    name = row.name or ""
    city_label = _city_label(city, name)
    kws = _keyword_parts(row.keywords)
    score = 0

    if iata == q or icao == q:
        score += 1000
    elif iata.startswith(q) and len(q) >= 2:
        score += 720
    elif icao.startswith(q) and len(q) >= 4:
        score += 700

    if row.type not in {"large_airport", "medium_airport", "small_airport"} and iata != q and icao != q:
        return 0

    text_hit = False
    kw_folded = [_fold(k) for k in kws]
    if q in kw_folded:
        score += 460
        text_hit = True
    elif any(_has_phrase(k, q) and len(_tokens(k)) == 1 for k in kws):
        score += 240
        text_hit = True
    elif any(_has_phrase(k, q) for k in kws):
        score += 40
        text_hit = True

    if _fold(city_label) == q:
        score += 420
        text_hit = True
    elif _fold(city_label).startswith(q) and len(q) >= 2:
        score += 300
        text_hit = True
    elif _has_phrase(city_label, q) or _fold(city_label).startswith(q + " "):
        score += 300
        text_hit = True
    elif _has_phrase(city, q):
        score += 80
        text_hit = True
    elif _prefix_token(city_label, q):
        score += 60
        text_hit = True

    if _has_phrase(name, q):
        score += 160
        text_hit = True
    elif _fold(name).startswith(q + " ") or _fold(name).startswith(q + "-"):
        score += 100
        text_hit = True

    if len(q) >= 4 and _has_phrase(row.country_name or "", q):
        score += 90
        text_hit = True
    if len(q) == 2 and q == (row.iso_country or "").lower():
        score += 100
        text_hit = True
    if len(q) >= 5 and _has_phrase(row.region_name or "", q):
        score += 55
        text_hit = True

    if iata == q or icao == q or (iata.startswith(q) and len(q) >= 2):
        text_hit = True

    if not text_hit:
        return 0

    if row.scheduled_service:
        score += 120
        if row.type == "large_airport" and _fold(city_label) == q:
            score += 90
    else:
        score -= 90
    score += _TYPE_W.get(row.type, -15)
    return score


async def _search_candidates(session: AsyncSession, needle: str) -> list[AirportRow]:
    like = f"%{needle}%"
    code = needle.upper()
    short = len(needle) <= 3 and needle.isalpha()
    try:
        stmt = text(
            """
            SELECT iata FROM airports
            WHERE iata ILIKE :code_like
               OR icao ILIKE :code_like
               OR unaccent(municipality) ILIKE '%' || unaccent(:q) || '%'
               OR unaccent(name) ILIKE '%' || unaccent(:q) || '%'
               OR unaccent(coalesce(keywords, '')) ILIKE '%' || unaccent(:q) || '%'
               OR (:long AND unaccent(country_name) ILIKE '%' || unaccent(:q) || '%')
               OR (:region AND unaccent(region_name) ILIKE '%' || unaccent(:q) || '%'
                   AND unaccent(region_name) NOT ILIKE '%parish%')
               OR (:fuzzy AND similarity(unaccent(municipality), unaccent(:q)) > 0.4)
               OR (:fuzzy AND similarity(unaccent(name), unaccent(:q)) > 0.45)
            LIMIT 160
            """
        )
        codes = (
            await session.execute(
                stmt,
                {
                    "q": needle,
                    "code_like": f"{code}%" if short else code,
                    "long": len(needle) >= 4,
                    "region": len(needle) >= 5,
                    "fuzzy": len(needle) >= 5,
                },
            )
        ).scalars().all()
        if codes:
            rows = (
                await session.execute(select(AirportRow).where(AirportRow.iata.in_(list(codes))))
            ).scalars().all()
            if rows:
                return list(rows)
    except Exception:
        await session.rollback()
    stmt = select(AirportRow).where(
        or_(
            AirportRow.iata.ilike(f"{code}%" if short else code),
            AirportRow.icao.ilike(f"{code}%" if short else like),
            AirportRow.municipality.ilike(like),
            AirportRow.name.ilike(like),
            AirportRow.keywords.ilike(like),
            AirportRow.country_name.ilike(like) if len(needle) >= 4 else False,
            AirportRow.region_name.ilike(like) if len(needle) >= 5 else False,
            AirportRow.iso_country == code if len(needle) == 2 else False,
        )
    ).limit(160)
    return list((await session.execute(stmt)).scalars().all())


async def search_airports(session: AsyncSession, q: str, limit: int = 12) -> list[Airport]:
    needle = q.strip()
    names = await continent_names(session)
    if not needle:
        rows = (
            await session.execute(
                select(AirportRow)
                .where(AirportRow.scheduled_service.is_(True), AirportRow.type == "large_airport")
                .order_by(AirportRow.municipality)
                .limit(20)
            )
        ).scalars().all()
        return [_to_airport(r, continents=names) for r in rows]

    country_iso = await _matching_country_iso(session, needle)
    rows = await _search_candidates(session, needle)
    if country_iso:
        extras = (
            await session.execute(
                select(AirportRow).where(
                    AirportRow.iso_country == country_iso,
                    AirportRow.scheduled_service.is_(True),
                    AirportRow.type.in_(("large_airport", "medium_airport")),
                )
            )
        ).scalars().all()
        have = {r.iata for r in rows}
        for r in extras:
            if r.iata not in have:
                rows.append(r)
    # Accent-folded catch for rows SQL collation missed (São Paulo vs sao paulo).
    folded = _fold(needle)
    if folded != needle.lower() or " " in folded:
        extra = (
            await session.execute(
                select(AirportRow).where(
                    AirportRow.scheduled_service.is_(True),
                    AirportRow.type.in_(("large_airport", "medium_airport")),
                )
            )
        ).scalars().all()
        have = {r.iata for r in rows}
        for r in extra:
            if r.iata in have:
                continue
            blob = " ".join(
                _fold(x)
                for x in (r.municipality, r.name, r.keywords or "", r.country_name, r.region_name)
            )
            if folded in blob or _has_phrase(r.municipality, folded) or _has_phrase(r.name, folded):
                rows.append(r)
                have.add(r.iata)

    ranked = [(_score_airport(r, needle), r) for r in rows]
    if country_iso:
        ranked = [(s + (400 if r.iso_country == country_iso else 0), r) for s, r in ranked]
        in_country = [(s, r) for s, r in ranked if r.iso_country == country_iso]
        if in_country:
            ranked = in_country
    ranked = [(s, r) for s, r in ranked if s >= 50]

    qf = _fold(needle)
    city_hit = next((r for s, r in ranked if _fold(_city_label(r.municipality, r.name)) == qf and r.scheduled_service), None)
    siblings: set[str] = set()
    if city_hit:
        near = await nearby_airports(session, city_hit.iata, max_km=90.0)
        have = {r.iata for _, r in ranked}
        for ap in near:
            if ap.iata in have:
                continue
            if not ap.scheduled_service or (ap.type or "") not in {"large_airport", "medium_airport"}:
                continue
            raw = await session.get(AirportRow, ap.iata)
            if raw:
                ranked.append((_score_airport(raw, needle) + 160, raw))
                have.add(raw.iata)
                siblings.add(raw.iata)

    if any(_fold(_city_label(r.municipality, r.name)) == qf for _, r in ranked):
        kept = []
        for s, r in ranked:
            if r.iata in siblings:
                kept.append((s, r))
                continue
            if _fold(_city_label(r.municipality, r.name)) == qf or _has_phrase(r.municipality, needle) or _has_phrase(r.name, needle):
                kept.append((s, r))
                continue
            if any(_fold(k) == qf or _has_phrase(k, needle) for k in _keyword_parts(r.keywords)):
                kept.append((s, r))
                continue
            if qf in {(r.iata or "").lower(), (r.icao or "").lower()}:
                kept.append((s, r))
        ranked = kept or ranked

    exact_code = len(_fold(needle)) == 3 and any(_fold(needle) == (r.iata or "").lower() for _, r in ranked)
    ranked.sort(
        key=lambda item: (
            0 if item[1].scheduled_service or exact_code else 1,
            -item[0],
            item[1].iata,
        )
    )
    if not exact_code:
        scheduled = [(s, r) for s, r in ranked if r.scheduled_service]
        other = [(s, r) for s, r in ranked if not r.scheduled_service]
        ranked = scheduled + other[:2]
    airports = [_to_airport(r, continents=names) for _, r in ranked[:limit]]
    metros = await _metros_for_query(session, needle, airports)
    if not metros:
        return airports
    if exact_code:
        return (airports[:1] + metros + airports[1:])[:limit]
    return (metros + [a for a in airports if a.iata not in {m.place_id for m in metros}])[:limit]


async def _metros_for_query(session: AsyncSession, needle: str, hits: list[Airport]) -> list[Airport]:
    qf = _fold(needle)
    exact = len(qf) == 3 and any(_fold(h.iata) == qf for h in hits)
    out: list[Airport] = []
    seen: set[str] = set()

    async def add(metro: Airport | None) -> None:
        if metro and metro.place_id not in seen and len(metro.members) >= 2:
            seen.add(metro.place_id)
            out.append(metro)

    for code, (city, _iso2, members, aliases) in METROS.items():
        hit_member = any(h.iata in members for h in hits)
        if exact and hit_member:
            await add(await get_metro(session, code))
            continue
        if _metro_matches(needle, code, city, aliases):
            await add(await get_metro(session, code))

    if not out:
        groups: dict[tuple[str, str], list[Airport]] = {}
        for h in hits:
            if h.type == "city" or not h.scheduled_service:
                continue
            groups.setdefault((h.country, _fold(h.city)), []).append(h)
        for (_cc, cityf), group in groups.items():
            if len(group) < 2:
                continue
            if cityf != qf and not (len(qf) >= 3 and cityf.startswith(qf)):
                continue
            await add(await get_metro(session, group[0].iata))
    return out


async def nearby_airports(session: AsyncSession, iata: str, max_km: float = 90.0) -> list[Airport]:
    origin = await session.get(AirportRow, iata.upper())
    if not origin:
        return []
    # Bounding box then precise haversine in Python (few thousand large/medium).
    dlat = max_km / 111.0
    dlon = max_km / max(111.0 * abs(cos(radians(origin.lat))), 20.0)
    stmt = select(AirportRow).where(
        AirportRow.iata != origin.iata,
        AirportRow.scheduled_service.is_(True),
        AirportRow.lat.between(origin.lat - dlat, origin.lat + dlat),
        AirportRow.lon.between(origin.lon - dlon, origin.lon + dlon),
    )
    rows = (await session.execute(stmt)).scalars().all()
    names = await continent_names(session)
    out: list[Airport] = []
    for r in rows:
        if _haversine(origin.lat, origin.lon, r.lat, r.lon) <= max_km:
            out.append(_to_airport(r, continents=names))
    out.sort(key=lambda a: _haversine(origin.lat, origin.lon, a.lat, a.lon))
    return out[:6]


async def destinations_from(session: AsyncSession, origin: str, limit: int = 40) -> list[tuple[str, str, int]]:
    """Historical OpenFlights spokes: (dest, airline, route_count). Not a live schedule."""
    return await destinations_from_many(session, [origin], per_origin=limit)


async def destinations_from_many(
    session: AsyncSession, origins: list[str], per_origin: int = 18
) -> list[tuple[str, str, int]]:
    codes = [o.upper()[:3] for o in origins if o]
    if not codes:
        return []
    stmt = (
        select(RouteRow.origin_iata, RouteRow.dest_iata, RouteRow.airline_iata, func.count())
        .where(RouteRow.origin_iata.in_(codes), RouteRow.stops == 0)
        .group_by(RouteRow.origin_iata, RouteRow.dest_iata, RouteRow.airline_iata)
        .order_by(func.count().desc())
    )
    rows = (await session.execute(stmt)).all()
    seen: dict[str, set[str]] = {code: set() for code in codes}
    counts: dict[str, int] = {code: 0 for code in codes}
    out: list[tuple[str, str, int]] = []
    for origin, dest, airline, n in rows:
        used = seen.get(origin)
        if used is None or dest in used or counts[origin] >= per_origin:
            continue
        used.add(dest)
        counts[origin] += 1
        out.append((dest, airline, int(n)))
    return out


async def airline_by_icao(session: AsyncSession, icao: str) -> AirlineRow | None:
    stmt = select(AirlineRow).where(AirlineRow.icao == icao.upper()).limit(1)
    return (await session.execute(stmt)).scalars().first()


async def list_runways(session: AsyncSession, iata: str) -> list[Runway]:
    rows = (
        await session.execute(select(RunwayRow).where(RunwayRow.airport_iata == iata.upper()).order_by(RunwayRow.length_ft.desc()))
    ).scalars().all()
    return [
        Runway(
            id=r.id,
            length_ft=r.length_ft,
            width_ft=r.width_ft,
            surface=r.surface,
            lighted=r.lighted,
            closed=r.closed,
            le_ident=r.le_ident,
            he_ident=r.he_ident,
        )
        for r in rows
    ]


async def list_navaids(session: AsyncSession, ident: str | None) -> list[Navaid]:
    if not ident:
        return []
    rows = (
        await session.execute(select(NavaidRow).where(NavaidRow.associated_airport == ident).limit(20))
    ).scalars().all()
    return [Navaid(ident=r.ident, name=r.name, type=r.type, frequency_khz=r.frequency_khz) for r in rows]


def _to_country(row: CountryRow, airport_count: int = 0, continents: dict[str, str] | None = None) -> Country:
    return Country(
        iso2=row.iso2,
        iso3=row.iso3,
        name=row.name,
        continent=row.continent,
        continent_name=(continents or {}).get(row.continent, row.continent),
        capital=row.capital,
        currency_code=row.currency_code,
        currency_name=row.currency_name,
        tld=row.tld,
        phone=row.phone,
        languages=row.languages,
        population=row.population,
        area_km2=row.area_km2,
        wikipedia=row.wikipedia,
        airport_count=airport_count,
        sources=row.sources,
    )


# Official GeoNames names miss the labels people actually type.
_COUNTRY_ALIASES = {
    "deutschland": "DE",
    "ivory": "CI",
    "ivory coast": "CI",
    "holland": "NL",
    "uk": "GB",
    "england": "GB",
    "britain": "GB",
    "great britain": "GB",
    "uae": "AE",
    "korea": "KR",
    "south korea": "KR",
    "north korea": "KP",
    "czech": "CZ",
    "czechia": "CZ",
    "czech republic": "CZ",
    "russia": "RU",
    "burma": "MM",
    "myanmar": "MM",
    "cape verde": "CV",
    "east timor": "TL",
    "swaziland": "SZ",
    "macedonia": "MK",
    "bolivia": "BO",
    "tanzania": "TZ",
    "iran": "IR",
    "laos": "LA",
    "syria": "SY",
    "vietnam": "VN",
    "venezuela": "VE",
}


def _score_country(row: CountryRow, needle: str) -> int:
    q = _fold(needle)
    if not q:
        return 1
    score = 0
    iso2, iso3 = _fold(row.iso2), _fold(row.iso3 or "")
    name, capital = row.name or "", row.capital or ""
    if iso2 == q:
        score += 1000
    if iso3 == q:
        score += 900
    if _fold(name) == q:
        score += 800
    elif _fold(name).startswith(q):
        score += 520
    elif _has_phrase(name, q):
        score += 320
    if _fold(capital) == q:
        score += 450
    elif _has_phrase(capital, q):
        score += 180
    if len(q) == 3 and _fold(row.currency_code) == q:
        score += 60
    if _COUNTRY_ALIASES.get(q) == row.iso2:
        score += 1000
    return score


async def search_countries(session: AsyncSession, q: str = "", limit: int = 300) -> list[Country]:
    counts_stmt = (
        select(AirportRow.iso_country, func.count())
        .where(AirportRow.scheduled_service.is_(True))
        .group_by(AirportRow.iso_country)
    )
    count_map = {iso: int(n) for iso, n in (await session.execute(counts_stmt)).all()}
    rows = (await session.execute(select(CountryRow))).scalars().all()
    names = await continent_names(session)
    needle = q.strip()
    if not needle:
        rows = sorted(rows, key=lambda r: r.name)
        return [_to_country(r, count_map.get(r.iso2, 0), names) for r in rows[:limit]]

    qf = _fold(needle)
    ranked = [(_score_country(r, needle), r) for r in rows]
    ranked = [(s, r) for s, r in ranked if s > 0]
    alias = _COUNTRY_ALIASES.get(qf)
    exact_name = [(s, r) for s, r in ranked if _fold(r.name) == qf]
    if alias:
        exact = [(s, r) for s, r in ranked if r.iso2 == alias]
        if exact:
            ranked = exact
    elif exact_name:
        ranked = exact_name
    elif len(qf) == 2:
        exact = [(s, r) for s, r in ranked if _fold(r.iso2) == qf]
        if exact:
            ranked = exact
    elif len(qf) == 3:
        exact = [(s, r) for s, r in ranked if _fold(r.iso3 or "") == qf]
        if exact:
            ranked = exact
    ranked.sort(key=lambda item: (-item[0], item[1].name))
    return [_to_country(r, count_map.get(r.iso2, 0), names) for _, r in ranked[:limit]]


async def _matching_country_iso(session: AsyncSession, needle: str) -> str | None:
    qf = _fold(needle)
    alias = _COUNTRY_ALIASES.get(qf)
    if alias:
        return alias
    rows = (await session.execute(select(CountryRow))).scalars().all()
    ranked = [(_score_country(r, needle), r) for r in rows]
    ranked = [(s, r) for s, r in ranked if s > 0]
    if not ranked:
        return None
    if len(qf) == 2:
        exact = [r for _, r in ranked if _fold(r.iso2) == qf]
        if exact:
            return exact[0].iso2
    elif len(qf) == 3:
        exact = [r for _, r in ranked if _fold(r.iso3 or "") == qf]
        if exact:
            return exact[0].iso2
    ranked.sort(key=lambda item: (-item[0], item[1].name))
    top = ranked[0][1]
    if qf in {_fold(top.iso2), _fold(top.iso3 or ""), _fold(top.name)}:
        return top.iso2
    return None


async def get_country(session: AsyncSession, iso2: str) -> Country | None:
    row = await session.get(CountryRow, iso2.upper())
    if not row:
        return None
    n = int(
        (
            await session.execute(
                select(func.count())
                .select_from(AirportRow)
                .where(AirportRow.iso_country == row.iso2, AirportRow.scheduled_service.is_(True))
            )
        ).scalar_one()
    )
    names = await continent_names(session)
    return _to_country(row, n, names)


async def regions_for(session: AsyncSession, iso2: str) -> list[Region]:
    country = await session.get(CountryRow, iso2.upper())
    rows = (
        await session.execute(
            select(RegionRow).where(RegionRow.iso_country == iso2.upper()).order_by(RegionRow.name)
        )
    ).scalars().all()
    return [
        Region(
            code=r.code,
            local_code=r.local_code,
            name=r.name,
            iso_country=r.iso_country,
            country_name=country.name if country else r.iso_country,
            continent=r.continent,
        )
        for r in rows
    ]


async def list_continents(session: AsyncSession) -> list[dict]:
    rows = (await session.execute(select(ContinentRow).order_by(ContinentRow.name))).scalars().all()
    out = []
    for r in rows:
        n = int(
            (
                await session.execute(
                    select(func.count()).select_from(CountryRow).where(CountryRow.continent == r.code)
                )
            ).scalar_one()
        )
        out.append({"code": r.code, "name": r.name, "country_count": n, "source": r.source})
    return out


async def counts(session: AsyncSession) -> dict[str, int]:
    async def c(model) -> int:
        return int((await session.execute(select(func.count()).select_from(model))).scalar_one())

    return {
        "continents": await c(ContinentRow),
        "countries": await c(CountryRow),
        "regions": await c(RegionRow),
        "airports": await c(AirportRow),
        "runways": await c(RunwayRow),
        "navaids": await c(NavaidRow),
        "airlines": await c(AirlineRow),
        "routes": await c(RouteRow),
        "hidden_deals": await c(HiddenDealRow),
        "fare_observations": await c(FareObservationRow),
        "route_edges": await c(RouteEdgeRow),
        "hidden_city_route_stats": await c(HiddenCityRouteStatRow),
        "provider_calls": await c(ProviderCallRow),
    }


async def persist_search(
    session: AsyncSession,
    *,
    origin: str,
    destination: str,
    date: str,
    adults: int,
    cabin: str,
    currency: str,
    elapsed_ms: float,
    sources: list[str],
    offers: list[dict],
) -> int:
    row = SearchRow(
        origin=origin,
        destination=destination,
        date=date,
        adults=adults,
        cabin=cabin,
        currency=currency,
        elapsed_ms=elapsed_ms,
        sources=sources,
    )
    session.add(row)
    await session.flush()
    for o in offers:
        session.add(
            OfferRow(
                search_id=row.id,
                offer_uid=str(o.get("id") or ""),
                source=str(o.get("source") or ""),
                layer=str(o.get("layer") or "priced-offer"),
                kind=str(o.get("kind") or ""),
                price=o.get("price"),
                currency=o.get("currency"),
                payload=o,
            )
        )
    await session.commit()
    return row.id


async def remember_offers(
    session: AsyncSession,
    offers: list[Offer],
    *,
    date: str,
    adults: int,
    cabin: str,
) -> int:
    from app.engines.index import observation_meta
    from sqlalchemy.dialects.postgresql import insert as pg_insert

    stored = 0
    for offer in offers:
        meta = observation_meta(offer)
        if meta is None:
            continue
        stmt = pg_insert(OfferObservationRow).values(
            origin=meta["origin"],
            ticketed=meta["ticketed"],
            connections=meta["connections"],
            date=date,
            adults=adults,
            cabin=cabin,
            currency=meta["currency"],
            source=meta["source"],
            fingerprint=meta["fingerprint"],
            price=meta["price"],
            payload=offer.model_dump(),
            observed_at=datetime.now(timezone.utc),
        )
        stmt = stmt.on_conflict_do_update(
            constraint="uq_offer_obs",
            set_={
                "price": stmt.excluded.price,
                "payload": stmt.excluded.payload,
                "connections": stmt.excluded.connections,
                "observed_at": stmt.excluded.observed_at,
                "currency": stmt.excluded.currency,
            },
        )
        await session.execute(stmt)
        stored += 1
    if stored:
        await session.commit()
    return stored


async def recall_offers(
    session: AsyncSession,
    *,
    origins: set[str],
    date: str,
    adults: int,
    cabin: str,
    max_age_seconds: int,
) -> list[Offer]:
    if not origins:
        return []
    cutoff = datetime.now(timezone.utc) - timedelta(seconds=max_age_seconds)
    codes = [o.upper()[:3] for o in origins]
    rows = (
        await session.execute(
            select(OfferObservationRow)
            .where(
                OfferObservationRow.origin.in_(codes),
                OfferObservationRow.date == date,
                OfferObservationRow.adults == adults,
                OfferObservationRow.cabin == cabin,
                OfferObservationRow.observed_at >= cutoff,
            )
            .order_by(OfferObservationRow.observed_at.desc())
            .limit(80)
        )
    ).scalars().all()
    seen: set[str] = set()
    out: list[Offer] = []
    for row in rows:
        if row.fingerprint in seen:
            continue
        seen.add(row.fingerprint)
        try:
            out.append(Offer.model_validate(row.payload))
        except (TypeError, ValueError, KeyError):
            continue
    return out


async def recall_candidate_dests(
    session: AsyncSession,
    *,
    origins: set[str],
    intended: set[str],
    date: str,
    adults: int,
    cabin: str,
    max_age_seconds: int,
    limit: int,
    any_date: bool = False,
) -> list[str]:
    """Ticketed destinations seen on A→B→C trips, even after the price is stale."""
    if not origins:
        return []
    cutoff = datetime.now(timezone.utc) - timedelta(seconds=max_age_seconds)
    codes = [o.upper()[:3] for o in origins]
    want = {c.upper() for c in intended}
    stmt = select(OfferObservationRow.ticketed, OfferObservationRow.connections).where(
        OfferObservationRow.origin.in_(codes),
        OfferObservationRow.observed_at >= cutoff,
    )
    if not any_date:
        stmt = stmt.where(
            OfferObservationRow.date == date,
            OfferObservationRow.adults == adults,
            OfferObservationRow.cabin == cabin,
        )
    rows = (await session.execute(stmt.order_by(OfferObservationRow.observed_at.desc()).limit(2000))).all()
    ranked: list[str] = []
    seen: set[str] = set()
    for ticketed, connections in rows:
        hops = {str(c).upper() for c in (connections or [])}
        if hops & want and ticketed.upper() not in want and ticketed.upper() not in seen:
            seen.add(ticketed.upper())
            ranked.append(ticketed.upper())
        if len(ranked) >= limit:
            break
    return ranked


async def persist_tracks(session: AsyncSession, airport_iata: str, states: list[dict], api_time: int | None) -> None:
    for s in states:
        session.add(
            TrackRow(
                airport_iata=airport_iata,
                icao24=s.get("icao24") or "",
                callsign=s.get("callsign"),
                origin_country=s.get("origin_country"),
                lat=s.get("lat"),
                lon=s.get("lon"),
                baro_altitude_m=s.get("baro_altitude_m"),
                on_ground=bool(s.get("on_ground")),
                velocity_ms=s.get("velocity_ms"),
                true_track=s.get("true_track"),
                position_source=s.get("position_source"),
                api_time=api_time,
            )
        )
    await session.commit()


async def airlines_by_iata(session: AsyncSession, codes: list[str]) -> list[tuple[str, str]]:
    clean = [c.upper() for c in codes if c and len(c) >= 2]
    if not clean:
        return []
    rows = (
        await session.execute(
            select(AirlineRow.iata, AirlineRow.name).where(AirlineRow.iata.in_(clean), AirlineRow.active.is_(True))
        )
    ).all()
    seen: set[str] = set()
    out: list[tuple[str, str]] = []
    for iata, name in rows:
        if iata and iata not in seen:
            seen.add(iata)
            out.append((iata, name))
    return out


async def default_pair(session: AsyncSession) -> tuple[Airport | None, Airport | None]:
    """Most-connected origin in the route table, then its busiest different-city dest."""
    stmt = (
        select(RouteRow.origin_iata, func.count().label("n"))
        .group_by(RouteRow.origin_iata)
        .order_by(func.count().desc())
        .limit(40)
    )
    ranked = [r[0] for r in (await session.execute(stmt)).all()]
    origin = None
    for iata in ranked:
        ap = await get_airport(session, iata)
        if ap and ap.scheduled_service:
            origin = ap
            break
    if not origin:
        return None, None
    dest = None
    for code, _, _ in await destinations_from(session, origin.iata, limit=20):
        d = await get_airport(session, code)
        if d and d.iata != origin.iata and (d.city != origin.city or d.country != origin.country):
            dest = d
            break
    return origin, dest


def _haversine(lat1: float, lon1: float, lat2: float, lon2: float) -> float:
    rlat1, rlon1, rlat2, rlon2 = map(radians, (lat1, lon1, lat2, lon2))
    dlon, dlat = rlon2 - rlon1, rlat2 - rlat1
    h = sin(dlat / 2) ** 2 + cos(rlat1) * cos(rlat2) * sin(dlon / 2) ** 2
    return 6371.0 * 2 * asin(sqrt(h))


async def persist_hidden_deals(
    session: AsyncSession,
    matches: list[HiddenCityMatch],
    *,
    origin: str,
    origin_city: str,
    dest: str,
    dest_city: str,
    date: str,
) -> int:
    saved = 0
    for match in matches:
        local = match.local_offer
        through = match.through_offer
        if (
            local.price is None
            or through.price is None
            or through.currency != local.currency
            or through.price >= local.price
        ):
            continue
        first = through.first_flight or ""
        existing = (
            await session.execute(
                select(HiddenDealRow).where(
                    HiddenDealRow.origin == origin,
                    HiddenDealRow.destination == dest,
                    HiddenDealRow.hidden_city == match.hidden_city,
                    HiddenDealRow.date == date,
                    HiddenDealRow.first_flight == first,
                )
            )
        ).scalar_one_or_none()
        fields = dict(
            origin_city=origin_city,
            dest_city=dest_city,
            hidden_city_name=match.hidden_city_name,
            honest_price=float(local.price),
            through_price=float(through.price),
            currency=match.currency,
            saving=float(match.gross_saving),
            saving_pct=float(match.saving_pct),
            source=through.source,
            bookers=[b.model_dump() for b in match.bookers],
            local_payload=local.model_dump(),
            through_payload=through.model_dump(),
        )
        if existing:
            for key, value in fields.items():
                setattr(existing, key, value)
        else:
            session.add(
                HiddenDealRow(
                    origin=origin,
                    destination=dest,
                    hidden_city=match.hidden_city,
                    date=date,
                    first_flight=first,
                    **fields,
                )
            )
        saved += 1
    if saved:
        await session.commit()
    return saved


def _deal_from_row(row: HiddenDealRow) -> HiddenDeal:
    bookers = [BookerLink.model_validate(b) for b in (row.bookers or [])]
    local = Offer.model_validate(row.local_payload) if row.local_payload else None
    through = Offer.model_validate(row.through_payload) if row.through_payload else None
    return HiddenDeal(
        id=row.id,
        origin=row.origin,
        origin_city=row.origin_city,
        dest=row.destination,
        dest_city=row.dest_city,
        hidden_city=row.hidden_city,
        hidden_city_name=row.hidden_city_name,
        date=row.date,
        honest_price=row.honest_price,
        through_price=row.through_price,
        currency=row.currency,
        saving=row.saving,
        saving_pct=row.saving_pct,
        first_flight=row.first_flight,
        source=row.source,
        bookers=bookers,
        local_offer=local,
        through_offer=through,
    )


async def list_hidden_deals(
    session: AsyncSession,
    *,
    limit: int = 48,
    origin: str = "",
    dest: str = "",
) -> list[HiddenDeal]:
    stmt = select(HiddenDealRow).order_by(HiddenDealRow.saving.desc(), HiddenDealRow.saving_pct.desc())
    if origin:
        stmt = stmt.where(HiddenDealRow.origin == origin.upper())
    if dest:
        stmt = stmt.where(HiddenDealRow.destination == dest.upper())
    rows = (await session.execute(stmt.limit(min(limit, 120)))).scalars().all()
    return [_deal_from_row(r) for r in rows]


# ── learned search intelligence ────────────────────────────────────────────────


async def route_map_from(session: AsyncSession, codes: list[str], per_origin: int = 60) -> dict[str, set[str]]:
    """Historical OpenFlights nonstop spokes B→C, keyed by B. A hint for the scorer, not a schedule."""
    want = [c.upper()[:3] for c in codes if c]
    if not want:
        return {}
    stmt = (
        select(RouteRow.origin_iata, RouteRow.dest_iata, func.count())
        .where(RouteRow.origin_iata.in_(want), RouteRow.stops == 0)
        .group_by(RouteRow.origin_iata, RouteRow.dest_iata)
        .order_by(func.count().desc())
    )
    rows = (await session.execute(stmt)).all()
    out: dict[str, set[str]] = {c: set() for c in want}
    for origin, dest, _n in rows:
        bucket = out.setdefault(origin, set())
        if len(bucket) < per_origin:
            bucket.add(dest)
    return out


async def load_route_stats(session: AsyncSession, origins: set[str], intended: set[str]):
    """hidden_city_route_stats rows for (A∈origins, B∈intended), merged per ticketed C."""
    from app.engines.candidates import RouteStat

    a_codes = [o.upper()[:3] for o in origins if o]
    b_codes = [b.upper()[:3] for b in intended if b]
    if not a_codes or not b_codes:
        return {}
    rows = (
        await session.execute(
            select(HiddenCityRouteStatRow).where(
                HiddenCityRouteStatRow.origin.in_(a_codes),
                HiddenCityRouteStatRow.intended.in_(b_codes),
            )
        )
    ).scalars().all()
    merged: dict[str, RouteStat] = {}
    weights: dict[str, int] = {}
    for row in rows:
        c = row.ticketed.upper()
        cur = merged.get(c)
        if cur is None:
            merged[c] = RouteStat(
                origin=row.origin,
                intended=row.intended,
                ticketed=c,
                observations=row.observations,
                successful_connections=row.successful_connections,
                cheaper_than_direct_count=row.cheaper_than_direct_count,
                median_saving=row.median_saving,
                average_saving_percent=row.average_saving_percent,
                last_success_at=row.last_success_at,
                last_cheaper_at=row.last_cheaper_at,
            )
            weights[c] = max(row.cheaper_than_direct_count, 0)
            continue
        cur.observations += row.observations
        cur.successful_connections += row.successful_connections
        w_old, w_new = weights[c], max(row.cheaper_than_direct_count, 0)
        if w_old + w_new > 0:
            cur.average_saving_percent = (
                cur.average_saving_percent * w_old + row.average_saving_percent * w_new
            ) / (w_old + w_new)
        weights[c] = w_old + w_new
        cur.cheaper_than_direct_count += row.cheaper_than_direct_count
        cur.median_saving = max(cur.median_saving, row.median_saving)
        for attr in ("last_success_at", "last_cheaper_at"):
            a, b = getattr(cur, attr), getattr(row, attr)
            if b is not None and (a is None or b > a):
                setattr(cur, attr, b)
    return merged


async def provider_rates_for(session: AsyncSession, origins: set[str], dests: list[str]) -> dict[str, float]:
    """Per candidate C: how often our suppliers answered A→C with offers (all providers pooled)."""
    from app.engines.budget import provider_score

    a_codes = [o.upper()[:3] for o in origins if o]
    d_codes = list({d.upper()[:3] for d in dests if d})
    if not a_codes or not d_codes:
        return {}
    stmt = (
        select(
            ProviderCallRow.dest,
            func.count(),
            func.sum(case((and_(ProviderCallRow.ok.is_(True), ProviderCallRow.offers > 0), 1), else_=0)),
            func.avg(ProviderCallRow.latency_ms),
        )
        .where(
            ProviderCallRow.origin.in_(a_codes),
            ProviderCallRow.dest.in_(d_codes),
            ProviderCallRow.cost_usd > 0,
        )
        .group_by(ProviderCallRow.dest)
    )
    rows = (await session.execute(stmt)).all()
    return {
        dest: provider_score(int(ok or 0), int(total or 0), float(avg or 0.0))
        for dest, total, ok, avg in rows
    }


async def provider_route_scores(session: AsyncSession, origin: str, dest: str) -> list[dict]:
    """ProviderScore(route, provider) — which supplier answers this pair best."""
    from app.engines.budget import provider_score

    stmt = (
        select(
            ProviderCallRow.provider,
            func.count(),
            func.sum(case((and_(ProviderCallRow.ok.is_(True), ProviderCallRow.offers > 0), 1), else_=0)),
            func.avg(ProviderCallRow.latency_ms),
            func.avg(ProviderCallRow.offers),
        )
        .where(ProviderCallRow.origin == origin.upper()[:3], ProviderCallRow.dest == dest.upper()[:3])
        .group_by(ProviderCallRow.provider)
    )
    rows = (await session.execute(stmt)).all()
    out = [
        {
            "provider": provider,
            "calls": int(total or 0),
            "with_offers": int(ok or 0),
            "avg_latency_ms": round(float(lat or 0.0), 1),
            "avg_offers": round(float(avg_offers or 0.0), 1),
            "score": provider_score(int(ok or 0), int(total or 0), float(lat or 0.0)),
        }
        for provider, total, ok, lat, avg_offers in rows
    ]
    out.sort(key=lambda r: -r["score"])
    return out


async def persist_provider_calls(session: AsyncSession, calls: list) -> int:
    if not calls:
        return 0
    for c in calls:
        session.add(
            ProviderCallRow(
                provider=c.provider,
                origin=c.origin[:3],
                dest=c.dest[:3],
                date=c.date,
                purpose=c.purpose,
                ok=c.ok,
                offers=c.offers,
                latency_ms=c.latency_ms,
                cost_usd=c.cost_usd,
            )
        )
    await session.commit()
    return len(calls)


async def budget_summary(session: AsyncSession, hours: int = 24) -> dict:
    since = datetime.now(timezone.utc) - timedelta(hours=hours)
    base = select(
        ProviderCallRow.provider,
        ProviderCallRow.purpose,
        func.count(),
        func.sum(ProviderCallRow.cost_usd),
        func.sum(case((and_(ProviderCallRow.ok.is_(True), ProviderCallRow.offers > 0), 1), else_=0)),
        func.avg(ProviderCallRow.latency_ms),
    ).where(ProviderCallRow.called_at >= since)
    rows = (await session.execute(base.group_by(ProviderCallRow.provider, ProviderCallRow.purpose))).all()
    by_row = [
        {
            "provider": p,
            "purpose": purpose,
            "calls": int(n or 0),
            "cost_usd": round(float(cost or 0.0), 4),
            "with_offers": int(ok or 0),
            "avg_latency_ms": round(float(lat or 0.0), 1),
        }
        for p, purpose, n, cost, ok, lat in rows
    ]
    searches = int(
        (
            await session.execute(
                select(func.count()).select_from(SearchRow).where(SearchRow.created_at >= since)
            )
        ).scalar_one()
    )
    total_calls = sum(r["calls"] for r in by_row)
    paid_calls = sum(r["calls"] for r in by_row if r["cost_usd"] > 0)
    return {
        "window_hours": hours,
        "searches": searches,
        "provider_calls": total_calls,
        "paid_calls": paid_calls,
        "cost_usd": round(sum(r["cost_usd"] for r in by_row), 4),
        "paid_calls_per_search": round(paid_calls / searches, 2) if searches else 0.0,
        "bookings": 0,
        "note": "This app never books; bookings stay 0. Booker clicks are outbound links.",
        "rows": by_row,
    }


async def apply_learning(session: AsyncSession, batch) -> dict[str, int]:
    """Persist a LearnBatch: append fares, upsert edges, fold stats. Never overwrite a price."""
    from app.engines.candidates import score_candidate, RouteStat
    from app.engines.learn import merge_savings, summarize_savings

    now = batch.observed_at
    written = {"fares": 0, "edges": 0, "stats": 0}

    for f in batch.fares:
        session.add(
            FareObservationRow(
                offer_uid=f.offer_uid[:80],
                fingerprint=f.fingerprint[:240],
                origin=f.origin[:3],
                ticketed=f.ticketed[:3],
                connections=f.connections,
                stops=f.stops,
                carrier=f.carrier[:8],
                provider=f.provider[:32],
                price=f.price,
                currency=f.currency[:8],
                date=f.date,
                adults=f.adults,
                cabin=f.cabin,
                fare_brand=(f.fare_brand or None) and f.fare_brand[:64],
                expires_at=(f.expires_at or None) and f.expires_at[:40],
                observed_at=now,
            )
        )
        written["fares"] += 1

    if batch.edges:
        origins = list({k[0] for k in batch.edges})
        dests = list({k[1] for k in batch.edges})
        existing = (
            await session.execute(
                select(RouteEdgeRow).where(RouteEdgeRow.origin.in_(origins), RouteEdgeRow.dest.in_(dests))
            )
        ).scalars().all()
        by_key = {(r.origin, r.dest, r.carrier, r.flight_number): r for r in existing}
        for key, edge in batch.edges.items():
            row = by_key.get(key)
            if row is None:
                session.add(
                    RouteEdgeRow(
                        origin=edge.origin[:3],
                        dest=edge.dest[:3],
                        carrier=edge.carrier[:8],
                        flight_number=edge.flight_number[:16],
                        observation_count=edge.count,
                        days_seen=len(edge.travel_dates),
                        travel_dates=sorted(edge.travel_dates),
                        first_seen=now,
                        last_seen=now,
                    )
                )
            else:
                dates = sorted(set(row.travel_dates or []) | edge.travel_dates)[-60:]
                row.observation_count = (row.observation_count or 0) + edge.count
                row.travel_dates = dates
                row.days_seen = len(dates)
                row.last_seen = now
            written["edges"] += 1

    if batch.stats:
        a_codes = list({k[0] for k in batch.stats})
        b_codes = list({k[1] for k in batch.stats})
        c_codes = list({k[2] for k in batch.stats})
        existing = (
            await session.execute(
                select(HiddenCityRouteStatRow).where(
                    HiddenCityRouteStatRow.origin.in_(a_codes),
                    HiddenCityRouteStatRow.intended.in_(b_codes),
                    HiddenCityRouteStatRow.ticketed.in_(c_codes),
                )
            )
        ).scalars().all()
        by_key = {(r.origin, r.intended, r.ticketed): r for r in existing}
        for key, d in batch.stats.items():
            row = by_key.get(key)
            if row is None:
                row = HiddenCityRouteStatRow(origin=d.origin[:3], intended=d.intended[:3], ticketed=d.ticketed[:3])
                session.add(row)
                by_key[key] = row
            prev_cheaper = row.cheaper_than_direct_count or 0
            row.observations = (row.observations or 0) + d.observations
            row.successful_connections = (row.successful_connections or 0) + d.successful_connections
            row.cheaper_than_direct_count = prev_cheaper + d.cheaper_than_direct_count
            row.observations = max(row.observations, row.successful_connections)
            row.successful_connections = max(row.successful_connections, row.cheaper_than_direct_count)
            row.success_rate = round(row.successful_connections / row.observations, 4) if row.observations else 0.0
            if d.savings:
                row.savings = merge_savings(list(row.savings or []), d.savings)
                avg, med, mx = summarize_savings(row.savings)
                row.average_saving, row.median_saving, row.maximum_saving = avg, med, mx
                n_new = len(d.saving_pcts)
                total = prev_cheaper + n_new
                row.average_saving_percent = round(
                    ((row.average_saving_percent or 0.0) * prev_cheaper + sum(d.saving_pcts)) / total, 2
                ) if total else 0.0
                row.currency = d.currency or row.currency
                row.last_cheaper_at = now
            if d.best_through_price is not None and (
                row.best_through_price is None or d.best_through_price < row.best_through_price
            ):
                row.best_through_price = d.best_through_price
            if d.success:
                row.last_success_at = now
            row.last_checked_at = now
            stat = RouteStat(
                origin=row.origin,
                intended=row.intended,
                ticketed=row.ticketed,
                observations=row.observations,
                successful_connections=row.successful_connections,
                cheaper_than_direct_count=row.cheaper_than_direct_count,
                median_saving=row.median_saving or 0.0,
                average_saving_percent=row.average_saving_percent or 0.0,
                last_success_at=row.last_success_at,
                last_cheaper_at=row.last_cheaper_at,
            )
            # Stored score is provider-neutral (hub 0, provider 0.5); the planner re-adds live parts.
            row.score, _ = score_candidate(stat, 0.0, 0.5, now)
            written["stats"] += 1

    if any(written.values()):
        await session.commit()
    return written


async def route_stats_snapshot(
    session: AsyncSession, *, origin: str = "", intended: str = "", limit: int = 100
) -> list[dict]:
    stmt = select(HiddenCityRouteStatRow).order_by(
        HiddenCityRouteStatRow.score.desc(), HiddenCityRouteStatRow.observations.desc()
    )
    if origin:
        stmt = stmt.where(HiddenCityRouteStatRow.origin == origin.upper()[:3])
    if intended:
        stmt = stmt.where(HiddenCityRouteStatRow.intended == intended.upper()[:3])
    rows = (await session.execute(stmt.limit(min(limit, 500)))).scalars().all()
    return [
        {
            "origin": r.origin,
            "intended_destination": r.intended,
            "ticketed_destination": r.ticketed,
            "observations": r.observations,
            "successful_connections": r.successful_connections,
            "success_rate": r.success_rate,
            "cheaper_than_direct_count": r.cheaper_than_direct_count,
            "average_saving": r.average_saving,
            "median_saving": r.median_saving,
            "maximum_saving": r.maximum_saving,
            "average_saving_percent": r.average_saving_percent,
            "currency": r.currency,
            "best_through_price": r.best_through_price,
            "score": r.score,
            "last_success_at": r.last_success_at.isoformat() if r.last_success_at else None,
            "last_cheaper_at": r.last_cheaper_at.isoformat() if r.last_cheaper_at else None,
            "last_checked_at": r.last_checked_at.isoformat() if r.last_checked_at else None,
        }
        for r in rows
    ]


async def route_edges_snapshot(session: AsyncSession, *, origin: str = "", limit: int = 100) -> list[dict]:
    stmt = select(RouteEdgeRow).order_by(RouteEdgeRow.observation_count.desc())
    if origin:
        stmt = stmt.where(RouteEdgeRow.origin == origin.upper()[:3])
    rows = (await session.execute(stmt.limit(min(limit, 500)))).scalars().all()
    return [
        {
            "origin": r.origin,
            "destination": r.dest,
            "carrier": r.carrier,
            "flight_number": r.flight_number,
            "observation_count": r.observation_count,
            "days_seen": r.days_seen,
            "first_seen": r.first_seen.isoformat() if r.first_seen else None,
            "last_seen": r.last_seen.isoformat() if r.last_seen else None,
        }
        for r in rows
    ]


async def price_history(session: AsyncSession, fingerprint: str, limit: int = 200) -> list[dict]:
    rows = (
        await session.execute(
            select(FareObservationRow)
            .where(FareObservationRow.fingerprint == fingerprint[:240])
            .order_by(FareObservationRow.observed_at.desc())
            .limit(min(limit, 1000))
        )
    ).scalars().all()
    return [
        {
            "price": r.price,
            "currency": r.currency,
            "provider": r.provider,
            "observed_at": r.observed_at.isoformat() if r.observed_at else None,
            "expires_at": r.expires_at,
        }
        for r in rows
    ]


async def busy_city_pairs(session: AsyncSession, limit: int = 24) -> list[tuple[str, str]]:
    stmt = (
        select(RouteRow.origin_iata, RouteRow.dest_iata, func.count())
        .where(RouteRow.stops == 0)
        .group_by(RouteRow.origin_iata, RouteRow.dest_iata)
        .order_by(func.count().desc())
        .limit(limit * 4)
    )
    rows = (await session.execute(stmt)).all()
    out: list[tuple[str, str]] = []
    seen: set[tuple[str, str]] = set()
    for origin, dest, _n in rows:
        if (origin, dest) in seen or origin == dest:
            continue
        o_ap = await get_airport(session, origin)
        d_ap = await get_airport(session, dest)
        if not o_ap or not d_ap or not o_ap.scheduled_service or not d_ap.scheduled_service:
            continue
        if (o_ap.type or "") not in {"large_airport", "medium_airport"}:
            continue
        if (d_ap.type or "") not in {"large_airport", "medium_airport"}:
            continue
        seen.add((origin, dest))
        out.append((origin, dest))
        if len(out) >= limit:
            break
    return out
