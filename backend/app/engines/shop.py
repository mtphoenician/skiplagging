from __future__ import annotations

import asyncio
import time
from collections.abc import Awaitable
from dataclasses import replace
from datetime import datetime

import httpx
from sqlalchemy.ext.asyncio import AsyncSession

from app.config import Settings
from app.db import repo
from app.engines.risk import attach_risk
from app.models import (
    BoardFlight,
    ChannelGroup,
    ConnectionHint,
    HiddenCityMatch,
    HonestPick,
    LiveTraffic,
    Offer,
    SearchQuery,
    SearchResponse,
)
from app.providers.aerodatabox import AeroDataBoxProvider
from app.providers.amadeus import AmadeusProvider
from app.providers.base import ShopRequest
from app.providers.bookers import booker_links
from app.providers.duffel import DuffelProvider
from app.providers.opensky import OpenSkyProvider


async def search_all_ways(
    query: SearchQuery,
    settings: Settings,
    client: httpx.AsyncClient,
    session: AsyncSession,
) -> SearchResponse:
    t0 = time.perf_counter()
    origin, dest = query.origin, query.destination
    query.origin, query.destination = origin, dest

    o_ap = await repo.get_place(session, origin)
    d_ap = await repo.get_place(session, dest)
    if not o_ap or not d_ap:
        raise ValueError(
            "Unknown IATA in the OurAirports table. Ingest has not been run, or the code is not a scheduled airport."
        )
    if _same_place(o_ap, d_ap):
        raise ValueError("Origin and destination must differ.")

    amadeus = AmadeusProvider(settings, client) if settings.amadeus_enabled else None
    duffel = DuffelProvider(settings, client) if settings.duffel_enabled else None
    opensky = OpenSkyProvider(settings, client)
    box = AeroDataBoxProvider(settings, client) if settings.aerodatabox_enabled else None

    o_tokens = shop_tokens(o_ap)
    d_tokens = shop_tokens(d_ap)
    o_members = list(o_ap.members or [o_ap.iata])
    d_members = list(d_ap.members or [d_ap.iata])
    o_airports = set(o_members)
    d_airports = set(d_members)
    req = ShopRequest(
        origin=o_tokens[0],
        dest=d_tokens[0],
        date=query.date,
        adults=query.adults,
        cabin=query.cabin,
        currency=query.currency,
        max_offers=20,
    )

    local_t = _shop_place(req, o_ap, d_ap, amadeus, duffel)
    nearby_t = (
        _shop_nearby(req, query, session, amadeus, duffel, o_members, d_members)
        if query.include_nearby
        else _empty_list()
    )
    hidden_t = _shop_hidden(req, query, settings, session, amadeus, duffel, o_ap, d_ap)
    hints_t = _connection_hints(session, origin, dest, amadeus, dest_members=list(d_airports))
    traffic_place_o = await repo.get_airport(session, (o_ap.members or [o_ap.iata])[0]) or o_ap
    traffic_place_d = await repo.get_airport(session, (d_ap.members or [d_ap.iata])[0]) or d_ap
    traffic_o_t = opensky.traffic_near(traffic_place_o)
    traffic_d_t = opensky.traffic_near(traffic_place_d)
    board_t = box.board((o_ap.members or [o_ap.iata])[0], "Departure") if box else _empty_list()

    gathered = await asyncio.gather(
        local_t, nearby_t, hidden_t, hints_t, traffic_o_t, traffic_d_t, board_t,
        return_exceptions=True,
    )
    local_offers = gathered[0] if isinstance(gathered[0], list) else []
    nearby_offers = gathered[1] if isinstance(gathered[1], list) else []
    hidden_matches = gathered[2] if isinstance(gathered[2], list) else []
    hints = gathered[3] if isinstance(gathered[3], list) else []
    traffic_o = gathered[4] if isinstance(gathered[4], LiveTraffic) else None
    traffic_d = gathered[5] if isinstance(gathered[5], LiveTraffic) else None
    board = gathered[6] if isinstance(gathered[6], list) else []

    local_offers = _dedupe(local_offers)
    nearby_offers = _dedupe(nearby_offers)
    hidden_matches.sort(key=lambda m: (-(m.gross_saving or 0), -m.risk.net_saving_estimate))

    priced = [o for o in local_offers if o.price is not None]
    nonstop = [o for o in priced if o.stops == 0]
    connecting = [o for o in priced if o.stops > 0]
    honest_pick = pick_honest(nonstop, connecting)
    hidden_if_cheaper = _hidden_if_cheaper(hidden_matches, honest_pick)
    compare_ccy = honest_pick.offer.currency if honest_pick else None
    local_cmp = [o for o in priced if compare_ccy is None or o.currency == compare_ccy]
    cheapest_local = min((o.price for o in local_cmp if o.price is not None), default=None)
    pool = local_cmp + [
        o for o in nearby_offers if o.price is not None and (compare_ccy is None or o.currency == compare_ccy)
    ] + [
        m.through_offer
        for m in hidden_matches
        if m.through_offer.price is not None and (compare_ccy is None or m.through_offer.currency == compare_ccy)
    ]
    cheapest_any = min((o.price for o in pool if o.price is not None), default=None)

    carriers = list({o.carrier for o in priced if o.carrier})
    airline_names = await repo.airlines_by_iata(session, carriers[:6])
    bookers = booker_links(o_ap.iata, d_ap.iata, query.date, query.adults, airline_names)
    for match in hidden_matches:
        thru_al = await repo.airlines_by_iata(session, [match.through_offer.carrier])
        match.bookers = booker_links(o_ap.iata, match.hidden_city, query.date, query.adults, thru_al)
    if hidden_matches:
        await repo.persist_hidden_deals(
            session,
            hidden_matches,
            origin=o_ap.iata[:3],
            origin_city=o_ap.city,
            dest=d_ap.iata[:3],
            dest_city=d_ap.city,
            date=query.date,
        )

    channels = [
        ChannelGroup(
            kind="nonstop",
            label="Priced nonstop A → B",
            blurb="GDS/NDC priced offers that end at your destination with one flight. A search offer is not a ticket.",
            offers=sorted(nonstop, key=lambda o: o.price or 0)[:10],
        ),
        ChannelGroup(
            kind="connecting",
            label="Priced connecting itineraries that end at B",
            blurb="Honest through products ticketed to B. Different from hidden-city A→B→C.",
            offers=sorted(connecting, key=lambda o: (o.price or 1e12, layover_minutes(o), o.duration_min))[:10],
        ),
        ChannelGroup(
            kind="nearby",
            label="Priced nearby-airport substitutes",
            blurb="Same trip intent, different IATA. Still a real A′→B′ offer, not skiplagging.",
            offers=sorted([o for o in nearby_offers if o.price is not None], key=lambda o: o.price or 0)[:8],
        ),
        ChannelGroup(
            kind="hidden-city",
            label="Priced hidden-city inversions",
            blurb="Only rows where a shop API returned P(A,B,C) < P(A,B) and the first sector is A→B.",
            offers=[m.through_offer for m in hidden_matches[:8]],
        ),
    ]

    sources_used = [
        "ourairports",
        "openflights-routes",
        "opensky",
        "google-flights",
        "kayak",
        "skyscanner",
        "momondo",
        "cheapflights",
        "wego",
        "expedia",
        "booking-com",
        "trip-com",
        "priceline",
        "kiwi",
        "cheapoair",
        "edreams",
        "traveloka",
        "makemytrip",
        "despegar",
        "skiplagged-com",
    ]
    if amadeus:
        sources_used.append("amadeus")
    if duffel:
        sources_used.append("duffel")
    if box:
        sources_used.append("aerodatabox")

    if traffic_o is None:
        traffic_o = LiveTraffic(
            airport=origin,
            source="opensky",
            api_time=None,
            note="OpenSky request failed (network or rate limit). Tracker deep-links remain valid.",
            aircraft=[],
            trackers=[],
        )
    if traffic_d is None:
        traffic_d = LiveTraffic(
            airport=dest,
            source="opensky",
            api_time=None,
            note="OpenSky request failed.",
            aircraft=[],
            trackers=[],
        )
    gaps = _gaps(amadeus, duffel, box, priced, hidden_matches, traffic_o, board)
    notes = [
        "Reference / track / schedule / priced-offer / booker are different layers. A tracker is not a ticket. A metasearch link is not a PNR.",
        "Hidden-city rows require two priced shop responses. Structural OpenFlights spokes are listed as connection hints, not savings.",
        "This API never calls Amadeus Flight Create Orders or Duffel Orders.",
    ]

    elapsed = round((time.perf_counter() - t0) * 1000, 1)
    persist_payload = [o.model_dump() for o in priced[:30]]
    persist_payload += [m.through_offer.model_dump() for m in hidden_matches[:15]]
    search_id = await repo.persist_search(
        session,
        origin=o_ap.iata[:3],
        destination=d_ap.iata[:3],
        date=query.date,
        adults=query.adults,
        cabin=query.cabin,
        currency=query.currency,
        elapsed_ms=elapsed,
        sources=sources_used,
        offers=persist_payload,
    )
    if traffic_o and traffic_o.aircraft:
        await repo.persist_tracks(
            session,
            (o_ap.members or [o_ap.iata])[0],
            [a.model_dump() for a in traffic_o.aircraft],
            traffic_o.api_time,
        )

    return SearchResponse(
        query=query,
        origin=o_ap,
        destination=d_ap,
        elapsed_ms=elapsed,
        search_id=search_id,
        sources_used=sources_used,
        data_gaps=gaps,
        cheapest_local=cheapest_local,
        cheapest_any=cheapest_any,
        honest_pick=honest_pick,
        hidden_if_cheaper=hidden_if_cheaper,
        channels=channels,
        hidden_city=hidden_matches[:20],
        connection_hints=hints,
        bookers=bookers,
        traffic_origin=traffic_o,
        traffic_destination=traffic_d,
        board_origin=board if isinstance(board, list) else [],
        notes=notes,
    )


