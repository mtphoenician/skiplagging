from __future__ import annotations

import asyncio
import time
from collections.abc import Awaitable
from dataclasses import replace
from datetime import datetime

import httpx
from sqlalchemy.ext.asyncio import AsyncSession

from typing import Literal

from app.config import Settings
from app.db import repo
from app.engines.budget import start_ledger, timed_shop, current_ledger
from app.engines.candidates import (
    Candidate,
    hub_probabilities,
    rank_candidates,
    record_offer,
    select_candidates,
)
from app.engines.hidden import (
    detect_hidden_city,
    hidden_city_savings,
    is_standard_to,
    meaningful_saving,
    ticketed_destination,
)
from app.engines.index import classify_for_search, ticketed_dests_through
from app.engines.learn import build_batch
from app.engines.risk import attach_risk
from app.engines.self_transfer import (
    MAX_LEG_FLIGHTS,
    bridge_pairs,
    combine_at_hubs,
    next_day,
    pick_transfer_hubs,
)
from app.fx import offer_in_usd, offers_in_usd
from app.models import (
    BoardFlight,
    CandidateTrace,
    ChannelGroup,
    ConnectionHint,
    HiddenCityMatch,
    HonestPick,
    LiveTraffic,
    Offer,
    SearchDebug,
    SearchQuery,
    SearchResponse,
)
from app.providers.aerodatabox import AeroDataBoxProvider
from app.providers.amadeus import AmadeusProvider
from app.providers.base import ShopRequest
from app.providers.bookers import booker_links
from app.providers.duffel import DuffelProvider
from app.providers.mock import MockProvider
from app.providers.opensky import OpenSkyProvider, tracker_links
from app.providers.sandbox import TEST_CARRIERS

SIDE_TIMEOUT = 1.2


