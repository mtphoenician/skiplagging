from __future__ import annotations

import re
import unicodedata
from math import asin, cos, radians, sin, sqrt

from sqlalchemy import Select, func, or_, select, text
from sqlalchemy.ext.asyncio import AsyncSession

from app.metros import METROS, catalog_code_for_member, normalize_place_id
from app.db.tables import (
    AirlineRow,
    AirportRow,
    ContinentRow,
    CountryRow,
    NavaidRow,
    OfferRow,
    RegionRow,
    RouteRow,
    RunwayRow,
    SearchRow,
    TrackRow,
)
from app.models import Airport, Country, Navaid, Region, Runway


async def continent_names(session: AsyncSession) -> dict[str, str]:
    rows = (await session.execute(select(ContinentRow))).scalars().all()
    return {r.code: r.name for r in rows}


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
    stmt = (
        select(RouteRow.dest_iata, RouteRow.airline_iata, func.count())
        .where(RouteRow.origin_iata == origin.upper(), RouteRow.stops == 0)
        .group_by(RouteRow.dest_iata, RouteRow.airline_iata)
        .order_by(func.count().desc())
        .limit(limit * 3)
    )
    rows = (await session.execute(stmt)).all()
    seen: dict[str, tuple[str, int]] = {}
    for dest, airline, n in rows:
        if dest not in seen:
            seen[dest] = (airline, int(n))
        if len(seen) >= limit:
            break
    return [(d, a, n) for d, (a, n) in seen.items()]


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