async def _empty_list() -> list:
    return []


def shop_tokens(place) -> list[str]:
    members = [m for m in (place.members or []) if m]
    if getattr(place, "type", "") == "city":
        if place.iata and place.iata not in members:
            return [place.iata]
        return members[:5] or [place.iata]
    return [place.iata]


def _same_place(a, b) -> bool:
    if (a.place_id or a.iata) == (b.place_id or b.iata):
        return True
    am, bm = set(a.members or [a.iata]), set(b.members or [b.iata])
    if a.type == "city" and b.iata in am:
        return True
    if b.type == "city" and a.iata in bm:
        return True
    return False


async def _shop_place(
    req: ShopRequest,
    origin: object,
    dest: object,
    amadeus: AmadeusProvider | None,
    duffel: DuffelProvider | None,
) -> list[Offer]:
    offers = await _shop_pairs(req, shop_tokens(origin), shop_tokens(dest), amadeus, duffel)
    if offers:
        return offers
    o_m = list(getattr(origin, "members", None) or [origin.iata])[:4]
    d_m = list(getattr(dest, "members", None) or [dest.iata])[:4]
    if o_m != shop_tokens(origin) or d_m != shop_tokens(dest):
        return await _shop_pairs(req, o_m, d_m, amadeus, duffel)
    return []


