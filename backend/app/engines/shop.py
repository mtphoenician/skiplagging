from __future__ import annotations

import asyncio
import time
from collections.abc import Awaitable
from dataclasses import replace

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
    origin, dest = query.origin.upper(), query.destination.upper()
    query.origin, query.destination = origin, dest

    o_ap = await repo.get_airport(session, origin)
    d_ap = await repo.get_airport(session, dest)
    if not o_ap or not d_ap:
        raise ValueError(
            "Unknown IATA in the OurAirports table. Ingest has not been run, or the code is not a scheduled airport."
        )
    if origin == dest:
        raise ValueError("Origin and destination must differ.")

    amadeus = AmadeusProvider(settings, client) if settings.amadeus_enabled else None
    duffel = DuffelProvider(settings, client) if settings.duffel_enabled else None
    opensky = OpenSkyProvider(settings, client)
    box = AeroDataBoxProvider(settings, client) if settings.aerodatabox_enabled else None

    req = ShopRequest(
        origin=origin,
        dest=dest,
        date=query.date,
        adults=query.adults,
        cabin=query.cabin,
        currency=query.currency,
        max_offers=20,
    )

    local_t = _shop_providers(req, amadeus, duffel)
    nearby_t = _shop_nearby(req, query, session, amadeus, duffel) if query.include_nearby else _empty_list()
    hidden_t = _shop_hidden(req, query, settings, session, amadeus, duffel)
    hints_t = _connection_hints(session, origin, dest, amadeus)
    traffic_o_t = opensky.traffic_near(o_ap)
    traffic_d_t = opensky.traffic_near(d_ap)
    board_t = box.board(origin, "Departure") if box else _empty_list()

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
    cheapest_local = min((o.price for o in priced if o.price is not None), default=None)
    pool = priced + [o for o in nearby_offers if o.price is not None] + [
        m.through_offer for m in hidden_matches if m.through_offer.price is not None
    ]
    cheapest_any = min((o.price for o in pool if o.price is not None), default=None)

    carriers = list({o.carrier for o in priced if o.carrier})
    airline_names = await repo.airlines_by_iata(session, carriers[:6])
    bookers = booker_links(origin, dest, query.date, query.adults, airline_names)
    for match in hidden_matches:
        thru_al = await repo.airlines_by_iata(session, [match.through_offer.carrier])
        match.bookers = booker_links(origin, match.hidden_city, query.date, query.adults, thru_al)

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
            offers=sorted(connecting, key=lambda o: o.price or 0)[:10],
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
        origin=origin,
        destination=dest,
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
            origin,
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
) -> list[Offer]:
    near_o = await repo.nearby_airports(session, req.origin)
    near_d = await repo.nearby_airports(session, req.dest)
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
) -> list[HiddenCityMatch]:
    if not amadeus and not duffel:
        return []
    local_offers = await _shop_providers(replace(req, nonstop=False, max_offers=15), amadeus, duffel)
    local_priced = [o for o in local_offers if o.price is not None]
    local = min((o for o in local_priced if o.stops == 0), key=lambda o: o.price or 1e12, default=None)
    if local is None:
        local = min(local_priced, key=lambda o: o.price or 1e12, default=None)
    if local is None:
        return []

    candidates = await _c_candidates(session, req.origin, req.dest, settings.max_hidden_candidates, amadeus)
    sem = asyncio.Semaphore(settings.max_concurrency)

    async def probe(c: str) -> HiddenCityMatch | None:
        async with sem:
            throughs = await _shop_providers(
                replace(req, dest=c, nonstop=False, max_offers=10),
                amadeus,
                duffel,
            )
            via = [o for o in throughs if _via_b(o, req.origin, req.dest, c) and o.price is not None]
            if not via:
                return None
            through = min(via, key=lambda o: o.price or 1e12)
            through.kind = "hidden-city"
            if through.price is None or local.price is None or through.price >= local.price:
                return None
            ap = await repo.get_airport(session, c)
            name = f"{ap.city} ({c})" if ap else c
            origin_ap = await repo.get_airport(session, req.origin)
            return attach_risk(
                match_id=f"hc-{through.id}-{c}",
                hidden_city=c,
                hidden_name=name,
                local=local,
                through=through,
                true_dest=req.dest,
                origin_country=origin_ap.country if origin_ap else "",
                hidden_country=ap.country if ap else "",
            )

    parts = await asyncio.gather(*(probe(c) for c in candidates), return_exceptions=True)
    return [p for p in parts if isinstance(p, HiddenCityMatch)]


async def _c_candidates(
    session: AsyncSession,
    origin: str,
    dest: str,
    limit: int,
    amadeus: AmadeusProvider | None,
) -> list[str]:
    seen = {origin, dest}
    ranked: list[str] = []

    def push(codes: list[str]) -> None:
        for c in codes:
            c = c.upper()
            if len(c) == 3 and c not in seen:
                seen.add(c)
                ranked.append(c)

    if amadeus:
        try:
            push(await amadeus.direct_destinations(dest))
        except httpx.HTTPError:
            pass
    spokes = await repo.destinations_from(session, dest, limit=limit)
    push([d for d, _, _ in spokes if d != origin])
    return ranked[:limit]


async def _connection_hints(
    session: AsyncSession, origin: str, dest: str, amadeus: AmadeusProvider | None
) -> list[ConnectionHint]:
    hints: list[ConnectionHint] = []
    spokes = await repo.destinations_from(session, dest, limit=18)
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


def _via_b(offer: Offer, origin: str, dest_b: str, dest_c: str) -> bool:
    if len(offer.segments) < 2:
        return False
    return (
        offer.segments[0].origin == origin
        and offer.segments[0].dest == dest_b
        and offer.segments[-1].dest == dest_c
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