async def search_all_ways(
    query: SearchQuery,
    settings: Settings,
    client: httpx.AsyncClient,
    session: AsyncSession,
    *,
    mode: Literal["fast", "deep"] = "fast",
    exclude: list[str] | None = None,
) -> SearchResponse:
    """Two-speed search. `fast`: index + direct A→B + top candidates, return quickly.
    `deep`: reuse the index, shop the remaining ranked candidates, return the fuller set."""
    t0 = time.perf_counter()
    ledger = start_ledger(settings.search_cost_usd)
    origin, dest = query.origin, query.destination
    query = query.model_copy(update={"currency": "USD"})

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
    mock = MockProvider() if settings.mock_enabled else None
    opensky = OpenSkyProvider(settings, client)
    box = AeroDataBoxProvider(settings, client) if settings.aerodatabox_enabled else None

    o_tokens = shop_tokens(o_ap)
    d_tokens = shop_tokens(d_ap)
    o_members = list(o_ap.members or [o_ap.iata])
    d_members = list(d_ap.members or [d_ap.iata])
    d_airports = set(d_members)
    route_origins = {*(o_members), *(o_tokens)}
    route_dests = {*(d_members), *(d_tokens)}
    city_search = o_ap.type == "city" or d_ap.type == "city"
    req = ShopRequest(
        origin=o_tokens[0],
        dest=d_tokens[0],
        date=query.date,
        adults=query.adults,
        cabin=query.cabin,
        currency="USD",
        max_offers=20,
    )

    indexed: list[Offer] = []
    try:
        indexed = await repo.recall_offers(
            session,
            origins=route_origins,
            date=query.date,
            adults=query.adults,
            cabin=query.cabin,
            max_age_seconds=settings.offer_ttl_seconds,
        )
    except Exception:
        indexed = []
    indexed = [o for o in offers_in_usd(indexed) if not _is_self_transfer(o)]
    indexed_honest = [
        o
        for o in indexed
        if o.price is not None
        and is_standard_to(o, route_dests)
        and o.segments
        and o.segments[0].origin.upper() in {c.upper() for c in route_origins}
    ]
    spokes = await repo.destinations_from_many(session, list(d_airports)[:4], per_origin=18)
    hint_aps = await repo.airports_by_iata(session, [code for code, _, _ in spokes])
    hints = _hints_from_spokes(spokes, hint_aps, origin)

    shop_local: list[Offer] = []
    traffic_o = _traffic_placeholder(o_ap)
    traffic_d = _traffic_placeholder(d_ap)
    board: list = []
    if indexed_honest:
        shop_local = await _shop_mock_only(req, o_ap, d_ap, mock)
    else:
        gathered = await asyncio.gather(
            _shop_place(req, o_ap, d_ap, amadeus, duffel, mock),
            _capped(opensky.traffic_near(o_ap), SIDE_TIMEOUT),
            _capped(opensky.traffic_near(d_ap), SIDE_TIMEOUT),
            _capped(box.board((o_ap.members or [o_ap.iata])[0], "Departure"), SIDE_TIMEOUT)
            if box
            else _empty_list(),
            return_exceptions=True,
        )
        if isinstance(gathered[0], list):
            shop_local = gathered[0]
        if isinstance(gathered[1], LiveTraffic):
            traffic_o = gathered[1]
        if isinstance(gathered[2], LiveTraffic):
            traffic_d = gathered[2]
        if isinstance(gathered[3], list):
            board = gathered[3]
    indexed_useful = [
        o for o in indexed if classify_for_search(o, route_origins, route_dests)
    ]
    raw_local = offers_in_usd(_drop_sandbox(_dedupe([*indexed_useful, *shop_local])))
    local_offers = _prefer_real_carriers(
        _dedupe_itineraries(_keep_on_route(raw_local, route_origins, route_dests))
    )
    local_offers = [o for o in local_offers if not _is_self_transfer(o)]
    priced = _priced_in_currency(
        [o for o in local_offers if o.price is not None], "USD"
    )
    nearby_offers: list[Offer] = []
    if (
        query.include_nearby
        and not city_search
        and (amadeus or duffel)
        and len(priced) < 3
    ):
        nearby_offers = await _shop_nearby(
            req, query, session, amadeus, duffel, o_members, d_members, mock
        )
        nearby_offers = [
            o for o in _dedupe(nearby_offers) if not _on_route(o, route_origins, route_dests)
        ]
        nearby_offers = _drop_sandbox(_prefer_real_carriers(_dedupe_itineraries(nearby_offers)))
        nearby_offers = offers_in_usd(nearby_offers)
        if priced:
            nearby_offers = [o for o in nearby_offers if o.currency == priced[0].currency]
        if any((o.carrier or "").upper() not in TEST_CARRIERS for o in local_offers):
            nearby_offers = [
                o for o in nearby_offers if (o.carrier or "").upper() not in TEST_CARRIERS
            ]

    nonstop = [o for o in priced if o.stops == 0]
    connecting = [o for o in priced if o.stops > 0]
    honest_pick = pick_honest(nonstop, connecting, "USD")
    best_pick = pick_best(nonstop, connecting, honest_pick)

    rejected: list[str] = []
    hidden_from_local: list[HiddenCityMatch] = []
    if honest_pick:
        hidden_from_local, rejected = _classify_hidden(
            raw_local, route_dests, honest_pick.offer, o_ap, settings.min_hidden_saving
        )
    hidden_matches: list[HiddenCityMatch] = list(hidden_from_local)
    expanded: list[str] = []
    pending: list[str] = []
    probe_offers: list[Offer] = []
    trace: list[CandidateTrace] = []
    covered = ticketed_dests_through(raw_local, route_dests)
    skipped = sorted(covered)
    excluded = {c.upper() for c in (exclude or [])} | covered
    can_shop = bool(amadeus or duffel or mock)
    self_transfer_offers: list[Offer] = []
    transfer_legs: list[Offer] = []
    mock_covered = bool(mock and any(o.source == "mock" for o in priced))
    if (amadeus or duffel) and not mock_covered:
        try:
            self_transfer_offers, transfer_legs = await _shop_self_transfers(
                req,
                settings,
                session,
                amadeus,
                duffel,
                o_ap,
                d_ap,
                route_dests,
                mode,
            )
            self_transfer_offers = offers_in_usd(_drop_sandbox(self_transfer_offers))
            transfer_legs = offers_in_usd(_drop_sandbox(transfer_legs))
        except Exception:
            self_transfer_offers, transfer_legs = [], []
    if can_shop and honest_pick:
        budget = settings.fast_candidates if mode == "fast" else settings.max_hidden_candidates
        try:
            ranked = await _plan_candidates(
                session, settings, route_origins, route_dests, d_members, amadeus
            )
        except Exception:
            ranked = []
        if mode == "fast" and hidden_from_local:
            # A cheaper through-ticket is already on hand: answer now, let the deep
            # pass spend money looking for a better one.
            pass
        else:
            chosen = select_candidates(
                ranked,
                budget,
                settings.candidate_min_score,
                cold_start=settings.fast_candidates,
                exclude=excluded,
            )
            expanded = [c.code for c in chosen]
        if mode == "fast":
            # Exactly what the deep pass will pay for, so the UI can say "N more".
            preview = select_candidates(
                ranked,
                settings.max_hidden_candidates,
                settings.candidate_min_score,
                cold_start=settings.fast_candidates,
                exclude=excluded | set(expanded),
            )
            pending = [c.code for c in preview]
            chosen_codes = set(expanded)
            for c in ranked:
                c.selected = c.code in chosen_codes
        if expanded:
            try:
                extra, probe_offers = await _probe_candidates(
                    req,
                    settings,
                    amadeus,
                    duffel,
                    mock,
                    o_ap,
                    route_dests,
                    honest_pick.offer,
                    expanded,
                )
                hidden_matches.extend(extra)
            except Exception:
                probe_offers = []
        trace = [
            CandidateTrace(
                code=c.code,
                score=c.score,
                source=c.source,
                observations=c.observations,
                successful_connections=c.successful_connections,
                cheaper_than_direct_count=c.cheaper_than_direct_count,
                median_saving=c.median_saving,
                parts=c.parts,
                selected=c.selected,
            )
            for c in ranked[:20]
        ]
    hidden_matches = _clean_hidden_matches(
        [m for m in hidden_matches if not _is_sandbox(m.through_offer)]
    )
    hidden_matches.sort(key=lambda m: (-(m.gross_saving or 0), -m.risk.net_saving_estimate))
    # One learning row per distinct offer: a through-ticket sits in both raw_local and
    # hidden_matches, and must not be counted twice. Never learn a stitched self-transfer
    # as if it were one PNR; component tickets are real and may be remembered.
    learned_offers = _dedupe(
        _drop_sandbox(
            [
                *raw_local,
                *probe_offers,
                *nearby_offers,
                *transfer_legs,
                *[m.through_offer for m in hidden_matches],
            ]
        )
    )
    learned_offers = [o for o in learned_offers if not _is_self_transfer(o)]
    try:
        await repo.remember_offers(
            session,
            learned_offers,
            date=query.date,
            adults=query.adults,
            cabin=query.cabin,
        )
    except Exception:
        pass
    try:
        batch = build_batch(
            learned_offers,
            origins=route_origins,
            intended=route_dests,
            honest_price=honest_pick.offer.price if honest_pick else None,
            honest_currency=honest_pick.offer.currency if honest_pick else None,
            probed=expanded,
            date=query.date,
            adults=query.adults,
            cabin=query.cabin,
            probe_origin=req.origin,
            probe_intended=req.dest,
        )
        await repo.apply_learning(session, batch)
    except Exception:
        await session.rollback()
    try:
        await repo.persist_provider_calls(session, ledger.calls)
    except Exception:
        await session.rollback()
    hidden_if_cheaper = _hidden_if_cheaper(hidden_matches, honest_pick)
    search_debug = SearchDebug(
        providers=[p for p, on in (("mock", mock), ("duffel", duffel), ("amadeus", amadeus)) if on],
        standard_query=f"{req.origin}→{req.dest}",
        mode=mode,
        expanded_destinations=expanded,
        pending_candidates=pending,
        skipped_expansion=skipped,
        candidates=trace,
        provider_calls=len(ledger.paid()),
        provider_cost_usd=ledger.total_cost,
        reused_from_index=len(indexed),
        rejected=rejected[:24],
        cache=_cache_state(indexed, shop_local),
        pending_self_transfer=bool((amadeus or duffel) and not mock_covered and mode == "fast"),
    )
    compare_ccy = honest_pick.offer.currency if honest_pick else None
    local_cmp = [o for o in priced if compare_ccy is None or o.currency == compare_ccy]
    cheapest_local = min((o.price for o in local_cmp if o.price is not None), default=None)
    pool = local_cmp + [
        o for o in nearby_offers if o.price is not None and (compare_ccy is None or o.currency == compare_ccy)
    ] + [
        m.through_offer
        for m in hidden_matches
        if m.through_offer.price is not None and (compare_ccy is None or m.through_offer.currency == compare_ccy)
    ] + [
        o for o in self_transfer_offers if o.price is not None and (compare_ccy is None or o.currency == compare_ccy)
    ]
    cheapest_any = min((o.price for o in pool if o.price is not None), default=None)

    name_codes = {
        *(o.carrier for o in priced if o.carrier),
        *(o.validating_airline for o in priced if o.validating_airline),
        *(s.carrier for o in priced for s in o.segments if s.carrier),
        *(m.through_offer.carrier for m in hidden_matches if m.through_offer.carrier),
        *(o.carrier for o in nearby_offers if o.carrier),
        *(o.carrier for o in self_transfer_offers if o.carrier),
        *(s.carrier for o in self_transfer_offers for s in o.segments if s.carrier),
    }
    airline_pairs = await repo.airlines_by_iata(session, list(name_codes))
    airline_names = {code: name for code, name in airline_pairs}
    book_ccy = "USD"
    bookers = booker_links(
        o_ap.iata,
        d_ap.iata,
        query.date,
        query.adults,
        airline_pairs[:6],
        currency=book_ccy,
        cabin=query.cabin,
    )
    ticketed_aps = await repo.airports_by_iata(
        session,
        [m.ticketed_destination or m.hidden_city for m in hidden_matches],
    )
    airline_map = {code: name for code, name in airline_pairs}
    for match in hidden_matches:
        code = (match.through_offer.carrier or "").upper()
        thru_al = [(code, airline_map[code])] if code in airline_map else []
        match.bookers = booker_links(
            o_ap.iata,
            match.hidden_city,
            query.date,
            query.adults,
            thru_al,
            currency=book_ccy,
            cabin=query.cabin,
        )
        ticketed = match.ticketed_destination or match.hidden_city
        ticketed_ap = ticketed_aps.get(ticketed)
        if ticketed_ap:
            match.hidden_city_name = f"{ticketed_ap.city} ({ticketed})"
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
            offers=sorted(nonstop, key=_honest_rank),
        ),
        ChannelGroup(
            kind="connecting",
            label="Priced connecting itineraries that end at B",
            blurb="Honest through products ticketed to B. Different from hidden-city A→B→C.",
            offers=sorted(connecting, key=_honest_rank),
        ),
        ChannelGroup(
            kind="nearby",
            label="Priced nearby-airport substitutes",
            blurb="Same trip intent, different IATA. Still a real A′→B′ offer, not skiplagging.",
            offers=sorted([o for o in nearby_offers if o.price is not None], key=_honest_rank),
        ),
        ChannelGroup(
            kind="self-transfer",
            label="Self-transfer (separate tickets)",
            blurb="Two or three independently priced tickets you buy yourself. A missed connection is not protected. Not hidden-city.",
            offers=self_transfer_offers,
        ),
        ChannelGroup(
            kind="hidden-city",
            label="Priced hidden-city inversions",
            blurb="Complete tickets that continue past the intended city. The priced itinerary is never rewritten.",
            offers=[m.through_offer for m in hidden_matches],
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
    if mock:
        sources_used.append("mock")
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
    gaps = _gaps(amadeus, duffel, box, priced, hidden_matches, traffic_o, board, mock)
    notes = [
        "Reference / track / schedule / priced-offer / booker are different layers. A tracker is not a ticket. A metasearch link is not a PNR.",
        "Ranking: this is a price comparator first. Cheapest regular ticket, then the best (fewest stops) and fastest. Self-transfer and hidden-city rows are additions shown only when they cost less than the cheapest regular ticket.",
        "Hidden-city rows are complete tickets that continue past the intended city. The priced itinerary is never rewritten into a fake A→B fare.",
        "Every priced row is converted to USD before ranking, so GBP and other airline quotes never mix with dollar fares.",
        "Self-transfer rows are separate tickets stitched at a hub, at most five flights. The sum is not one PNR and is not hidden-city.",
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
        best_pick=best_pick,
        hidden_if_cheaper=hidden_if_cheaper,
        channels=channels,
        hidden_city=hidden_matches,
        connection_hints=hints,
        bookers=bookers,
        airline_names=airline_names,
        traffic_origin=traffic_o,
        traffic_destination=traffic_d,
        board_origin=board if isinstance(board, list) else [],
        notes=notes,
        search_debug=search_debug,
    )


async def _empty_list() -> list:
    return []


async def _capped(coro, timeout: float = SIDE_TIMEOUT):
    try:
        return await asyncio.wait_for(coro, timeout)
    except Exception:
        return None


def _cache_state(indexed: list, shopped: list) -> str:
    if indexed and not shopped:
        return "index"
    if indexed:
        return "index+live"
    return "miss"


def _needs_city_nonstop(origin_place, dest_place, pair_index: int) -> bool:
    return pair_index == 0 and (
        getattr(origin_place, "type", "") == "city" or getattr(dest_place, "type", "") == "city"
    )


def _traffic_placeholder(place) -> LiveTraffic:
    members = getattr(place, "members", None) or [place.iata]
    iata = members[0]
    icao = getattr(place, "icao", None) or iata
    return LiveTraffic(
        airport=place.iata,
        source="opensky",
        api_time=None,
        note="Search does not wait on OpenSky. Use the tracker links for live aircraft.",
        aircraft=[],
        trackers=tracker_links(iata, icao),
    )


def _hints_from_spokes(
    spokes: list[tuple[str, str, int]],
    airports: dict,
    origin: str,
) -> list[ConnectionHint]:
    hints: list[ConnectionHint] = []
    seen: set[str] = set()
    origin_u = origin.upper()[:3]
    for code, airline, n in spokes:
        if code == origin_u or code in seen:
            continue
        seen.add(code)
        ap = airports.get(code)
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
        if len(hints) >= 24:
            break
    return hints


def shop_tokens(place) -> list[str]:
    members = [m for m in (place.members or []) if m]
    if getattr(place, "type", "") == "city":
        tokens: list[str] = []
        if place.iata:
            tokens.append(place.iata)
        for code in members:
            if code not in tokens:
                tokens.append(code)
        return tokens[:4] or [place.iata]
    return [place.iata]


def _route_pairs(origins: list[str], dests: list[str], limit: int = 8) -> list[tuple[str, str]]:
    pairs: list[tuple[str, str]] = []
    seen: set[tuple[str, str]] = set()

    def add(origin: str, dest: str) -> None:
        key = (origin.upper(), dest.upper())
        if origin and dest and key[0] != key[1] and key not in seen:
            seen.add(key)
            pairs.append((origin, dest))

    if origins and dests:
        add(origins[0], dests[0])
    for origin in origins[1:] or origins[:1]:
        for dest in dests[1:] or dests[:1]:
            add(origin, dest)
    return pairs[:limit]


def _same_place(a, b) -> bool:
    if (a.place_id or a.iata) == (b.place_id or b.iata):
        return True
    am, bm = set(a.members or [a.iata]), set(b.members or [b.iata])
    if a.type == "city" and b.iata in am:
        return True
    if b.type == "city" and a.iata in bm:
        return True
    return False


async def _shop_mock_only(
    req: ShopRequest,
    origin: object,
    dest: object,
    mock: MockProvider | None,
) -> list[Offer]:
    if not mock:
        return []
    pairs = _route_pairs(shop_tokens(origin), shop_tokens(dest))
    if not pairs:
        return []
    parts = await asyncio.gather(
        *(mock.shop(replace(req, origin=o, dest=d)) for o, d in pairs),
        return_exceptions=True,
    )
    out: list[Offer] = []
    for part in parts:
        if isinstance(part, list):
            out.extend(part)
    return out


async def _shop_place(
    req: ShopRequest,
    origin: object,
    dest: object,
    amadeus: AmadeusProvider | None,
    duffel: DuffelProvider | None,
    mock: MockProvider | None = None,
) -> list[Offer]:
    pairs = _route_pairs(shop_tokens(origin), shop_tokens(dest))
    if not pairs:
        return []

    # Every pair allows connections. Metro codes (PAR→LON) often miss
    # inventory that only appears on CDG→LHR / ORY→LGW, including 1-stops.
    sem = asyncio.Semaphore(3)

    async def one(idx: int, o: str, d: str) -> list[Offer]:
        async with sem:
            shop = replace(req, origin=o, dest=d)
            return await _shop_providers(
                shop,
                amadeus,
                duffel,
                extra_nonstop=_needs_city_nonstop(origin, dest, idx),
                mock=mock,
            )

    parts = await asyncio.gather(
        *(one(i, o, d) for i, (o, d) in enumerate(pairs)), return_exceptions=True
    )
    out: list[Offer] = []
    for part in parts:
        if isinstance(part, list):
            out.extend(part)
    return out


async def _shop_pairs(
    req: ShopRequest,
    origins: list[str],
    dests: list[str],
    amadeus: AmadeusProvider | None,
    duffel: DuffelProvider | None,
) -> list[Offer]:
    pairs = _route_pairs(origins, dests)
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
    req: ShopRequest,
    amadeus: AmadeusProvider | None,
    duffel: DuffelProvider | None,
    extra_nonstop: bool = False,
    mock: MockProvider | None = None,
    purpose: str = "direct",
) -> list[Offer]:
    tasks: list[Awaitable[list[Offer]]] = []
    # Partner adapters only. No airline-website collection.
    if mock:
        mocked = await timed_shop("mock", req.origin, req.dest, req.date, purpose, mock.shop(req))
        if mocked:
            return offers_in_usd(_drop_sandbox(mocked))

    def paid(provider: str, r: ShopRequest, coro: Awaitable[list[Offer]]) -> Awaitable[list[Offer]]:
        return timed_shop(provider, r.origin, r.dest, r.date, purpose, coro)

    if amadeus:
        tasks.append(paid("amadeus", req, amadeus.shop(req)))
        if extra_nonstop and not req.nonstop:
            r = replace(req, nonstop=True, max_offers=8)
            tasks.append(paid("amadeus", r, amadeus.shop(r)))
    if duffel:
        tasks.append(paid("duffel", req, duffel.shop(req)))
        if extra_nonstop and not req.nonstop:
            r = replace(req, nonstop=True, max_offers=12)
            tasks.append(paid("duffel", r, duffel.shop(r)))
    if not tasks:
        return []
    parts = await asyncio.gather(*tasks, return_exceptions=True)
    out: list[Offer] = []
    for part in parts:
        if isinstance(part, list):
            out.extend(part)
    return offers_in_usd(_drop_sandbox(out))