async def _shop_pairs(
    req: ShopRequest,
    origins: list[str],
    dests: list[str],
    amadeus: AmadeusProvider | None,
    duffel: DuffelProvider | None,
) -> list[Offer]:
    pairs = [(o, d) for o in origins for d in dests if o != d][:6]
    if not pairs:
        return []

    async def one(o: str, d: str) -> list[Offer]:
        return await _shop_providers(replace(req, origin=o, dest=d), amadeus, duffel)

    parts = await asyncio.gather(*(one(o, d) for o, d in pairs), return_exceptions=True)
    out: list[Offer] = []
    for part in parts:
        if isinstance(part, list):
            out.extend(part)
    return out


async def _shop_providers(
    req: ShopRequest, amadeus: AmadeusProvider | None, duffel: DuffelProvider | None
) -> list[Offer]:
    tasks: list[Awaitable[list[Offer]]] = []
    if amadeus:
        tasks.append(amadeus.shop(req))
        if not req.nonstop:
            tasks.append(amadeus.shop(replace(req, nonstop=True, max_offers=8)))
    if duffel:
        tasks.append(duffel.shop(req))
    if not tasks:
        return []
    parts = await asyncio.gather(*tasks, return_exceptions=True)
    out: list[Offer] = []
    for part in parts:
        if isinstance(part, list):
            out.extend(part)
    return out


