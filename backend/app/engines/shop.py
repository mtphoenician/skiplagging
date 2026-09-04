from __future__ import annotations

import asyncio
import time
from collections.abc import Awaitable
from dataclasses import replace

import httpx

from app.config import Settings
from app.engines.risk import attach_risk
from app.graph.airports import AIRPORTS, airport, nearby
from app.graph.hubs import hidden_city_candidates
from app.models import ChannelGroup, HiddenCityMatch, Offer, SearchQuery, SearchResponse
from app.providers.amadeus import AmadeusProvider
from app.providers.base import ShopRequest
from app.providers.demo import DemoProvider, _force_inversion

_demo = DemoProvider()


async def search_all_ways(
    query: SearchQuery,
    settings: Settings,
    client: httpx.AsyncClient,
) -> SearchResponse:
    t0 = time.perf_counter()
    origin, dest = query.origin.upper(), query.destination.upper()
    query.origin, query.destination = origin, dest

    o_ap, d_ap = airport(origin), airport(dest)
    if not o_ap or not d_ap:
        known = ", ".join(sorted(AIRPORTS)[:12])
        raise ValueError(f"Unknown airport. Try IATA codes such as {known}…")
    if origin == dest:
        raise ValueError("Origin and destination must differ.")

    live = query.live and settings.amadeus_enabled
    amadeus = AmadeusProvider(settings, client) if live else None
    sources = ["demo"] + (["amadeus"] if amadeus else [])

    req = ShopRequest(
        origin=origin,
        dest=dest,
        date=query.date,
        adults=query.adults,
        cabin=query.cabin,
        currency=query.currency,
        max_offers=15,
    )

    local_task = _shop_local(req, amadeus)
    nearby_task = _shop_nearby(req, query, amadeus) if query.include_nearby else _empty()
    hidden_task = _shop_hidden(req, query, settings, amadeus)

    local_offers, nearby_offers, hidden_matches = await asyncio.gather(
        local_task, nearby_task, hidden_task
    )

    local_offers = _dedupe(local_offers)
    nearby_offers = _dedupe(nearby_offers)
    hidden_matches.sort(key=lambda m: (-m.gross_saving, -m.risk.net_saving_estimate))

    nonstop = [o for o in local_offers if o.kind == "nonstop" or o.stops == 0]
    connecting = [o for o in local_offers if o.stops > 0]
    cheapest_local = min((o.price for o in local_offers), default=None)
    pool = local_offers + nearby_offers + [m.through_offer for m in hidden_matches]
    cheapest_any = min((o.price for o in pool), default=None)

    channels = [
        ChannelGroup(
            kind="nonstop",
            label="Nonstop A → B",
            blurb="The local origin-destination product. This is what you actually want to consume.",
            offers=sorted(nonstop, key=lambda o: o.price)[:8],
        ),
        ChannelGroup(
            kind="connecting",
            label="Connecting to B",
            blurb="A genuine itinerary that ends at your destination. Often the cheapest honest ticket.",
            offers=sorted(connecting, key=lambda o: o.price)[:8],
        ),
        ChannelGroup(
            kind="nearby",
            label="Nearby airports",
            blurb="Same metro, different runway. Another legal way to buy the trip you intend to take.",
            offers=sorted(nearby_offers, key=lambda o: o.price)[:8],
        ),
        ChannelGroup(
            kind="hidden-city",
            label="Hidden-city A → B → C",
            blurb="Through fares cheaper than local A–B, using the same first sector. Ticketed destination is C.",
            offers=[m.through_offer for m in hidden_matches[:8]],
        ),
    ]

    notes = [
        "Hidden-city results are fare discoveries, not booking advice. Carriers prohibit point-beyond ticketing.",
        "A ticket is an end-to-end O-D product with sequential coupons — not two disposable flights.",
        "Net saving subtracts expected disruption and enforcement costs from the displayed fare gap.",
    ]
    if not live:
        notes.append(
            "Demo engine is on. Prices illustrate O-D inversions (including the published ORD–DCA–BOS and JFK–SFO–SEA cases). Add Amadeus keys and toggle live search for GDS offers."
        )
    elif not settings.amadeus_enabled:
        notes.append("Live search requested but AMADEUS_CLIENT_ID / SECRET are missing.")

    return SearchResponse(
        query=query,
        origin=o_ap,
        destination=d_ap,
        elapsed_ms=round((time.perf_counter() - t0) * 1000, 1),
        sources_used=sources,
        cheapest_local=cheapest_local,
        cheapest_any=cheapest_any,
        channels=channels,
        hidden_city=hidden_matches[:20],
        notes=notes,
    )


async def _empty() -> list:
    return []


