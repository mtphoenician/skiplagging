from __future__ import annotations

from datetime import datetime, timezone

import httpx

from app.config import Settings
from app.models import Airport, LiveTraffic, TrackedAircraft, TrackerLink

POSITION_SOURCE = {0: "ADS-B", 1: "ASTERIX", 2: "MLAT", 3: "FLARM"}
TOKEN_URL = "https://auth.opensky-network.org/auth/realms/opensky-network/protocol/openid-connect/token"


class OpenSkyProvider:
    name = "opensky"

    def __init__(self, settings: Settings, client: httpx.AsyncClient):
        self._s = settings
        self._client = client
        self._token: str | None = None
        self._token_exp = 0.0

    async def traffic_near(self, airport: Airport, radius_deg: float = 0.55) -> LiveTraffic:
        """Current transponder states in a box around the airport. Not a schedule."""
        params = {
            "lamin": airport.lat - radius_deg,
            "lamax": airport.lat + radius_deg,
            "lomin": airport.lon - radius_deg,
            "lomax": airport.lon + radius_deg,
        }
        headers = await self._headers()
        r = await self._client.get(
            "https://opensky-network.org/api/states/all",
            params=params,
            headers=headers,
            timeout=25.0,
        )
        note = (
            "OpenSky state vectors: live transponders, not tickets. "
            "Anonymous access is current time only (~10s resolution). "
            "position_source 0=ADS-B 1=ASTERIX 2=MLAT 3=FLARM."
        )
        if r.status_code == 429:
            return self._empty(airport, "OpenSky rate-limited (anonymous credit bucket). Retry shortly.")
        if r.status_code >= 400:
            return self._empty(airport, f"OpenSky /states/all returned HTTP {r.status_code}.")
        body = r.json()
        api_time = body.get("time")
        aircraft: list[TrackedAircraft] = []
        for row in body.get("states") or []:
            if not row or len(row) < 12:
                continue
            aircraft.append(
                TrackedAircraft(
                    icao24=(row[0] or "").lower(),
                    callsign=(row[1] or "").strip() or None,
                    origin_country=row[2],
                    lon=row[5],
                    lat=row[6],
                    baro_altitude_m=row[7],
                    on_ground=bool(row[8]),
                    velocity_ms=row[9],
                    true_track=row[10],
                    position_source=row[16] if len(row) > 16 else None,
                    position_source_name=POSITION_SOURCE.get(row[16], "unknown") if len(row) > 16 else "unknown",
                )
            )
        aircraft.sort(key=lambda a: (not a.on_ground, -(a.baro_altitude_m or 0)))
        icao = airport.icao or airport.iata
        return LiveTraffic(
            airport=airport.iata,
            source="opensky",
            api_time=api_time,
            note=note,
            aircraft=aircraft[:40],
            trackers=tracker_links(airport.iata, icao),
        )

    async def _headers(self) -> dict[str, str]:
        if not (self._s.opensky_client_id and self._s.opensky_client_secret):
            return {}
        import time

        now = time.time()
        if self._token and now < self._token_exp - 30:
            return {"Authorization": f"Bearer {self._token}"}
        r = await self._client.post(
            TOKEN_URL,
            data={
                "grant_type": "client_credentials",
                "client_id": self._s.opensky_client_id,
                "client_secret": self._s.opensky_client_secret,
            },
            timeout=15.0,
        )
        if r.status_code >= 400:
            return {}
        body = r.json()
        self._token = body.get("access_token")
        self._token_exp = now + int(body.get("expires_in", 1700))
        return {"Authorization": f"Bearer {self._token}"} if self._token else {}

    def _empty(self, airport: Airport, note: str) -> LiveTraffic:
        icao = airport.icao or airport.iata
        return LiveTraffic(
            airport=airport.iata,
            source="opensky",
            api_time=int(datetime.now(timezone.utc).timestamp()),
            note=note,
            aircraft=[],
            trackers=tracker_links(airport.iata, icao),
        )


def tracker_links(iata: str, icao: str) -> list[TrackerLink]:
    iata, icao = iata.upper(), (icao or iata).upper()
    return [
        TrackerLink(
            id="opensky",
            name="OpenSky Network",
            layer="live-track",
            role="Crowd ADS-B/MLAT state vectors. This is tracking, not booking.",
            url=f"https://opensky-network.org/",
        ),
        TrackerLink(
            id="flightradar24",
            name="Flightradar24 departures",
            layer="live-track-link",
            role="Commercial tracker board. We do not ingest their feed.",
            url=f"https://www.flightradar24.com/airport/{iata.lower()}/departures",
        ),
        TrackerLink(
            id="flightaware",
            name="FlightAware airport",
            layer="live-track-link",
            role="Commercial status/tracker. We do not call AeroAPI.",
            url=f"https://www.flightaware.com/live/airport/{icao}",
        ),
    ]
