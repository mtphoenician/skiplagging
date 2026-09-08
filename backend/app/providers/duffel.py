from __future__ import annotations

import asyncio
from datetime import datetime, timezone
from typing import Any

import httpx

from app.config import Settings
from app.models import Offer, Segment
from app.providers.base import ShopRequest


class DuffelProvider:
    name = "duffel"

    def __init__(self, settings: Settings, client: httpx.AsyncClient):
        self._s = settings
        self._client = client

    async def shop(self, req: ShopRequest) -> list[Offer]:
        payload = {
            "data": {
                "slices": [
                    {
                        "origin": req.origin,
                        "destination": req.dest,
                        "departure_date": req.date,
                    }
                ],
                "passengers": [{"type": "adult"} for _ in range(req.adults)],
                "cabin_class": req.cabin.lower().replace("premium_economy", "premium_economy"),
                "max_connections": 0 if req.nonstop else 1,
            }
        }
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
        now = datetime.now(timezone.utc).isoformat(timespec="seconds")
        offers: list[Offer] = []
        for raw in (r.json().get("data") or {}).get("offers") or []:
            try:
                parsed = _one(raw, req, now)
            except (TypeError, ValueError, KeyError):
                continue
            if parsed:
                offers.append(parsed)
            if len(offers) >= req.max_offers:
                break
        return offers


def _one(raw: dict[str, Any], req: ShopRequest, now: str) -> Offer | None:
    slices = raw.get("slices") or []
    if not slices:
        return None
    segs_raw = slices[0].get("segments") or []
    if not segs_raw:
        return None
    segments = []
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
    owner = (raw.get("owner") or {}).get("iata_code") or segments[0].carrier
    live = bool(raw.get("live_mode"))
    slice_mins = _dur(slices[0].get("duration"))
    return Offer(
        id=f"duffel-{raw.get('id', '')}",
        kind="nonstop" if len(segments) == 1 else "connecting",
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
        duration_min=slice_mins or sum(s.duration_min for s in segments),
        stops=max(len(segments) - 1, 0),
        first_flight=segments[0].flight_number,
        retrieved_at=now,
        expires_at=raw.get("expires_at"),
        live=live,
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
    return days * 1440 + hours * 60 + minutes + seconds // 60
