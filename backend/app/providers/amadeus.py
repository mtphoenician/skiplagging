from __future__ import annotations

import time
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
        params = {
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
            timeout=20.0,
        )
        if r.status_code >= 400:
            return []
        data = r.json()
        return self._parse(data, req)

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
        offers: list[Offer] = []
        for raw in data.get("data") or []:
            parsed = self._one(raw, req)
            if parsed:
                offers.append(parsed)
        return offers

    def _one(self, raw: dict[str, Any], req: ShopRequest) -> Offer | None:
        itineraries = raw.get("itineraries") or []
        if not itineraries:
            return None
        segs_raw = itineraries[0].get("segments") or []
        if not segs_raw:
            return None
        segments: list[Segment] = []
        for s in segs_raw:
            dep = s.get("departure") or {}
            arr = s.get("arrival") or {}
            dep_at = (dep.get("at") or "")[:16]
            arr_at = (arr.get("at") or "")[:16]
            duration = _iso_minutes(s.get("duration") or itineraries[0].get("duration") or "PT0M")
            segments.append(
                Segment(
                    origin=dep.get("iataCode") or "",
                    dest=arr.get("iataCode") or "",
                    carrier=s.get("carrierCode") or s.get("operating", {}).get("carrierCode") or "XX",
                    flight_number=f"{s.get('carrierCode', 'XX')}{s.get('number', '')}",
                    dep=dep_at,
                    arr=arr_at,
                    duration_min=duration,
                    rbd=((raw.get("travelerPricings") or [{}])[0]
                         .get("fareDetailsBySegment") or [{}])[0]
                    .get("class")
                    or "Y",
                    aircraft=(s.get("aircraft") or {}).get("code") or "32N",
                )
            )
        if not segments[0].origin or not segments[-1].dest:
            return None
        price = float((raw.get("price") or {}).get("grandTotal") or 0)
        fare_basis = (
            ((raw.get("travelerPricings") or [{}])[0].get("fareDetailsBySegment") or [{}])[0].get("fareBasis")
            or "NA"
        )
        kind = "nonstop" if len(segments) == 1 else "connecting"
        return Offer(
            id=f"amadeus-{raw.get('id', segments[0].flight_number)}",
            kind=kind,  # type: ignore[arg-type]
            channel="gds",
            source="amadeus",
            segments=segments,
            price=round(price, 2),
            currency=(raw.get("price") or {}).get("currency") or req.currency,
            cabin=req.cabin,
            fare_basis=fare_basis,
            seats=int(raw.get("numberOfBookableSeats") or 4),
            refundable=False,
            bags_included=0,
            carrier=segments[0].carrier,
            duration_min=sum(s.duration_min for s in segments),
            stops=max(len(segments) - 1, 0),
            first_flight=segments[0].flight_number,
        )


def _iso_minutes(iso: str) -> int:
    # PT2H15M
    iso = iso.replace("PT", "")
    hours = mins = 0
    if "H" in iso:
        h, iso = iso.split("H", 1)
        hours = int(h or 0)
    if "M" in iso:
        m, _ = iso.split("M", 1)
        mins = int(m or 0)
    return hours * 60 + mins
