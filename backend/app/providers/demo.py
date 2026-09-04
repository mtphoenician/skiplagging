from __future__ import annotations

import hashlib
from datetime import datetime, timedelta
from math import log1p

from app.graph.airports import AIRPORTS, airport, haversine
from app.graph.hubs import HUBS, SEEDED
from app.models import Offer, Segment
from app.providers.base import ShopRequest

CARRIERS = {
    "ATL": "DL",
    "DTW": "DL",
    "MSP": "DL",
    "SLC": "DL",
    "JFK": "DL",
    "LGA": "DL",
    "DFW": "AA",
    "CLT": "AA",
    "MIA": "AA",
    "PHL": "AA",
    "PHX": "AA",
    "DCA": "AA",
    "ORD": "UA",
    "DEN": "UA",
    "IAH": "UA",
    "SFO": "UA",
    "EWR": "UA",
    "IAD": "UA",
    "SEA": "AS",
    "LHR": "BA",
    "CDG": "AF",
    "AMS": "KL",
    "FRA": "LH",
    "MUC": "LH",
    "MAD": "IB",
    "DXB": "EK",
    "DOH": "QR",
    "IST": "TK",
    "SIN": "SQ",
    "YYZ": "AC",
}

CABIN_MULT = {
    "ECONOMY": 1.0,
    "PREMIUM_ECONOMY": 1.7,
    "BUSINESS": 3.15,
    "FIRST": 4.6,
}

RBD = {
    "ECONOMY": ("Y", "B", "M", "H", "K", "L"),
    "PREMIUM_ECONOMY": ("W", "P"),
    "BUSINESS": ("J", "C", "D"),
    "FIRST": ("F", "A"),
}


def _seed(*parts: object) -> int:
    h = hashlib.blake2s(repr(parts).encode(), digest_size=8).digest()
    return int.from_bytes(h, "big")


def _carrier_for(code: str) -> str:
    return CARRIERS.get(code, (airport(code).hub_carriers[0] if airport(code) and airport(code).hub_carriers else "XX"))


def _flight_no(carrier: str, origin: str, dest: str, date: str, extra: int = 0) -> str:
    n = 100 + (_seed(carrier, origin, dest, date, extra) % 880)
    return f"{carrier}{n}"


def _clock(origin: str, dest: str, date: str, wave: int) -> tuple[str, str, int]:
    km = max(haversine(origin, dest), 200)
    block = int(38 + km / 12.5)
    dep_min = 6 * 60 + (wave * 47 + _seed(origin, dest, date) % 40) % (16 * 60)
    dep_min = min(max(dep_min, 360), 1260)
    start = datetime.fromisoformat(f"{date}T00:00:00") + timedelta(minutes=dep_min)
    end = start + timedelta(minutes=block)
    return start.strftime("%Y-%m-%dT%H:%M"), end.strftime("%Y-%m-%dT%H:%M"), block


def _rbd(cabin: str, cheap: bool) -> str:
    letters = RBD.get(cabin, RBD["ECONOMY"])
    return letters[-1] if cheap else letters[1]


def _fare_basis(rbd: str, date: str) -> str:
    return f"{rbd}14NR{date[5:7]}"


def _days_out(date: str) -> int:
    try:
        d = datetime.fromisoformat(date).date()
        return max((d - datetime.now().date()).days, 0)
    except ValueError:
        return 21


def local_fare(origin: str, dest: str, date: str, cabin: str) -> float:
    """O-D local price: distance + hub monopoly premium + late-booking spike."""
    km = haversine(origin, dest)
    base = 68 + 0.092 * km + 18 * log1p(km)
    dest_ap = airport(dest)
    origin_ap = airport(origin)
    if dest in HUBS:
        base *= 1.38
    if origin in HUBS and dest not in HUBS:
        base *= 1.08
    # Weak local competition into a fortress hub.
    if dest_ap and dest_ap.hub_carriers and dest in HUBS:
        base *= 1.16
    days = _days_out(date)
    if days <= 7:
        base *= 1.42
    elif days <= 14:
        base *= 1.18
    elif days >= 60:
        base *= 0.86
    jitter = 0.92 + (_seed(origin, dest, date, "local") % 170) / 1000
    price = base * CABIN_MULT[cabin] * jitter
    if origin_ap and dest_ap and origin_ap.country != dest_ap.country:
        price += 42
    return round(price, 2)