async def _shop_local(req: ShopRequest, amadeus: AmadeusProvider | None) -> list[Offer]:
    tasks: list[Awaitable[list[Offer]]] = [_demo.shop(req)]
    if amadeus:
        tasks.append(amadeus.shop(req))
        tasks.append(amadeus.shop(replace(req, nonstop=True, max_offers=8)))
    parts = await asyncio.gather(*tasks, return_exceptions=True)
    out: list[Offer] = []
    for part in parts:
        if isinstance(part, list):
            out.extend(part)
    return out


async def _shop_nearby(
    req: ShopRequest, query: SearchQuery, amadeus: AmadeusProvider | None
) -> list[Offer]:
    pairs: list[tuple[str, str]] = []
    for o in [req.origin, *nearby(req.origin)[:2]]:
        for d in [req.dest, *nearby(req.dest)[:2]]:
            if (o, d) != (req.origin, req.dest) and o != d:
                pairs.append((o, d))
    pairs = pairs[:6]

    async def one(o: str, d: str) -> list[Offer]:
        r = ShopRequest(
            origin=o,
            dest=d,
            date=req.date,
            adults=req.adults,
            cabin=req.cabin,
            currency=req.currency,
            nonstop=True,
            max_offers=4,
        )
        offers = await _demo.shop(r)
        if amadeus:
            try:
                offers.extend(await amadeus.shop(r))
            except httpx.HTTPError:
                pass
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
    amadeus: AmadeusProvider | None,
) -> list[HiddenCityMatch]:
    candidates = hidden_city_candidates(req.origin, req.dest, settings.max_hidden_candidates)
    local_price = _demo.local_price(req.origin, req.dest, req.date, req.cabin)
    # Build a representative local nonstop for pairing.
    local_offers = await _demo.shop(replace(req, nonstop=True, max_offers=3))
    local = next((o for o in local_offers if o.stops == 0), None)
    if local is None and local_offers:
        local = local_offers[0]
    if local is None:
        return []

    sem = asyncio.Semaphore(settings.max_concurrency)

    async def probe(c: str) -> HiddenCityMatch | None:
        async with sem:
            through = _demo.shop_hidden(req, c)
            live_offers: list[Offer] = []
            if amadeus:
                try:
                    live_offers = await amadeus.shop(
                        ShopRequest(
                            origin=req.origin,
                            dest=c,
                            date=req.date,
                            adults=req.adults,
                            cabin=req.cabin,
                            currency=req.currency,
                            max_offers=10,
                        )
                    )
                except httpx.HTTPError:
                    live_offers = []
            return _select_match(c, local, through, live_offers, local_price)

    parts = await asyncio.gather(*(probe(c) for c in candidates), return_exceptions=True)
    matches: list[HiddenCityMatch] = []
    for part in parts:
        if isinstance(part, HiddenCityMatch):
            matches.append(part)
    return matches


def _select_match(
    beyond: str,
    local: Offer,
    demo_through: Offer | None,
    live_offers: list[Offer],
    fallback_local_price: float,
) -> HiddenCityMatch | None:
    ap = airport(beyond)
    name = f"{ap.city} ({beyond})" if ap else beyond

    def via_b(offer: Offer) -> bool:
        if len(offer.segments) < 2:
            return False
        return (
            offer.segments[0].origin == local.segments[0].origin
            and offer.segments[0].dest == local.segments[0].dest
            and offer.segments[-1].dest == beyond
        )

    live_via = [o for o in live_offers if via_b(o)]
    through = None
    if live_via:
        through = min(live_via, key=lambda o: o.price)
        through.kind = "hidden-city"
    elif demo_through:
        through = demo_through

    if through is None:
        return None

    pair_local = local
    forced = _force_inversion(local.segments[0].origin, local.segments[0].dest, beyond)
    if forced:
        pair_local = local.model_copy(update={"price": forced[0]})
        through = through.model_copy(update={"price": forced[1]})
    if through.price >= pair_local.price:
        # Still allow if live local is higher; otherwise no inversion.
        return None
    if pair_local.price <= 0:
        pair_local = local.model_copy(update={"price": fallback_local_price})

    return attach_risk(
        match_id=f"hc-{through.id}-{beyond}",
        hidden_city=beyond,
        hidden_name=name,
        local=pair_local,
        through=through,
        true_dest=local.segments[0].dest if local.segments else "",
    )


def _dedupe(offers: list[Offer]) -> list[Offer]:
    seen: set[tuple] = set()
    out: list[Offer] = []
    for o in sorted(offers, key=lambda x: x.price):
        key = (o.kind, o.first_flight, o.price, tuple((s.origin, s.dest, s.flight_number) for s in o.segments))
        if key in seen:
            continue
        seen.add(key)
        out.append(o)
    return out
