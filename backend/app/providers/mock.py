"""Deterministic priced offers. Never used as a substitute for a live shop on other pairs."""

from __future__ import annotations

from datetime import datetime, timezone

from app.models import Offer, Segment
from app.providers.base import ShopRequest


def _seg(
    origin: str,
    dest: str,
    dep: str,
    arr: str,
    flight: str,
    carrier: str,
    minutes: int,
) -> Segment:
    return Segment(
        origin=origin,
        dest=dest,
        carrier=carrier,
        operating_carrier=carrier,
        flight_number=flight,
        dep=dep,
        arr=arr,
        duration_min=minutes,
        rbd="Y",
        aircraft="",
    )


def _offer(
    oid: str,
    price: float,
    segments: list[Segment],
    date: str,
) -> Offer:
    return Offer(
        id=oid,
        kind="nonstop" if len(segments) == 1 else "connecting",
        channel="ndc",
        source="mock",
        layer="priced-offer",
        segments=segments,
        price=price,
        base_price=round(price * 0.82, 2),
        taxes=round(price * 0.18, 2),
        currency="USD",
        cabin="ECONOMY",
        fare_basis="Y",
        seats=7,
        validating_airline=segments[0].carrier,
        refundable=False,
        bags_included=0,
        carrier=segments[0].carrier,
        duration_min=sum(s.duration_min for s in segments) + (40 if len(segments) > 1 else 0),
        stops=max(len(segments) - 1, 0),
        first_flight=segments[0].flight_number,
        retrieved_at=datetime.now(timezone.utc).isoformat(timespec="seconds"),
        note="Mock fixture. Complete priced itinerary — not a reconstructed segment sum.",
    )


def _t(date: str, hhmm: str) -> str:
    return f"{date}T{hhmm}:00"


def jfk_ord_nonstop(date: str) -> Offer:
    return _offer(
        "mock-jfk-ord-nonstop",
        240,
        [_seg("JFK", "ORD", _t(date, "08:00"), _t(date, "10:15"), "AA100", "AA", 135)],
        date,
    )


def jfk_ord_den(date: str) -> Offer:
    return _offer(
        "mock-jfk-ord-den",
        170,
        [
            _seg("JFK", "ORD", _t(date, "08:00"), _t(date, "10:15"), "UA200", "UA", 135),
            _seg("ORD", "DEN", _t(date, "11:30"), _t(date, "13:20"), "UA300", "UA", 110),
        ],
        date,
    )


def jfk_ord_sea(date: str) -> Offer:
    return _offer(
        "mock-jfk-ord-sea",
        185,
        [
            _seg("JFK", "ORD", _t(date, "09:00"), _t(date, "11:20"), "AS400", "AS", 140),
            _seg("ORD", "SEA", _t(date, "12:40"), _t(date, "15:10"), "AS500", "AS", 150),
        ],
        date,
    )


def jfk_dfw_lax(date: str) -> Offer:
    return _offer(
        "mock-jfk-dfw-lax",
        150,
        [
            _seg("JFK", "DFW", _t(date, "07:00"), _t(date, "10:30"), "AA600", "AA", 210),
            _seg("DFW", "LAX", _t(date, "11:40"), _t(date, "13:10"), "AA700", "AA", 90),
        ],
        date,
    )


def jfk_dfw_den(date: str) -> Offer:
    """Refresh mutation: no longer passes through ORD."""
    return _offer(
        "mock-jfk-dfw-den",
        150,
        [
            _seg("JFK", "DFW", _t(date, "08:00"), _t(date, "11:10"), "UA200", "UA", 190),
            _seg("DFW", "DEN", _t(date, "12:20"), _t(date, "13:40"), "UA300", "UA", 80),
        ],
        date,
    )


class MockProvider:
    name = "mock"

    async def shop(self, req: ShopRequest) -> list[Offer]:
        origin, dest = req.origin.upper(), req.dest.upper()
        date = req.date
        if origin == "JFK" and dest in {"ORD", "CHI", "MDW"}:
            return [jfk_ord_nonstop(date), jfk_ord_den(date), jfk_ord_sea(date), jfk_dfw_lax(date)]
        if origin == "JFK" and dest == "DEN":
            return [jfk_ord_den(date)]
        if origin == "JFK" and dest == "SEA":
            return [jfk_ord_sea(date)]
        return []

    async def refresh_offer(self, offer: Offer) -> Offer | None:
        date = (offer.segments[0].dep or "")[:10] or datetime.now(timezone.utc).date().isoformat()
        if offer.id == "mock-jfk-ord-den":
            return jfk_ord_den(date).model_copy(update={"price": 175.0, "id": offer.id})
        if offer.id == "mock-jfk-ord-den-mutated":
            return jfk_dfw_den(date)
        if offer.source != "mock":
            return None
        if offer.price is None:
            return None
        return offer.model_copy(update={"price": round(offer.price + 5, 2)})