def through_fare(origin: str, dest: str, beyond: str, date: str, cabin: str) -> float:
    """A-C through fare: competitive beyond-hub product, often below local A-B."""
    km = haversine(origin, dest) + 0.35 * haversine(dest, beyond)
    base = 74 + 0.071 * km + 12 * log1p(km)
    # Strong A-C competition compresses through fares.
    if beyond in HUBS:
        base *= 0.94
    else:
        base *= 0.82
    days = _days_out(date)
    if days <= 7:
        base *= 1.08  # through stays competitive near departure
    elif days >= 60:
        base *= 0.9
    jitter = 0.90 + (_seed(origin, dest, beyond, date, "thru") % 160) / 1000
    price = base * CABIN_MULT[cabin] * jitter
    origin_ap, beyond_ap = airport(origin), airport(beyond)
    if origin_ap and beyond_ap and origin_ap.country != beyond_ap.country:
        price += 55
    else:
        price += 18
    return round(price, 2)


def _force_inversion(origin: str, dest: str, beyond: str) -> tuple[float, float] | None:
    """Replay the published historical quotations when the city pair matches."""
    key = (origin, dest)
    if key == ("ORD", "DCA") and beyond == "BOS":
        return 387.0, 178.0
    if key == ("JFK", "SFO") and beyond == "SEA":
        return 300.0, 170.0
    if key == ("GNV", "CLT") and beyond in {"LGA", "JFK", "EWR"}:
        return 340.0, 196.0
    seeded = SEEDED.get(key)
    if seeded and beyond == seeded[0]:
        local = local_fare(origin, dest, "2099-01-01", "ECONOMY")
        return round(local * 1.05, 2), round(local * 0.52, 2)
    return None


def _offer(
    *,
    kind: str,
    origin: str,
    dest: str,
    date: str,
    cabin: str,
    currency: str,
    price: float,
    segments: list[Segment],
    cheap_inventory: bool,
    extra: str = "",
) -> Offer:
    carrier = segments[0].carrier
    rbd = segments[0].rbd
    first = segments[0].flight_number
    oid = hashlib.blake2s(
        f"{kind}|{first}|{dest}|{date}|{price}|{extra}".encode(), digest_size=6
    ).hexdigest()
    duration = sum(s.duration_min for s in segments)
    if len(segments) == 2:
        # add connect time already baked into clock offsets
        pass
    return Offer(
        id=f"demo-{oid}",
        kind=kind,  # type: ignore[arg-type]
        channel="demo",
        source="demo",
        segments=segments,
        price=price,
        currency=currency,
        cabin=cabin,  # type: ignore[arg-type]
        fare_basis=_fare_basis(rbd, date),
        seats=3 + (_seed(oid) % 6),
        refundable=not cheap_inventory,
        bags_included=0 if cheap_inventory else 1,
        carrier=carrier,
        duration_min=duration,
        stops=max(len(segments) - 1, 0),
        first_flight=first,
    )


def _nonstop_segment(origin: str, dest: str, date: str, cabin: str, cheap: bool, wave: int = 0) -> Segment:
    carrier = _carrier_for(dest if dest in HUBS else origin)
    dep, arr, mins = _clock(origin, dest, date, wave)
    return Segment(
        origin=origin,
        dest=dest,
        carrier=carrier,
        flight_number=_flight_no(carrier, origin, dest, date, wave),
        dep=dep,
        arr=arr,
        duration_min=mins,
        rbd=_rbd(cabin, cheap),
        aircraft="7M8" if mins > 240 else "32Q",
    )


