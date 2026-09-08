from __future__ import annotations

import asyncio
from datetime import datetime, timezone
from typing import Any

import httpx

from app.config import Settings
from app.models import Cabin, Offer, Segment
from app.providers.base import ShopRequest

# Duffel's documented ceiling is 2. Omitting the field defaults to 1, which
# never returns two-stop tickets. We still classify a longer ticket if one arrives.
DUFFEL_MAX_CONNECTIONS = 2


def offer_request_body(req: ShopRequest) -> dict:
    connections = 0 if req.nonstop else DUFFEL_MAX_CONNECTIONS
    slices = [
        {
            "origin": req.origin,
            "destination": req.dest,
            "departure_date": req.date,
        }
    ]
    if req.return_date:
        slices.append(
            {
                "origin": req.dest,
                "destination": req.origin,
                "departure_date": req.return_date,
            }
        )
    # Duffel has no offer-request currency field. Quotes arrive in the org
    # billing currency, or the airline's currency for IATA agencies. The
    # shop layer converts every amount to USD before ranking.
    return {
        "data": {
            "slices": slices,
            "passengers": [{"type": "adult"} for _ in range(req.adults)],
            "cabin_class": req.cabin.lower().replace("premium_economy", "premium_economy"),
            "max_connections": connections,
        }
    }


class DuffelProvider:
    name = "duffel"

    def __init__(self, settings: Settings, client: httpx.AsyncClient):
        self._s = settings
        self._client = client

    async def shop(self, req: ShopRequest) -> list[Offer]:
        payload = offer_request_body(req)
        headers = {
            "Authorization": f"Bearer {self._s.duffel_token}",
            "Duffel-Version": "v2",
            "Accept": "application/json",
            "Content-Type": "application/json",
        }
        r = None
        for attempt in range(4):
            r = await self._client.post(
                "https://api.duffel.com/air/offer_requests",
                params={"return_offers": "true"},
                headers=headers,
                json=payload,
                timeout=40.0,
            )
            if r.status_code != 429:
                break
            raw = r.headers.get("retry-after") or r.headers.get("ratelimit-reset") or "3"
            try:
                wait = float(raw)
            except ValueError:
                wait = 3.0
            await asyncio.sleep(min(max(wait, 1.5), 25.0))
        if r is None or r.status_code >= 400:
            return []
        try:
            payload = r.json() or {}
        except Exception:
            return []
        now = datetime.now(timezone.utc).isoformat(timespec="seconds")
        offers: list[Offer] = []
        for raw in (payload.get("data") or {}).get("offers") or []:
            try:
                parsed = _one(raw, req, now)
            except (TypeError, ValueError, KeyError):
                continue
            if parsed:
                offers.append(parsed)
            if len(offers) >= req.max_offers:
                break
        return offers

    async def get_offer(self, offer_id: str, *, cabin: Cabin = "ECONOMY", currency: str = "USD") -> Offer | None:
        """Live re-fetch of one Duffel offer. Price is current; itinerary may still expire."""
        oid = offer_id.removeprefix("duffel-")
        if not oid:
            return None
        headers = {
            "Authorization": f"Bearer {self._s.duffel_token}",
            "Duffel-Version": "v2",
            "Accept": "application/json",
        }
        r = await self._client.get(
            f"https://api.duffel.com/air/offers/{oid}",
            headers=headers,
            timeout=20.0,
        )
        if r.status_code >= 400:
            return None
        try:
            raw = (r.json() or {}).get("data") or {}
        except Exception:
            return None
        if not raw:
            return None
        dummy = ShopRequest(origin="AAA", dest="BBB", date="2099-01-01", cabin=cabin, currency=currency)
        try:
            parsed = _one(raw, dummy, datetime.now(timezone.utc).isoformat(timespec="seconds"))
        except (TypeError, ValueError, KeyError):
            return None
        return parsed

    async def refresh_offer(self, offer: Offer) -> Offer | None:
        """GET the stored offer id, else re-shop the ticketed city-pair and match the path."""
        from app.engines.hidden import itinerary_fingerprint, ticketed_destination

        fresh = await self.get_offer(offer.id, cabin=offer.cabin, currency="USD")
        if fresh is not None and fresh.live is not False:
            return fresh
        if not offer.segments:
            return None
        first = offer.segments[0]
        date = (first.dep or "")[:10]
        if not date:
            return None
        outbound_n = (offer.outbound_end + 1) if offer.outbound_end is not None else len(offer.segments)
        req = ShopRequest(
            origin=first.origin,
            dest=ticketed_destination(offer) or offer.segments[-1].dest,
            date=date,
            adults=1,
            cabin=offer.cabin,
            currency="USD",
            nonstop=outbound_n == 1,
            max_offers=20,
            return_date=offer.return_date,
        )
        want = itinerary_fingerprint(offer)
        for candidate in await self.shop(req):
            if itinerary_fingerprint(candidate) == want:
                return candidate
        return None


