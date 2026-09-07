"""Shop hub destinations and persist priced hidden-city inversions."""

from __future__ import annotations

import asyncio

import httpx

from app.config import get_settings
from app.db import repo
from app.db.session import init_db, session_factory
from app.engines.risk import attach_risk
from app.engines.shop import _priced_in_currency, _shop_providers, layover_minutes
from app.models import Offer
from app.providers.amadeus import AmadeusProvider
from app.providers.base import ShopRequest
from app.providers.bookers import booker_links
from app.providers.duffel import DuffelProvider

HUBS = ("LHR", "CDG", "AMS", "FRA", "JFK", "ORD", "ATL", "DFW", "LAX", "SFO", "MIA", "BOS")


def _scan_date() -> str:
    # Duffel test inventory is sparse; mid-autumn Thursdays have been populated.
    return "2026-10-22"


def _cheapest_to(offers: list[Offer], dest: str) -> Offer | None:
    priced = [o for o in offers if o.price is not None and o.segments and o.segments[-1].dest == dest]
    priced = _priced_in_currency(priced)
    if not priced:
        return None
    return min(priced, key=lambda o: (o.price or 1e12, layover_minutes(o), o.duration_min))


async def discover_hidden_deals(*, limit: int = 12, day: str | None = None) -> dict:
    settings = get_settings()
    await init_db()
    day = day or _scan_date()
    if not settings.amadeus_enabled and not settings.duffel_enabled:
        return {"scanned": 0, "found_this_run": 0, "stored": 0, "errors": ["No shop keys"], "date": day}

    found = 0
    scanned = 0
    errors: list[str] = []
    hubs = HUBS[:limit]

    async with httpx.AsyncClient(timeout=httpx.Timeout(40.0, connect=8.0)) as client:
        amadeus = AmadeusProvider(settings, client) if settings.amadeus_enabled else None
        duffel = DuffelProvider(settings, client) if settings.duffel_enabled else None
        for origin in hubs:
            async with session_factory()() as session:
                o_ap = await repo.get_airport(session, origin)
                if not o_ap:
                    errors.append(f"{origin}: unknown")
                    continue
                dests = [code for code, _, _ in await repo.destinations_from(session, origin, limit=8)]
                dests = [d for d in dests if d != origin][:8]
            if not dests:
                continue
            scanned += 1
            reqs = [
                ShopRequest(origin=origin, dest=d, date=day, nonstop=False, max_offers=10)
                for d in dests
            ]
            parts = []
            for req in reqs:
                try:
                    parts.append(await _shop_providers(req, amadeus, duffel))
                except Exception as exc:
                    parts.append(exc)
                await asyncio.sleep(0.8)
            by_dest: dict[str, list[Offer]] = {}
            all_offers: list[Offer] = []
            for dest, part in zip(dests, parts):
                if not isinstance(part, list):
                    continue
                by_dest[dest] = part
                all_offers.extend(part)

            cheapest_b = {d: _cheapest_to(offers, d) for d, offers in by_dest.items()}
            matches = []
            seen: set[tuple[str, str, str]] = set()
            for offer in all_offers:
                if offer.price is None or len(offer.segments) < 2:
                    continue
                dest_b = offer.segments[0].dest
                dest_c = offer.segments[-1].dest
                if dest_b == dest_c or dest_b == origin:
                    continue
                local = cheapest_b.get(dest_b)
                if (
                    local is None
                    or local.price is None
                    or offer.currency != local.currency
                    or offer.price >= local.price
                ):
                    continue
                key = (origin, dest_b, dest_c)
                if key in seen:
                    continue
                seen.add(key)
                async with session_factory()() as session:
                    b_ap = await repo.get_airport(session, dest_b)
                    c_ap = await repo.get_airport(session, dest_c)
                    if not b_ap or not c_ap:
                        continue
                    match = attach_risk(
                        match_id=f"disc-{origin}-{dest_b}-{dest_c}-{offer.id}",
                        hidden_city=dest_c,
                        hidden_name=f"{c_ap.city} ({dest_c})",
                        local=local,
                        through=offer,
                        true_dest=dest_b,
                        origin_country=o_ap.country,
                        hidden_country=c_ap.country,
                    )
                    match.bookers = booker_links(origin, dest_c, day, 1)
                    await repo.persist_hidden_deals(
                        session,
                        [match],
                        origin=origin,
                        origin_city=o_ap.city,
                        dest=dest_b,
                        dest_city=b_ap.city,
                        date=day,
                    )
                matches.append(match)
            found += len(matches)
            print(f"{origin}: {len(matches)} hidden-city row(s) from {len(dests)} dest shops", flush=True)

    async with session_factory()() as session:
        deals = await repo.list_hidden_deals(session, limit=120)
    return {"scanned": scanned, "found_this_run": found, "stored": len(deals), "errors": errors, "date": day}


async def main() -> None:
    summary = await discover_hidden_deals()
    print(summary)


if __name__ == "__main__":
    asyncio.run(main())
