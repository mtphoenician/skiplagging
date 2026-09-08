"""Shop many A→C itineraries, then price every layover B, and persist inversions."""

from __future__ import annotations

import argparse
import asyncio
from collections import defaultdict
from datetime import date, timedelta

import httpx

from app.config import get_settings
from app.db import repo
from app.db.session import init_db, session_factory
from app.engines.risk import attach_risk
from app.engines.shop import _drop_sandbox, _priced_in_currency, _shop_providers, layover_minutes
from app.models import Offer
from app.providers.amadeus import AmadeusProvider
from app.providers.base import ShopRequest
from app.providers.bookers import booker_links
from app.providers.duffel import DuffelProvider
from app.providers.sandbox import is_live_fare


def _scan_dates() -> list[str]:
    """A few weeks out so suppliers still hold inventory. Not a fixed calendar."""
    today = date.today()
    return [(today + timedelta(days=offset)).isoformat() for offset in (21, 35, 49)]


def _cheapest_to(offers: list[Offer], dest: str) -> Offer | None:
    priced = [
        o
        for o in offers
        if o.price is not None and o.segments and o.segments[-1].dest.upper() == dest.upper()
    ]
    priced = _priced_in_currency(priced)
    if not priced:
        return None
    return min(priced, key=lambda o: (o.price or 1e12, layover_minutes(o), o.duration_min))


async def _shop(
    req: ShopRequest,
    amadeus: AmadeusProvider | None,
    duffel: DuffelProvider | None,
    sem: asyncio.Semaphore,
) -> list[Offer]:
    async with sem:
        try:
            offers = await _shop_providers(req, amadeus, duffel, purpose="discover")
        except Exception as exc:
            print(f"  shop error {req.origin}->{req.dest} {req.date}: {exc}", flush=True)
            return []
        await asyncio.sleep(0.35)
        return _priced_in_currency(_drop_sandbox(offers))


async def _dests_for(origin: str, extra: list[str], limit: int, skip: set[str]) -> list[str]:
    async with session_factory()() as session:
        spokes = [code for code, _, _ in await repo.destinations_from(session, origin, limit=limit)]
    out: list[str] = []
    seen = {origin.upper(), *skip}
    for code in [*extra, *spokes]:
        c = code.upper()
        if len(c) != 3 or c in seen:
            continue
        seen.add(c)
        out.append(c)
    return out