async def _shop_nearby(
    req: ShopRequest,
    query: SearchQuery,
    session: AsyncSession,
    amadeus: AmadeusProvider | None,
    duffel: DuffelProvider | None,
    origin_members: list[str] | None = None,
    dest_members: list[str] | None = None,
) -> list[Offer]:
    skip_o = set(origin_members or [req.origin])
    skip_d = set(dest_members or [req.dest])
    anchor_o = (origin_members or [req.origin])[0]
    anchor_d = (dest_members or [req.dest])[0]
    near_o = [a for a in await repo.nearby_airports(session, anchor_o) if a.iata not in skip_o]
    near_d = [a for a in await repo.nearby_airports(session, anchor_d) if a.iata not in skip_d]
    pairs: list[tuple[str, str]] = []
    for o in [req.origin, *[a.iata for a in near_o[:2]]]:
        for d in [req.dest, *[a.iata for a in near_d[:2]]]:
            if (o, d) != (req.origin, req.dest) and o != d:
                pairs.append((o, d))
    pairs = pairs[:5]
    if not pairs or (not amadeus and not duffel):
        return []

    async def one(o: str, d: str) -> list[Offer]:
        r = replace(req, origin=o, dest=d, nonstop=True, max_offers=5)
        offers = await _shop_providers(r, amadeus, duffel)
        for offer in offers:
            offer.kind = "nearby"
        return offers

    parts = await asyncio.gather(*(one(o, d) for o, d in pairs), return_exceptions=True)
    out: list[Offer] = []
    for part in parts:
        if isinstance(part, list):
            out.extend(part)
    return out


async def _shop_hidden(
    req: ShopRequest,
    query: SearchQuery,
    settings: Settings,
    session: AsyncSession,
    amadeus: AmadeusProvider | None,
    duffel: DuffelProvider | None,
    origin_place=None,
    dest_place=None,
) -> list[HiddenCityMatch]:
    if not amadeus and not duffel:
        return []
    origin_members = list((origin_place.members if origin_place else None) or [req.origin])
    dest_members = list((dest_place.members if dest_place else None) or [req.dest])
    origins = set(origin_members)
    dests = set(dest_members)
    if origin_place is not None and dest_place is not None:
        local_offers = await _shop_place(req, origin_place, dest_place, amadeus, duffel)
    else:
        local_offers = await _shop_providers(replace(req, nonstop=False, max_offers=15), amadeus, duffel)
    local_priced = [o for o in local_offers if o.price is not None]
    local = _best_local_honest(local_priced)
    if local is None:
        return []

    candidates = await _c_candidates(
        session, origins, dest_members, settings.max_hidden_candidates, amadeus
    )
    sem = asyncio.Semaphore(settings.max_concurrency)

    async def probe(c: str) -> HiddenCityMatch | None:
        async with sem:
            throughs = await _shop_providers(
                replace(req, dest=c, nonstop=False, max_offers=10),
                amadeus,
                duffel,
            )
            via = [o for o in throughs if _via_b(o, origins, dests, c) and o.price is not None]
            if not via:
                return None
            through = min(via, key=lambda o: o.price or 1e12)
            through.kind = "hidden-city"
            if (
                through.price is None
                or local.price is None
                or through.currency != local.currency
                or through.price >= local.price
            ):
                return None
            ap = await repo.get_airport(session, c)
            name = f"{ap.city} ({c})" if ap else c
            return attach_risk(
                match_id=f"hc-{through.id}-{c}",
                hidden_city=c,
                hidden_name=name,
                local=local,
                through=through,
                true_dest=req.dest,
                origin_country=getattr(origin_place, "country", "") or "",
                hidden_country=ap.country if ap else "",
            )

    parts = await asyncio.gather(*(probe(c) for c in candidates), return_exceptions=True)
    return [p for p in parts if isinstance(p, HiddenCityMatch)]


