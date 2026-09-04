from __future__ import annotations

from typing import Any

import httpx

from app.config import Settings
from app.models import BoardFlight


class AeroDataBoxProvider:
    name = "aerodatabox"

    def __init__(self, settings: Settings, client: httpx.AsyncClient):
        self._s = settings
        self._client = client

    async def board(self, iata: str, direction: str = "Departure") -> list[BoardFlight]:
        """Airport FIDS around now. Scheduled/estimated/actual — not a fare, not ADS-B."""
        r = await self._client.get(
            f"https://aerodatabox.p.rapidapi.com/flights/airports/iata/{iata}",
            params={
                "offsetMinutes": "-120",
                "durationMinutes": "720",
                "withLeg": "true",
                "direction": direction,
                "withCancelled": "true",
                "withCodeshared": "true",
                "withCargo": "false",
                "withPrivate": "false",
            },
            headers={
                "X-RapidAPI-Key": self._s.rapidapi_key,
                "X-RapidAPI-Host": "aerodatabox.p.rapidapi.com",
            },
            timeout=25.0,
        )
        if r.status_code >= 400:
            return []
        body = r.json()
        key = "departures" if direction.lower().startswith("dep") else "arrivals"
        flights: list[BoardFlight] = []
        for raw in body.get(key) or []:
            parsed = _one(raw, iata, direction)
            if parsed:
                flights.append(parsed)
        return flights[:40]


def _one(raw: dict[str, Any], iata: str, direction: str) -> BoardFlight | None:
    number = raw.get("number") or raw.get("callSign")
    if not number:
        return None
    airline = (raw.get("airline") or {}).get("iata") or (raw.get("airline") or {}).get("name")
    dep = raw.get("departure") or raw.get("movement") or {}
    arr = raw.get("arrival") or {}
    if direction.lower().startswith("dep"):
        other = (arr.get("airport") or {}).get("iata")
        sched = (dep.get("scheduledTime") or {}).get("local") or dep.get("scheduledTimeLocal")
        est = (dep.get("revisedTime") or {}).get("local") or dep.get("revisedTimeLocal")
        terminal = dep.get("terminal")
        gate = dep.get("gate")
        origin, dest = iata, other
    else:
        other = (dep.get("airport") or {}).get("iata")
        sched = (arr.get("scheduledTime") or {}).get("local") or arr.get("scheduledTimeLocal")
        est = (arr.get("revisedTime") or {}).get("local") or arr.get("revisedTimeLocal")
        terminal = arr.get("terminal")
        gate = arr.get("gate")
        origin, dest = other, iata
    status = (raw.get("status") or "") if isinstance(raw.get("status"), str) else str(raw.get("status") or "")
    return BoardFlight(
        flight_number=str(number),
        carrier=airline,
        origin=origin,
        dest=dest,
        scheduled=str(sched)[:19] if sched else None,
        estimated=str(est)[:19] if est else None,
        status=status or None,
        terminal=str(terminal) if terminal else None,
        gate=str(gate) if gate else None,
        source="aerodatabox",
        layer="schedule-status",
    )
