"""Virtual interline / self-transfer: stitch separately priced tickets.

Kayak's "self-transfer hack" is not hidden-city. The passenger is going to B.
They buy two or three complete tickets and change terminals themselves.

We never turn those tickets into one fake through-fare. The combined row is a
sum of partner offers, labeled as separate tickets, with unprotected connections.
"""

from __future__ import annotations

from datetime import datetime, timedelta

from app.engines.hidden import detect_hidden_city, is_standard_to, ticketed_destination
from app.models import Offer, SeparateTicket

# 2.5h to collect bags / recheck; 22h so we do not invent an overnight hotel product.
MIN_TRANSFER_MIN = 150
MAX_TRANSFER_MIN = 22 * 60

# Combined self-transfer: 5 flights is already a long day. A 6th is almost
# never cheaper than a simpler 2-ticket stitch, and barely sold as inventory.
MAX_SELF_FLIGHTS = 5
# One coupon: Duffel tops out at 2 connections (3 flights). Longer legs are rare.
MAX_LEG_FLIGHTS = 3


def next_day(date: str) -> str:
    return (datetime.strptime(date, "%Y-%m-%d") + timedelta(days=1)).strftime("%Y-%m-%d")


def connection_minutes(arr: str, dep: str) -> int | None:
    try:
        a = datetime.fromisoformat(arr.replace("Z", "+00:00"))
        b = datetime.fromisoformat(dep.replace("Z", "+00:00"))
    except ValueError:
        return None
    if a.tzinfo is None and b.tzinfo is not None:
        b = b.replace(tzinfo=None)
    elif a.tzinfo is not None and b.tzinfo is None:
        a = a.replace(tzinfo=None)
    gap = int((b - a).total_seconds() // 60)
    return gap


def bridge_pairs(
    hubs: list[str],
    from_origin: set[str] | None = None,
    into_dest: set[str] | None = None,
    limit: int = 2,
) -> list[tuple[str, str]]:
    """3-ticket H1→H2 only when no shared hub can do A→H + H→B.

    H1 is a city the origin already flies to; H2 is a city that already flies
    to B. No fixed KUL/DPS list — the OpenFlights overlap for this search is
    the geography.
    """
    if limit <= 0:
        return []
    chosen = [h.upper() for h in hubs if len(h) == 3]
    from_u = {c.upper() for c in (from_origin or set())}
    into_u = {c.upper() for c in (into_dest or set())}
    if not from_u or not into_u:
        return []
    if any(h in from_u and h in into_u for h in chosen):
        return []
    lefts = [h for h in chosen if h in from_u]
    rights = [h for h in chosen if h in into_u]
    out: list[tuple[str, str]] = []
    for h1 in lefts:
        for h2 in rights:
            if h1 != h2:
                out.append((h1, h2))
            if len(out) >= limit:
                return out
    return out


def pick_transfer_hubs(
    origin: str,
    dest: str,
    from_origin: set[str],
    into_dest: set[str],
    limit: int = 4,
    extra_hubs: list[str] | None = None,
    continents: dict[str, str] | None = None,
) -> list[str]:
    """Rank change-of-plane cities from route overlap, then globally busy airports."""
    if limit <= 0:
        return []
    origin_u, dest_u = origin.upper(), dest.upper()
    exclude = {origin_u, dest_u}
    from_u = {c.upper() for c in from_origin} - exclude
    into_u = {c.upper() for c in into_dest} - exclude
    extra = [h.upper() for h in (extra_hubs or []) if len(h) == 3 and h not in exclude]
    extra_rank = {h: i for i, h in enumerate(extra)}
    seen: set[str] = set()
    ranked: list[tuple[int, int, str]] = []
    for hub in [*from_u, *into_u, *extra]:
        if hub in seen or len(hub) != 3:
            continue
        seen.add(hub)
        overlap = int(hub in from_u) + int(hub in into_u)
        ranked.append((-overlap, extra_rank.get(hub, 80), hub))
    ranked.sort()
    origin_c = (continents or {}).get(origin_u, "")
    dest_c = (continents or {}).get(dest_u, "")
    region_cap = 3 if origin_c and dest_c and origin_c != dest_c else 2
    out: list[str] = []
    region_n: dict[str, int] = {}
    for _, _, h in ranked:
        region = (continents or {}).get(h) or h
        if region_n.get(region, 0) >= region_cap:
            continue
        region_n[region] = region_n.get(region, 0) + 1
        out.append(h)
        if len(out) >= limit:
            return out
    for _, _, h in ranked:
        if h not in out:
            out.append(h)
        if len(out) >= limit:
            break
    return out


def _ticket_date(offer: Offer) -> str:
    dep = offer.segments[0].dep if offer.segments else ""
    return dep[:10] if len(dep) >= 10 else ""


def stitch_chain(parts: list[Offer], intended: str | set[str]) -> Offer | None:
    """Join 2+ complete tickets that meet at their ticketed cities. Not a PNR."""
    if len(parts) < 2:
        return None
    dests = {intended.upper()} if isinstance(intended, str) else {c.upper() for c in intended}
    if any(p.price is None or p.price <= 0 for p in parts):
        return None
    if any(not p.segments for p in parts):
        return None
    ccy = (parts[0].currency or "").upper()
    if not ccy or any((p.currency or "").upper() != ccy for p in parts):
        return None
    cabin = parts[0].cabin
    if any(p.cabin != cabin for p in parts):
        return None
    if parts[0].segments[0].origin.upper() in dests:
        return None
    hubs: list[str] = []
    for left, right in zip(parts, parts[1:]):
        hub = ticketed_destination(left)
        if not hub or hub in dests:
            return None
        if right.segments[0].origin.upper() != hub:
            return None
        if not is_standard_to(left, hub):
            return None
        if detect_hidden_city(left, dests):
            return None
        gap = connection_minutes(left.segments[-1].arr, right.segments[0].dep)
        if gap is None or gap < MIN_TRANSFER_MIN or gap > MAX_TRANSFER_MIN:
            return None
        hubs.append(hub)
    if len(hubs) != len(set(hubs)):
        return None
    last = parts[-1]
    if not is_standard_to(last, dests):
        return None
    if detect_hidden_city(last, dests):
        return None
    segments = [s for p in parts for s in p.segments]
    if len(segments) > MAX_SELF_FLIGHTS:
        return None
    first, last_seg = segments[0], segments[-1]
    door = connection_minutes(first.dep, last_seg.arr)
    carriers = []
    for p in parts:
        if p.carrier and p.carrier not in carriers:
            carriers.append(p.carrier)
    tickets = [
        SeparateTicket(
            origin=p.segments[0].origin.upper(),
            dest=ticketed_destination(p),
            date=_ticket_date(p),
            price=float(p.price or 0),
            currency=ccy,
            carrier=p.carrier,
        )
        for p in parts
    ]
    return Offer(
        id="self-" + "+".join(p.id for p in parts),
        kind="self-transfer",
        channel="ota",
        source="self-transfer",
        layer="priced-offer",
        segments=segments,
        price=round(sum(float(p.price or 0) for p in parts), 2),
        currency=ccy,
        cabin=cabin,
        fare_basis="",
        bags_included=min((p.bags_included for p in parts), default=0),
        carrier=carriers[0] if carriers else last.carrier,
        duration_min=door if door and door > 0 else sum(s.duration_min for s in segments),
        stops=max(len(segments) - 1, 0),
        first_flight=first.flight_number,
        live=all(p.live is not False for p in parts),
        self_transfer_airports=hubs,
        separate_tickets=tickets,
        note=(
            "Self-transfer: "
            + str(len(parts))
            + " separate tickets. You check in again at "
            + ", ".join(hubs)
            + ". A missed connection is not protected. This is not hidden-city."
        ),
    )


def combine_at_hubs(
    inbound: dict[str, list[Offer]],
    outbound: dict[str, list[Offer]],
    intended: set[str],
    mids: dict[tuple[str, str], list[Offer]] | None = None,
    limit: int = 8,
) -> list[Offer]:
    """2-ticket A→H + H→B, plus optional 3-ticket A→H1 + H1→H2 + H2→B."""
    out: list[Offer] = []

    def cheapest(offers: list[Offer], n: int = 4) -> list[Offer]:
        return sorted(offers, key=lambda o: (o.stops, o.price or 1e12, o.duration_min))[:n]

    for hub, lefts in inbound.items():
        rights = outbound.get(hub) or []
        for left in cheapest(lefts):
            for right in cheapest(rights):
                joined = stitch_chain([left, right], intended)
                if joined:
                    out.append(joined)
    for (h1, h2), middles in (mids or {}).items():
        for left in cheapest(inbound.get(h1) or []):
            for mid in cheapest(middles, 3):
                for right in cheapest(outbound.get(h2) or []):
                    joined = stitch_chain([left, mid, right], intended)
                    if joined:
                        out.append(joined)
    out.sort(
        key=lambda o: (
            o.price or 1e12,
            len(o.segments),
            len(o.separate_tickets),
            o.duration_min,
        )
    )
    seen: set[str] = set()
    uniq: list[Offer] = []
    for offer in out:
        key = tuple((s.origin, s.dest, s.flight_number, (s.dep or "")[:16]) for s in offer.segments)
        if key in seen:
            continue
        seen.add(key)
        uniq.append(offer)
    twos = [o for o in uniq if len(o.separate_tickets) <= 2]
    threes = [o for o in uniq if len(o.separate_tickets) >= 3]
    best_two = min((o.price or 1e12) for o in twos) if twos else None
    if best_two is not None:
        threes = [o for o in threes if (o.price or 1e12) < best_two]
    ranked = sorted(
        [*twos, *threes],
        key=lambda o: (
            o.price or 1e12,
            len(o.segments),
            len(o.separate_tickets),
            o.duration_min,
        ),
    )
    return ranked[:limit]