async def _c_candidates(
    session: AsyncSession,
    origin: str | set[str],
    dests: str | list[str],
    limit: int,
    amadeus: AmadeusProvider | None,
) -> list[str]:
    dest_list = [dests] if isinstance(dests, str) else list(dests)
    origin_codes = {origin} if isinstance(origin, str) else set(origin)
    seen = {c.upper() for c in origin_codes} | {d.upper() for d in dest_list}
    ranked: list[str] = []

    def push(codes: list[str]) -> None:
        for c in codes:
            c = c.upper()
            if len(c) == 3 and c not in seen:
                seen.add(c)
                ranked.append(c)

    if amadeus:
        for dest in dest_list[:3]:
            try:
                push(await amadeus.direct_destinations(dest))
            except httpx.HTTPError:
                pass
    for dest in dest_list[:4]:
        spokes = await repo.destinations_from(session, dest, limit=limit)
        push([d for d, _, _ in spokes if d not in seen])
    return ranked[:limit]


async def _connection_hints(
    session: AsyncSession,
    origin: str,
    dest: str,
    amadeus: AmadeusProvider | None,
    dest_members: list[str] | None = None,
) -> list[ConnectionHint]:
    hints: list[ConnectionHint] = []
    hubs = dest_members or [dest]
    spokes: list[tuple[str, str, int]] = []
    for hub in hubs[:4]:
        spokes.extend(await repo.destinations_from(session, hub, limit=18))
    for code, airline, n in spokes:
        if code == origin:
            continue
        ap = await repo.get_airport(session, code)
        hints.append(
            ConnectionHint(
                dest=code,
                dest_name=f"{ap.city} ({code})" if ap else code,
                evidence=f"{airline} appears on {n} OpenFlights B→C record(s)",
                source="openflights-routes",
                layer="historical-route-map",
                note="Historical route map (~2014–2017). Not a priced fare and not proof the flight operates tomorrow.",
            )
        )
    if amadeus:
        try:
            live = await amadeus.direct_destinations(dest)
        except httpx.HTTPError:
            live = []
        have = {h.dest for h in hints}
        for code in live:
            if code in have or code == origin:
                continue
            ap = await repo.get_airport(session, code)
            hints.append(
                ConnectionHint(
                    dest=code,
                    dest_name=f"{ap.city} ({code})" if ap else code,
                    evidence="Amadeus Airport Routes / direct-destinations from B",
                    source="amadeus",
                    layer="reference",
                    note="Published destination list, not a shopped fare.",
                )
            )
    return hints[:24]


def _via_b(offer: Offer, origin: str | set[str], dest_b: str | set[str], dest_c: str) -> bool:
    if len(offer.segments) < 2:
        return False
    origins = {o.upper() for o in ({origin} if isinstance(origin, str) else origin)}
    dests = {d.upper() for d in ({dest_b} if isinstance(dest_b, str) else dest_b)}
    first, last = offer.segments[0], offer.segments[-1]
    return (
        first.origin.upper() in origins
        and first.dest.upper() in dests
        and last.dest.upper() == dest_c.upper()
        and last.dest.upper() not in dests
    )


def _dedupe(offers: list[Offer]) -> list[Offer]:
    seen: set[tuple] = set()
    out: list[Offer] = []
    for o in sorted(offers, key=lambda x: x.price if x.price is not None else 1e12):
        key = (o.source, o.first_flight, o.price, tuple((s.origin, s.dest, s.flight_number) for s in o.segments))
        if key in seen:
            continue
        seen.add(key)
        out.append(o)
    return out