async def discover_hidden_deals(
    *,
    limit: int = 28,
    dests_per_origin: int = 22,
    day: str | None = None,
    dates: list[str] | None = None,
) -> dict:
    settings = get_settings()
    await init_db()
    days = [day] if day else list(dates or _scan_dates())
    skip = settings.skip_dests
    if not settings.amadeus_enabled and not settings.duffel_enabled:
        return {"scanned": 0, "found_this_run": 0, "stored": 0, "errors": ["No shop keys"], "dates": days}
    if not settings.publish_deals:
        return {
            "scanned": 0,
            "found_this_run": 0,
            "stored": 0,
            "errors": [
                "Need a Duffel live token (duffel_live_…) or Amadeus production to publish deals. "
                "Test inventory is not saved."
            ],
            "dates": days,
        }

    found = 0
    scanned = 0
    shops = 0
    connecting_seen = 0
    compared = 0
    empty_shops = 0
    closest: list[tuple[float, str]] = []
    errors: list[str] = []
    extras_by_origin: dict[str, list[str]] = defaultdict(list)
    async with session_factory()() as session:
        hubs = await repo.busiest_airports(session, limit=limit, exclude=skip)
        for o in hubs:
            extras_by_origin[o] = [
                d for d, _, _ in await repo.destinations_from(session, o, limit=8) if d.upper() not in skip
            ]

    sem = asyncio.Semaphore(max(2, min(settings.max_concurrency, 3)))

    async with httpx.AsyncClient(timeout=httpx.Timeout(45.0, connect=10.0)) as client:
        amadeus = AmadeusProvider(settings, client) if settings.amadeus_enabled else None
        duffel = DuffelProvider(settings, client) if settings.duffel_enabled else None
        for date in days:
            print(f"\n===== {date} =====", flush=True)
            for origin in hubs:
                dests = await _dests_for(origin, extras_by_origin.get(origin, []), dests_per_origin, skip)
                if not dests:
                    continue
                scanned += 1
                print(f"{origin} {date}: shopping {len(dests)} destinations", flush=True)
                by_dest: dict[str, list[Offer]] = {}
                all_offers: list[Offer] = []
                for dest in dests:
                    req = ShopRequest(origin=origin, dest=dest, date=date, nonstop=False, max_offers=25, currency="USD")
                    offers = await _shop(req, amadeus, duffel, sem)
                    shops += 1
                    if not offers:
                        empty_shops += 1
                        continue
                    by_dest[dest] = offers
                    all_offers.extend(offers)

                missing_b: set[str] = set()
                for offer in all_offers:
                    if offer.price is None or len(offer.segments) < 2:
                        continue
                    connecting_seen += 1
                    dest_b = offer.segments[0].dest.upper()
                    dest_c = offer.segments[-1].dest.upper()
                    if dest_b in {origin, dest_c} or dest_b in skip:
                        continue
                    if dest_b not in by_dest:
                        missing_b.add(dest_b)

                for dest_b in sorted(missing_b)[:20]:
                    req = ShopRequest(origin=origin, dest=dest_b, date=date, nonstop=False, max_offers=25, currency="USD")
                    offers = await _shop(req, amadeus, duffel, sem)
                    shops += 1
                    if offers:
                        by_dest[dest_b] = offers
                        all_offers.extend(offers)
                    else:
                        empty_shops += 1

                cheapest_b = {d: _cheapest_to(offers, d) for d, offers in by_dest.items()}
                matches = []
                seen: set[tuple[str, str, str]] = set()
                async with session_factory()() as session:
                    o_ap = await repo.get_airport(session, origin)
                    if not o_ap:
                        errors.append(f"{origin}: unknown")
                        continue
                    for offer in all_offers:
                        if offer.price is None or len(offer.segments) < 2:
                            continue
                        dest_b = offer.segments[0].dest.upper()
                        dest_c = offer.segments[-1].dest.upper()
                        if dest_b in {origin, dest_c}:
                            continue
                        local = cheapest_b.get(dest_b)
                        if local is None or local.price is None or offer.currency != local.currency:
                            continue
                        if not is_live_fare(offer) or not is_live_fare(local):
                            continue
                        compared += 1
                        if offer.price >= local.price:
                            gap = (offer.price / local.price) if local.price else 99
                            closest.append(
                                (
                                    gap,
                                    f"{origin}->{dest_b}->{dest_c} through={offer.price} local={local.price} {offer.currency}",
                                )
                            )
                            continue
                        key = (origin, dest_b, dest_c, date)
                        if key in seen:
                            continue
                        seen.add(key)
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
                        match.bookers = booker_links(origin, dest_c, date, 1, currency="USD")
                        await repo.persist_hidden_deals(
                            session,
                            [match],
                            origin=origin,
                            origin_city=o_ap.city,
                            dest=dest_b,
                            dest_city=b_ap.city,
                            date=date,
                        )
                        matches.append(match)
                        print(
                            f"  HIT {origin}->{dest_b}->{dest_c} through={offer.price} {offer.currency} "
                            f"local={local.price} save={match.gross_saving}",
                            flush=True,
                        )
                found += len(matches)
                print(
                    f"{origin} {date}: {len(matches)} inversion(s), {len(all_offers)} offers, "
                    f"{sum(1 for o in all_offers if len(o.segments) > 1)} connecting",
                    flush=True,
                )

    async with session_factory()() as session:
        deals = await repo.list_hidden_deals(session, limit=120)
    summary = {
        "scanned": scanned,
        "shops": shops,
        "empty_shops": empty_shops,
        "connecting_seen": connecting_seen,
        "compared_cheaper": compared,
        "found_this_run": found,
        "stored": len(deals),
        "errors": errors,
        "dates": days,
        "closest_misses": [row for _gap, row in sorted(closest)[:8]],
    }
    print(summary, flush=True)
    return summary


async def main() -> None:
    parser = argparse.ArgumentParser(description="Scan shop APIs for hidden-city inversions")
    parser.add_argument("--limit", type=int, default=28)
    parser.add_argument("--dests", type=int, default=22)
    parser.add_argument("--date", default="")
    parser.add_argument(
        "--wipe",
        action="store_true",
        help="Delete stored deals before scanning so test-token junk cannot rank above a live scan.",
    )
    args = parser.parse_args()
    if args.wipe:
        await init_db()
        async with session_factory()() as session:
            n = await repo.clear_hidden_deals(session)
            print(f"wiped {n} stored deal(s)", flush=True)
    await discover_hidden_deals(
        limit=args.limit,
        dests_per_origin=args.dests,
        day=args.date or None,
    )


if __name__ == "__main__":
    asyncio.run(main())