class DemoProvider:
    """
    Instant, economically grounded fare engine.

    Prices are not live tickets. They reproduce the O-D inversion mechanism
    (hub local power vs competitive through markets) so the search, ranking
    and risk layers can be exercised without a GDS key.
    """

    name = "demo"

    async def shop(self, req: ShopRequest) -> list[Offer]:
        o, d = req.origin.upper(), req.dest.upper()
        if o not in AIRPORTS or d not in AIRPORTS or o == d:
            return []
        offers = [self._local_nonstop(req, o, d, wave=0)]
        if not req.nonstop:
            via = self._best_via(o, d)
            if via:
                offers.append(self._connecting(req, o, via, d))
        return offers[: req.max_offers]

    def shop_hidden(self, req: ShopRequest, beyond: str) -> Offer | None:
        o, b, c = req.origin.upper(), req.dest.upper(), beyond.upper()
        if c not in AIRPORTS or c in {o, b}:
            return None
        forced = _force_inversion(o, b, c)
        price = forced[1] if forced else through_fare(o, b, c, req.date, req.cabin)
        first = _nonstop_segment(o, b, req.date, req.cabin, cheap=True, wave=0)
        # Connection: 70–110 minutes after arrival
        connect = 75 + (_seed(o, b, c) % 35)
        km = haversine(b, c)
        block = int(38 + km / 12.5)
        start = datetime.fromisoformat(first.arr) + timedelta(minutes=connect)
        end = start + timedelta(minutes=block)
        carrier = first.carrier
        second = Segment(
            origin=b,
            dest=c,
            carrier=carrier,
            flight_number=_flight_no(carrier, b, c, req.date, 9),
            dep=start.strftime("%Y-%m-%dT%H:%M"),
            arr=end.strftime("%Y-%m-%dT%H:%M"),
            duration_min=block,
            rbd=_rbd(req.cabin, True),
            aircraft="32Q",
        )
        return _offer(
            kind="hidden-city",
            origin=o,
            dest=c,
            date=req.date,
            cabin=req.cabin,
            currency=req.currency,
            price=price,
            segments=[first, second],
            cheap_inventory=True,
            extra=c,
        )

    def local_price(self, origin: str, dest: str, date: str, cabin: str, beyond: str | None = None) -> float:
        if beyond:
            forced = _force_inversion(origin, dest, beyond)
            if forced:
                return forced[0]
        return local_fare(origin, dest, date, cabin)

    def _local_nonstop(self, req: ShopRequest, o: str, d: str, wave: int) -> Offer:
        price = local_fare(o, d, req.date, req.cabin)
        seg = _nonstop_segment(o, d, req.date, req.cabin, cheap=False, wave=wave)
        return _offer(
            kind="nonstop",
            origin=o,
            dest=d,
            date=req.date,
            cabin=req.cabin,
            currency=req.currency,
            price=price,
            segments=[seg],
            cheap_inventory=False,
        )

    def _connecting(self, req: ShopRequest, o: str, via: str, d: str) -> Offer:
        # Genuine A-via-D to true destination B — a real way to buy A→B.
        km = haversine(o, via) + haversine(via, d)
        price = round((62 + 0.078 * km) * CABIN_MULT[req.cabin], 2)
        days = _days_out(req.date)
        if days <= 7:
            price = round(price * 1.22, 2)
        first = _nonstop_segment(o, via, req.date, req.cabin, cheap=True, wave=1)
        connect = 55 + (_seed(o, via, d) % 40)
        km2 = haversine(via, d)
        block = int(38 + km2 / 12.5)
        start = datetime.fromisoformat(first.arr) + timedelta(minutes=connect)
        end = start + timedelta(minutes=block)
        carrier = _carrier_for(via)
        second = Segment(
            origin=via,
            dest=d,
            carrier=carrier,
            flight_number=_flight_no(carrier, via, d, req.date, 3),
            dep=start.strftime("%Y-%m-%dT%H:%M"),
            arr=end.strftime("%Y-%m-%dT%H:%M"),
            duration_min=block,
            rbd=_rbd(req.cabin, True),
            aircraft="32Q",
        )
        return _offer(
            kind="connecting",
            origin=o,
            dest=d,
            date=req.date,
            cabin=req.cabin,
            currency=req.currency,
            price=price,
            segments=[first, second],
            cheap_inventory=True,
            extra=via,
        )

    def _best_via(self, o: str, d: str) -> str | None:
        if d in HUBS:
            # Already flying into a hub — connect via another nearby hub.
            candidates = [h for h in ("ATL", "ORD", "DFW", "CLT", "DEN", "DTW") if h not in {o, d}]
        else:
            candidates = [h for h in ("ATL", "ORD", "DFW", "CLT", "DEN", "DTW", "IAD") if h not in {o, d}]
        best = None
        best_km = 1e12
        direct = haversine(o, d)
        for v in candidates:
            detour = haversine(o, v) + haversine(v, d)
            if detour < best_km and detour < direct * 2.4:
                best_km = detour
                best = v
        return best