def _parse_leg_time(value: str) -> datetime | None:
    if not value:
        return None
    raw = value.replace("Z", "+00:00")
    try:
        return datetime.fromisoformat(raw)
    except ValueError:
        return None


def layover_minutes(offer: Offer) -> int:
    if len(offer.segments) < 2:
        return 0
    total = 0
    parsed = 0
    for first, second in zip(offer.segments, offer.segments[1:]):
        arr = _parse_leg_time(first.arr)
        dep = _parse_leg_time(second.dep)
        if arr is None or dep is None:
            continue
        if arr.tzinfo is None and dep.tzinfo is not None:
            dep = dep.replace(tzinfo=None)
        elif arr.tzinfo is not None and dep.tzinfo is None:
            arr = arr.replace(tzinfo=None)
        gap = int((dep - arr).total_seconds() // 60)
        if gap > 0:
            total += gap
            parsed += 1
    if parsed:
        return total
    air = sum(s.duration_min for s in offer.segments)
    inferred = offer.duration_min - air
    if inferred > 0:
        return inferred
    return 24 * 60


def _priced_in_currency(offers: list[Offer]) -> list[Offer]:
    priced = [o for o in offers if o.price is not None]
    if not priced:
        return []
    counts: dict[str, int] = {}
    for offer in priced:
        counts[offer.currency] = counts.get(offer.currency, 0) + 1
    preferred = max(counts, key=lambda c: counts[c])
    return [o for o in priced if o.currency == preferred]


def _best_local_honest(offers: list[Offer]) -> Offer | None:
    priced = _priced_in_currency(offers)
    if not priced:
        return None
    return min(priced, key=lambda o: (o.price or 1e12, layover_minutes(o), o.duration_min))


def pick_honest(nonstop: list[Offer], connecting: list[Offer]) -> HonestPick | None:
    priced = _priced_in_currency([*nonstop, *connecting])
    if not priced:
        return None
    offer = min(priced, key=lambda o: (o.price or 1e12, layover_minutes(o), o.duration_min))
    if offer.stops == 0:
        return HonestPick(
            kind="nonstop",
            reason="Cheapest nonstop",
            offer=offer,
            layover_min=0,
        )
    return HonestPick(
        kind="connecting",
        reason="Cheapest connecting — shortest layover if tied",
        offer=offer,
        layover_min=layover_minutes(offer),
    )


def _hidden_if_cheaper(
    matches: list[HiddenCityMatch], honest: HonestPick | None
) -> HiddenCityMatch | None:
    if not matches:
        return None
    if honest is None or honest.offer.price is None:
        return matches[0]
    ceiling = honest.offer.price
    currency = honest.offer.currency
    for match in matches:
        price = match.through_offer.price
        if (
            price is not None
            and match.through_offer.currency == currency
            and price < ceiling
        ):
            return match
    return None


def _gaps(
    amadeus: AmadeusProvider | None,
    duffel: DuffelProvider | None,
    box: AeroDataBoxProvider | None,
    priced: list[Offer],
    hidden: list[HiddenCityMatch],
    traffic: LiveTraffic,
    board: list[BoardFlight] | object,
) -> list[str]:
    gaps: list[str] = []
    if not amadeus and not duffel:
        gaps.append(
            "No shop API keys. Priced offers and hidden-city inversions are empty until AMADEUS_CLIENT_ID/SECRET or DUFFEL_TOKEN are set. Booker deep-links still open live shops."
        )
    elif not priced:
        gaps.append("Shop APIs are configured but returned no priced offers for this city-pair/date (test inventory, date, or airline coverage).")
    if not hidden:
        gaps.append("No priced inversion P(A,B,C)<P(A,B) in the current shop. Connection hints are not savings.")
    if not traffic.aircraft:
        gaps.append(traffic.note)
    if not box:
        gaps.append("No RAPIDAPI_KEY: AeroDataBox FIDS (scheduled/estimated/actual board) is off. Use FR24/FlightAware links for commercial boards.")
    return gaps