async def _shop_nearby(
    req: ShopRequest,
    query: SearchQuery,
    session: AsyncSession,
    amadeus: AmadeusProvider | None,
    duffel: DuffelProvider | None,
    origin_members: list[str] | None = None,
    dest_members: list[str] | None = None,
    mock: MockProvider | None = None,
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
    pairs = pairs[:2]
    if not pairs or (not amadeus and not duffel):
        return []

    async def one(o: str, d: str) -> list[Offer]:
        r = replace(req, origin=o, dest=d, nonstop=True, max_offers=5)
        offers = await _shop_providers(r, amadeus, duffel, mock=mock, purpose="nearby")
        for offer in offers:
            offer.kind = "nearby"
        return offers

    parts = await asyncio.gather(*(one(o, d) for o, d in pairs), return_exceptions=True)
    out: list[Offer] = []
    for part in parts:
        if isinstance(part, list):
            out.extend(part)
    return out


async def _shop_self_transfers(
    req: ShopRequest,
    settings: Settings,
    session: AsyncSession,
    amadeus: AmadeusProvider | None,
    duffel: DuffelProvider | None,
    origin_place,
    dest_place,
    intended: set[str],
    mode: Literal["fast", "deep"],
) -> tuple[list[Offer], list[Offer]]:
    """Shop A→H and H→B, then stitch unprotected self-transfers. Not hidden-city."""
    origin = (origin_place.members or [origin_place.iata])[0]
    dest = (dest_place.members or [dest_place.iata])[0]
    from_origin = {
        code
        for code, _, _ in await repo.destinations_from_many(
            session, list(origin_place.members or [origin])[:3], per_origin=24
        )
    }
    into: set[str] = set()
    for code in list(dest_place.members or [dest])[:3]:
        into.update(await repo.origins_into(session, code, limit=24))
    extra = await repo.busiest_airports(
        session, limit=20, exclude={origin, dest, *list(origin_place.members or []), *list(dest_place.members or [])}
    )
    continents = await repo.continents_for(session, [origin, dest, *from_origin, *into, *extra])
    n_hubs = 2 if mode == "fast" else 3
    ledger = current_ledger()
    paid = len(ledger.paid()) if ledger else 0
    left = max(0, settings.max_provider_calls - paid)
    per_hub = 2 if mode == "fast" else 3
    n_hubs = min(n_hubs, left // per_hub) if per_hub else 0
    if n_hubs < 1:
        return [], []
    hubs = pick_transfer_hubs(
        origin,
        dest,
        from_origin,
        into,
        limit=n_hubs,
        extra_hubs=extra,
        continents=continents,
    )
    if not hubs:
        return [], []

    async def leg(o: str, d: str, date: str) -> list[Offer]:
        r = replace(req, origin=o, dest=d, date=date, nonstop=False, max_offers=8)
        return await _shop_providers(r, amadeus, duffel, mock=None, purpose="interline")

    def keep_legs(offers: list[Offer], dest_ok) -> list[Offer]:
        kept = _prefer_real_carriers(
            [
                o
                for o in offers
                if dest_ok(o)
                and detect_hidden_city(o, intended) is None
                and 0 < len(o.segments) <= MAX_LEG_FLIGHTS
            ]
        )
        kept.sort(key=lambda o: (o.stops, o.price or 1e12, o.duration_min))
        return kept[:4]

    async def inbound_hub(hub: str) -> tuple[str, list[Offer]]:
        offers = await leg(req.origin, hub, req.date)
        return hub, keep_legs(offers, lambda o: is_standard_to(o, hub))

    async def outbound_hub(hub: str) -> tuple[str, list[Offer]]:
        dates = [req.date] if mode == "fast" else [req.date, next_day(req.date)]
        parts = await asyncio.gather(*(leg(hub, req.dest, d) for d in dates), return_exceptions=True)
        pooled: list[Offer] = []
        for part in parts:
            if isinstance(part, list):
                pooled.extend(part)
        return hub, keep_legs(pooled, lambda o: is_standard_to(o, intended))

    inbound_rows = await asyncio.gather(*(inbound_hub(h) for h in hubs), return_exceptions=True)
    outbound_rows = await asyncio.gather(*(outbound_hub(h) for h in hubs), return_exceptions=True)
    inbound: dict[str, list[Offer]] = {}
    outbound: dict[str, list[Offer]] = {}
    for row in inbound_rows:
        if isinstance(row, tuple):
            inbound[row[0]] = row[1]
    for row in outbound_rows:
        if isinstance(row, tuple):
            outbound[row[0]] = row[1]

    mids: dict[tuple[str, str], list[Offer]] = {}
    if mode == "deep":
        for h1, h2 in bridge_pairs(hubs, from_origin=from_origin, into_dest=into, limit=2):
            mid = await leg(h1, h2, req.date)
            kept_mid = keep_legs(mid, lambda o, dest_h=h2: is_standard_to(o, dest_h))
            if kept_mid:
                mids[(h1, h2)] = kept_mid
    legs: list[Offer] = []
    for group in (*inbound.values(), *outbound.values(), *mids.values()):
        legs.extend(group)
    return combine_at_hubs(inbound, outbound, intended, mids=mids, limit=8), _dedupe(legs)


async def _plan_candidates(
    session: AsyncSession,
    settings: Settings,
    route_origins: set[str],
    route_dests: set[str],
    dest_members: list[str],
    amadeus: AmadeusProvider | None,
) -> list[Candidate]:
    """CandidateGenerator: pool every plausible C, score it, rank it. No paid calls here."""
    origin = sorted(route_origins)[0] if route_origins else ""
    stats = await repo.load_route_stats(session, route_origins, route_dests)
    pool: dict[str, str] = {}
    for c in await repo.recall_candidate_dests(
        session,
        origins=route_origins,
        intended=route_dests,
        date="",  # any date: a C seen through B on another day is still a good guess
        adults=0,
        cabin="",
        max_age_seconds=30 * 24 * 3600,
        limit=settings.max_hidden_candidates * 3,
        any_date=True,
    ):
        pool.setdefault(c, "index")
    if amadeus:
        parts = await asyncio.gather(
            *(amadeus.direct_destinations(d) for d in dest_members[:3]), return_exceptions=True
        )
        for part in parts:
            if isinstance(part, list):
                for c in part:
                    pool.setdefault(c.upper(), "amadeus")
    route_map = await repo.route_map_from(session, dest_members[:4])
    for b, spokes in route_map.items():
        for c in sorted(spokes):
            pool.setdefault(c, "openflights")
    hub_prob = hub_probabilities(route_dests, route_map)
    provider_rate = await repo.provider_rates_for(session, route_origins, [*pool, *stats])
    return rank_candidates(origin, route_dests, stats, pool, hub_prob, provider_rate)


async def _probe_candidates(
    req: ShopRequest,
    settings: Settings,
    amadeus: AmadeusProvider | None,
    duffel: DuffelProvider | None,
    mock: MockProvider | None,
    origin_place,
    intended: set[str],
    local: Offer,
    candidates: list[str],
) -> tuple[list[HiddenCityMatch], list[Offer]]:
    """Pay for A→C on the chosen candidates and classify what comes back against A→B.
    Returns (matches, every complete offer seen) so the learner can record misses too."""
    if not candidates or not (amadeus or duffel or mock):
        return [], []
    sem = asyncio.Semaphore(settings.max_concurrency)

    async def probe(c: str) -> tuple[list[HiddenCityMatch], list[Offer]]:
        async with sem:
            throughs = await _shop_providers(
                replace(req, dest=c, nonstop=False, max_offers=20),
                amadeus,
                duffel,
                mock=mock,
                purpose="expand",
            )
            matches, _ = _classify_hidden(
                throughs, intended, local, origin_place, settings.min_hidden_saving
            )
            return matches, throughs

    parts = await asyncio.gather(*(probe(c) for c in candidates), return_exceptions=True)
    matches: list[HiddenCityMatch] = []
    seen: list[Offer] = []
    for part in parts:
        if isinstance(part, tuple):
            matches.extend(part[0])
            seen.extend(part[1])
    return matches, seen


def _classify_hidden(
    offers: list[Offer],
    intended: set[str],
    local: Offer,
    origin_place,
    min_saving: float,
) -> tuple[list[HiddenCityMatch], list[str]]:
    local_usd = offer_in_usd(local)
    if local_usd is None or local_usd.price is None:
        return [], ["honest fare could not be converted to USD"]
    matches: list[HiddenCityMatch] = []
    rejected: list[str] = []
    for offer in offers:
        if offer.price is None:
            continue
        converted = offer_in_usd(offer)
        if converted is None or converted.price is None:
            rejected.append(f"{offer.id}: could not convert to USD")
            continue
        if is_standard_to(converted, intended):
            continue
        hit = detect_hidden_city(converted, intended)
        if hit is None:
            rejected.append(f"{offer.id}: does not pass through intended destination")
            continue
        saving = hidden_city_savings(local_usd.price, converted.price)
        if not meaningful_saving(saving, min_saving):
            rejected.append(f"{offer.id}: saving below floor")
            continue
        converted.kind = "hidden-city"
        ticketed = ticketed_destination(converted)
        matches.append(
            attach_risk(
                match_id=f"hc-{converted.id}-{hit.exit_airport}",
                hidden_city=ticketed,
                hidden_name=ticketed,
                local=local_usd,
                through=converted,
                true_dest=hit.exit_airport,
                origin_country=getattr(origin_place, "country", "") or "",
                hidden_country="",
                exit_segment_index=hit.exit_segment_index,
            )
        )
        record_offer(converted.segments[0].origin, hit.exit_airport, ticketed, converted.price)
    return matches, rejected


def _via_b(offer: Offer, origin: str | set[str], dest_b: str | set[str], dest_c: str) -> bool:
    """True when a complete A→…→C ticket exits at B. B may be any intermediate stop."""
    if not offer.segments:
        return False
    origins = {o.upper() for o in ({origin} if isinstance(origin, str) else origin)}
    dests = {d.upper() for d in ({dest_b} if isinstance(dest_b, str) else dest_b)}
    if offer.segments[0].origin.upper() not in origins:
        return False
    if ticketed_destination(offer) != dest_c.upper() or dest_c.upper() in dests:
        return False
    return detect_hidden_city(offer, dests) is not None


def _is_sandbox(offer: Offer) -> bool:
    if offer.live is False:
        return True
    note = offer.note or ""
    return offer.source == "duffel" and "live_mode=False" in note


def _is_self_transfer(offer: Offer) -> bool:
    return offer.kind == "self-transfer" or offer.source == "self-transfer"


def _drop_sandbox(offers: list[Offer]) -> list[Offer]:
    return [o for o in offers if not _is_sandbox(o)]


def _on_route(offer: Offer, origins: set[str], dests: set[str]) -> bool:
    if not offer.segments:
        return False
    allowed_o = {c.upper() for c in origins}
    allowed_d = {c.upper() for c in dests}
    return offer.segments[0].origin.upper() in allowed_o and offer.segments[-1].dest.upper() in allowed_d


def _keep_on_route(offers: list[Offer], origins: set[str], dests: set[str]) -> list[Offer]:
    return [o for o in offers if _on_route(o, origins, dests)]


def _codeshare_key(offer: Offer) -> tuple:
    return tuple(
        (s.origin.upper(), s.dest.upper(), (s.dep or "")[:16], (s.arr or "")[:16]) for s in offer.segments
    )


def _dedupe_itineraries(offers: list[Offer]) -> list[Offer]:
    best: dict[tuple, Offer] = {}
    for offer in sorted(offers, key=lambda o: (o.price or 1e12, layover_minutes(o), o.duration_min)):
        key = _codeshare_key(offer)
        if key not in best:
            best[key] = offer
    return list(best.values())


def _prefer_real_carriers(offers: list[Offer]) -> list[Offer]:
    real = [o for o in offers if (o.carrier or "").upper() not in TEST_CARRIERS]
    return real or offers


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


def _priced_in_currency(offers: list[Offer], preferred: str | None = None) -> list[Offer]:
    priced = [o for o in offers_in_usd(offers) if o.price is not None]
    if not priced:
        return []
    want = (preferred or "USD").upper()
    matched = [o for o in priced if (o.currency or "").upper() == want]
    if matched:
        return matched
    counts: dict[str, int] = {}
    for offer in priced:
        counts[offer.currency] = counts.get(offer.currency, 0) + 1
    modal = max(counts, key=lambda c: counts[c])
    return [o for o in priced if o.currency == modal]


def _best_local_honest(offers: list[Offer], preferred: str | None = None) -> Offer | None:
    priced = _priced_in_currency(offers, preferred)
    if not priced:
        return None
    return min(priced, key=lambda o: (o.price or 1e12, layover_minutes(o), o.duration_min))


def _as_pick(offer: Offer, reason: str) -> HonestPick:
    return HonestPick(
        kind="nonstop" if offer.stops == 0 else "connecting",
        reason=reason,
        offer=offer,
        layover_min=layover_minutes(offer),
    )


def pick_honest(
    nonstop: list[Offer], connecting: list[Offer], preferred: str | None = None
) -> HonestPick | None:
    priced = _priced_in_currency([*nonstop, *connecting], preferred)
    if not priced:
        return None
    offer = min(priced, key=lambda o: (o.price or 1e12, layover_minutes(o), o.duration_min))
    if offer.stops == 0:
        return _as_pick(offer, "Best — cheapest nonstop")
    return _as_pick(offer, "Cheapest — connecting")


def _honest_rank(offer: Offer) -> tuple:
    return (offer.stops, offer.price or 1e12, layover_minutes(offer), offer.duration_min)


def pick_best(
    nonstop: list[Offer],
    connecting: list[Offer],
    cheapest: HonestPick | None,
) -> HonestPick | None:
    """Best honest ticket: cheapest nonstop, else cheapest trip with the fewest stops."""
    priced = _priced_in_currency([*nonstop, *connecting])
    if not priced:
        return None
    cheap_id = cheapest.offer.id if cheapest else None
    nons = [o for o in priced if o.stops == 0]
    if nons:
        best = min(nons, key=_honest_rank)
        if best.id == cheap_id:
            return None
        return _as_pick(best, "Best — cheapest nonstop")
    min_stops = min(o.stops for o in priced)
    fewest = [o for o in priced if o.stops == min_stops]
    best = min(fewest, key=_honest_rank)
    if best.id == cheap_id:
        return None
    label = "one stop" if min_stops == 1 else f"{min_stops} stops"
    return _as_pick(best, f"Best — cheapest {label}")


def _clean_hidden_matches(matches: list[HiddenCityMatch]) -> list[HiddenCityMatch]:
    if not matches:
        return []
    throughs = [m.through_offer for m in matches]
    keep_ids = {o.id for o in _prefer_real_carriers(throughs)}
    cleaned = [m for m in matches if m.through_offer.id in keep_ids]
    best: dict[tuple, HiddenCityMatch] = {}
    for match in sorted(cleaned, key=lambda m: (m.through_offer.price or 1e12, -(m.gross_saving or 0))):
        key = _codeshare_key(match.through_offer)
        if key not in best:
            best[key] = match
    return list(best.values())


def _hidden_if_cheaper(
    matches: list[HiddenCityMatch], honest: HonestPick | None
) -> HiddenCityMatch | None:
    if not matches or honest is None or honest.offer.price is None:
        return None
    ceiling_offer = offer_in_usd(honest.offer)
    if ceiling_offer is None or ceiling_offer.price is None:
        return None
    ceiling = ceiling_offer.price
    for match in matches:
        through = offer_in_usd(match.through_offer)
        if through is None or through.price is None:
            continue
        if through.price < ceiling:
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
    mock: MockProvider | None = None,
) -> list[str]:
    gaps: list[str] = []
    if not amadeus and not duffel and not mock:
        gaps.append(
            "No shop API keys and mock is off. Priced offers stay empty until AMADEUS_CLIENT_ID/SECRET, DUFFEL_TOKEN, or MOCK_ENABLED."
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