def _one(raw: dict[str, Any], req: ShopRequest, now: str) -> Offer | None:
    slices = raw.get("slices") or []
    if not slices:
        return None
    segments: list[Segment] = []
    outbound_end = -1
    total_mins = 0
    for slice_i, sl in enumerate(slices):
        segs_raw = sl.get("segments") or []
        if not segs_raw:
            return None
        for s in segs_raw:
            origin = (s.get("origin") or {}).get("iata_code") or ""
            dest = (s.get("destination") or {}).get("iata_code") or ""
            mkt = s.get("marketing_carrier") or {}
            op = s.get("operating_carrier") or {}
            segments.append(
                Segment(
                    origin=origin,
                    dest=dest,
                    carrier=mkt.get("iata_code") or "XX",
                    operating_carrier=op.get("iata_code"),
                    flight_number=f"{mkt.get('iata_code', 'XX')}{s.get('marketing_carrier_flight_number', '')}",
                    dep=s.get("departing_at") or "",
                    arr=s.get("arriving_at") or "",
                    duration_min=_dur(s.get("duration")),
                    rbd="",
                    aircraft=(s.get("aircraft") or {}).get("iata_code") or "",
                )
            )
        if slice_i == 0:
            outbound_end = len(segments) - 1
        total_mins += _dur(sl.get("duration"))
    if not segments or outbound_end < 0:
        return None
    owner = (raw.get("owner") or {}).get("iata_code") or segments[0].carrier
    live = bool(raw.get("live_mode"))
    outbound_n = outbound_end + 1
    ret = None
    if len(slices) > 1:
        inbound = slices[1]
        ret = (inbound.get("departure_date") or "")[:10] or None
        if not ret:
            first_in = (inbound.get("segments") or [{}])[0]
            ret = (first_in.get("departing_at") or "")[:10] or None
        if not ret:
            ret = req.return_date
    return Offer(
        id=f"duffel-{raw.get('id', '')}",
        kind="nonstop" if outbound_n == 1 else "connecting",
        channel="ndc",
        source="duffel",
        layer="priced-offer",
        segments=segments,
        price=_f(raw.get("total_amount")),
        base_price=_f(raw.get("base_amount")),
        taxes=_f(raw.get("tax_amount")),
        currency=raw.get("total_currency") or req.currency,
        cabin=req.cabin,
        fare_basis="",
        seats=None,
        validating_airline=owner,
        refundable=False,
        carrier=segments[0].carrier,
        duration_min=total_mins or sum(s.duration_min for s in segments),
        stops=max(outbound_n - 1, 0),
        first_flight=segments[0].flight_number,
        retrieved_at=now,
        expires_at=raw.get("expires_at"),
        live=live,
        return_date=ret,
        outbound_end=outbound_end,
        note=(
            "Duffel offer. live_mode="
            + str(live)
            + ". Not an order. Creating an order would require passenger + payment and is disabled here."
            + ("" if live else " Test tokens include Duffel Airways sandbox inventory.")
        ),
    )


def _f(v: Any) -> float | None:
    try:
        return float(v) if v is not None else None
    except (TypeError, ValueError):
        return None


def _dur(iso: str | None) -> int:
    if not iso:
        return 0
    raw = iso.strip().upper()
    if not raw.startswith("P"):
        return 0
    days = hours = minutes = seconds = 0
    body = raw[1:]
    if "T" in body:
        date_part, time_part = body.split("T", 1)
    else:
        date_part, time_part = body, ""
    try:
        if "D" in date_part:
            days = int(date_part.split("D", 1)[0] or 0)
        if "H" in time_part:
            hours, time_part = time_part.split("H", 1)
            hours = int(hours or 0)
        if "M" in time_part:
            minutes, time_part = time_part.split("M", 1)
            minutes = int(minutes or 0)
        if "S" in time_part:
            seconds = int(time_part.split("S", 1)[0] or 0)
    except (TypeError, ValueError):
        return 0
    return days * 1440 + hours * 60 + minutes + seconds // 60
