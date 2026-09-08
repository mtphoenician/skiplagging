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

# Long-haul change-of-plane cities. OpenFlights overlap ranks them per search.
WORLD_HUBS = (
    "DXB", "DOH", "AUH", "SHJ", "IST", "SIN", "KUL", "DPS", "BKK", "HKG",
    "SYD", "MEL", "FRA", "CDG", "LHR", "AMS", "ICN", "NRT", "LAX", "JFK",
)

HUB_REGION = {
    "DXB": "gulf",
    "DOH": "gulf",
    "AUH": "gulf",
    "SHJ": "gulf",
    "IST": "tr",
    "SIN": "asean",
    "KUL": "asean",
    "DPS": "asean",
    "BKK": "asean",
    "HKG": "ea",
    "ICN": "ea",
    "NRT": "ea",
    "SYD": "oc",
    "MEL": "oc",
    "FRA": "eu",
    "CDG": "eu",
    "LHR": "eu",
    "AMS": "eu",
    "LAX": "na",
    "JFK": "na",
}

OCEANIA = {
    "CBR", "SYD", "MEL", "BNE", "PER", "ADL", "OOL", "HBA", "CNS", "AKL", "CHC", "WLG",
}

# Deep pass: extra A→H1 + H1→H2 + H2→B stitches (Kayak's KUL + DPS self-transfers).
BRIDGE_PAIRS = (("KUL", "DPS"), ("KUL", "SYD"), ("SIN", "SYD"), ("SIN", "DPS"))


def preferred_hubs(origin: str, dest: str) -> list[str]:
    dest_u, origin_u = dest.upper(), origin.upper()
    if dest_u in OCEANIA:
        return ["KUL", "SIN", "DPS", "SYD", "MEL", "DXB", "DOH", "SHJ"]
    if origin_u in OCEANIA:
        return ["SIN", "KUL", "DXB", "DOH", "LHR"]
    return list(WORLD_HUBS)


def bridge_pairs(hubs: list[str], dest: str, limit: int = 2) -> list[tuple[str, str]]:
    """Only Oceania long-haul needs a 3-ticket bridge (KUL→DPS). No fallback mid-leg."""
    chosen = {h.upper() for h in hubs}
    wanted: list[tuple[str, str]] = []
    if dest.upper() in OCEANIA:
        for a, b in BRIDGE_PAIRS:
            if a in chosen and b in chosen and a != b:
                wanted.append((a, b))
    return wanted[:limit]


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


def pick_transfer_hubs(
    origin: str,
    dest: str,
    from_origin: set[str],
    into_dest: set[str],
    limit: int = 4,
) -> list[str]:
    if limit <= 0:
        return []
    origin_u, dest_u = origin.upper(), dest.upper()
    exclude = {origin_u, dest_u}
    from_u = {c.upper() for c in from_origin}
    into_u = {c.upper() for c in into_dest}
    preferred = preferred_hubs(origin_u, dest_u)
    ranked: list[tuple[int, int, int, str]] = []
    seen: set[str] = set()
    pool = preferred + list(WORLD_HUBS) + sorted(from_u | into_u)
    for hub in pool:
        h = hub.upper()
        if len(h) != 3 or h in exclude or h in seen:
            continue
        seen.add(h)
        pref = preferred.index(h) if h in preferred else 80
        overlap = int(h in from_u) + int(h in into_u)
        world = 0 if h in WORLD_HUBS else 1
        ranked.append((pref, -overlap, world, h))
    ranked.sort()
    out: list[str] = []
    region_n: dict[str, int] = {}
    region_cap = 3 if dest_u in OCEANIA else 2
    for _, _, _, h in ranked:
        region = HUB_REGION.get(h, h)
        if region_n.get(region, 0) >= region_cap:
            continue
        region_n[region] = region_n.get(region, 0) + 1
        out.append(h)
        if len(out) >= limit:
            return out
    for _, _, _, h in ranked:
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
        return sorted(offers, key=lambda o: (o.price or 1e12, o.duration_min, o.stops))[:n]

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
    out.sort(key=lambda o: (o.price or 1e12, o.duration_min, o.stops))
    seen: set[str] = set()
    uniq: list[Offer] = []
    for offer in out:
        key = tuple((s.origin, s.dest, s.flight_number, (s.dep or "")[:16]) for s in offer.segments)
        if key in seen:
            continue
        seen.add(key)
        uniq.append(offer)
        if len(uniq) >= limit:
            break
    return uniq
