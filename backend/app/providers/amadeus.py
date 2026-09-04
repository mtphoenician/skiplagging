from __future__ import annotations

import time
from datetime import datetime, timezone
from typing import Any

import httpx

from app.config import Settings
from app.models import Offer, Segment
from app.providers.base import ShopRequest


class AmadeusProvider:
    name = "amadeus"

    def __init__(self, settings: Settings, client: httpx.AsyncClient):
        self._s = settings
        self._client = client
        self._token: str | None = None
        self._token_exp = 0.0

    async def shop(self, req: ShopRequest) -> list[Offer]:
        token = await self._access_token()
        params: dict[str, Any] = {
            "originLocationCode": req.origin,
            "destinationLocationCode": req.dest,
            "departureDate": req.date,
            "adults": req.adults,
            "currencyCode": req.currency,
            "max": min(req.max_offers, 30),
            "nonStop": str(req.nonstop).lower(),
        }
        if req.cabin != "ECONOMY":
            params["travelClass"] = req.cabin
        r = await self._client.get(
            f"{self._s.amadeus_base}/v2/shopping/flight-offers",
            params=params,
            headers={"Authorization": f"Bearer {token}"},
            timeout=25.0,
        )
        if r.status_code >= 400:
            return []
        return self._parse(r.json(), req)

    async def direct_destinations(self, origin: str) -> list[str]:
        """Amadeus Airport Routes: published direct destinations. Not prices."""
        token = await self._access_token()
        r = await self._client.get(
            f"{self._s.amadeus_base}/v1/airport/direct-destinations",
            params={"departureAirportCode": origin, "max": 50},
            headers={"Authorization": f"Bearer {token}"},
            timeout=20.0,
        )
        if r.status_code >= 400:
            return []
        out: list[str] = []
        for item in r.json().get("data") or []:
            code = (item.get("iataCode") or "").upper()
            if len(code) == 3:
                out.append(code)
        return out

    async def _access_token(self) -> str:
        now = time.time()
        if self._token and now < self._token_exp - 30:
            return self._token
        r = await self._client.post(
            f"{self._s.amadeus_base}/v1/security/oauth2/token",
            data={
                "grant_type": "client_credentials",
                "client_id": self._s.amadeus_client_id,
                "client_secret": self._s.amadeus_client_secret,
            },
            headers={"Content-Type": "application/x-www-form-urlencoded"},
            timeout=15.0,
        )
        r.raise_for_status()
        body = r.json()
        self._token = body["access_token"]
        self._token_exp = now + int(body.get("expires_in", 1799))
        return self._token

    def _parse(self, data: dict[str, Any], req: ShopRequest) -> list[Offer]:
        now = datetime.now(timezone.utc).isoformat(timespec="seconds")
        offers: list[Offer] = []
        for raw in data.get("data") or []:
            parsed = self._one(raw, req, now)
            if parsed:
                offers.append(parsed)
        return offers

    def _one(self, raw: dict[str, Any], req: ShopRequest, now: str) -> Offer | None:
        itineraries = raw.get("itineraries") or []
        if not itineraries:
            return None
        segs_raw = itineraries[0].get("segments") or []
        if not segs_raw:
            return None
        fare_by_seg = _fare_map(raw)
        segments: list[Segment] = []
        for s in segs_raw:
            dep, arr = s.get("departure") or {}, s.get("arrival") or {}
            sid = str(s.get("id") or "")
            fare = fare_by_seg.get(sid, {})
            segments.append(
                Segment(
                    origin=dep.get("iataCode") or "",
                    dest=arr.get("iataCode") or "",
                    carrier=s.get("carrierCode") or "XX",
                    operating_carrier=(s.get("operating") or {}).get("carrierCode"),
                    flight_number=f"{s.get('carrierCode', 'XX')}{s.get('number', '')}",
                    dep=(dep.get("at") or "")[:16],
                    arr=(arr.get("at") or "")[:16],
                    duration_min=_iso_minutes(s.get("duration") or "PT0M"),
                    rbd=fare.get("class") or fare.get("rbd") or "",
                    fare_basis=fare.get("fareBasis"),
                    aircraft=(s.get("aircraft") or {}).get("code") or "",
                )
            )
        if not segments[0].origin or not segments[-1].dest:
            return None
        price = raw.get("price") or {}
        grand = _f(price.get("grandTotal"))
        base = _f(price.get("base"))
        taxes = None
        if grand is not None and base is not None:
            taxes = round(grand - base, 2)
        kind = "nonstop" if len(segments) == 1 else "connecting"
        return Offer(
            id=f"amadeus-{raw.get('id', segments[0].flight_number)}",
            kind=kind,  # type: ignore[arg-type]
            channel="gds",
            source="amadeus",
            layer="priced-offer",
            segments=segments,
            price=grand,
            base_price=base,
            taxes=taxes,
            currency=price.get("currency") or req.currency,
            cabin=req.cabin,
            fare_basis=segments[0].fare_basis or "NA",
            seats=int(raw["numberOfBookableSeats"]) if raw.get("numberOfBookableSeats") is not None else None,
            last_ticketing_date=raw.get("lastTicketingDate"),
            validating_airline=(raw.get("validatingAirlineCodes") or [None])[0],
            instant_ticketing=raw.get("instantTicketingRequired"),
            refundable=False,
            bags_included=0,
            carrier=segments[0].carrier,
            duration_min=_iso_minutes(itineraries[0].get("duration") or "PT0M")
            or sum(s.duration_min for s in segments),
            stops=max(len(segments) - 1, 0),
            first_flight=segments[0].flight_number,
            retrieved_at=now,
            note="Amadeus Flight Offers Search priced itinerary. Not a PNR. Confirm with Flight Offers Price before any booking API.",
        )


def _fare_map(raw: dict[str, Any]) -> dict[str, dict]:
    out: dict[str, dict] = {}
    for traveler in raw.get("travelerPricings") or []:
        for fd in traveler.get("fareDetailsBySegment") or []:
            sid = str(fd.get("segmentId") or "")
            if sid:
                out[sid] = fd
        break
    return out


def _f(v: Any) -> float | None:
    if v is None or v == "":
        return None
    try:
        return float(v)
    except (TypeError, ValueError):
        return None


def _iso_minutes(iso: str) -> int:
    iso = (iso or "").replace("PT", "")
    hours = mins = 0
    if "H" in iso:
        h, iso = iso.split("H", 1)
        hours = int(h or 0)
    if "M" in iso:
        m, _ = iso.split("M", 1)
        mins = int(m or 0)
    return hours * 60 + mins
