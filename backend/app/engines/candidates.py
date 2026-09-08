"""Likely ticketed destinations beyond the user's intended stop.

Bootstrap hubs are not a global schedule. Observed A→B→C offers should
outweigh this list once the shop has seen them.
"""

from __future__ import annotations

from collections import defaultdict
from dataclasses import dataclass
from datetime import datetime, timezone

HUB_ONWARD: dict[str, list[str]] = {
    "ORD": ["DEN", "SEA", "SFO", "LAX", "PHX", "LAS", "MSP", "MCI", "OMA"],
    "DFW": ["LAX", "SFO", "PHX", "LAS", "AUS", "SAT", "DEN"],
    "ATL": ["MCO", "TPA", "FLL", "MIA", "MSY", "DFW", "IAH"],
    "CLT": ["ATL", "MIA", "TPA", "ORD", "DFW"],
    "DEN": ["SEA", "SFO", "LAX", "PHX", "ORD"],
    "LHR": ["JFK", "EWR", "BOS", "ORD", "DXB", "SIN"],
    "CDG": ["JFK", "BOS", "ATL", "DXB"],
    "FRA": ["JFK", "ORD", "SFO", "LAX"],
    "AMS": ["JFK", "BOS", "ORD"],
}


@dataclass
class HiddenCityCandidateEdge:
    origin: str
    connection: str
    ticketed_destination: str
    observation_count: int = 0
    last_observed_at: str = ""
    best_observed_price: float | None = None


_GRAPH: dict[tuple[str, str, str], HiddenCityCandidateEdge] = {}
_SEGMENT_EDGES: dict[tuple[str, str], int] = defaultdict(int)


def record_offer(origin: str, connection: str, ticketed: str, price: float | None) -> None:
    key = (origin.upper(), connection.upper(), ticketed.upper())
    now = datetime.now(timezone.utc).isoformat(timespec="seconds")
    edge = _GRAPH.get(key)
    if edge is None:
        _GRAPH[key] = HiddenCityCandidateEdge(
            origin=key[0],
            connection=key[1],
            ticketed_destination=key[2],
            observation_count=1,
            last_observed_at=now,
            best_observed_price=price,
        )
    else:
        edge.observation_count += 1
        edge.last_observed_at = now
        if price is not None and (edge.best_observed_price is None or price < edge.best_observed_price):
            edge.best_observed_price = price
    _SEGMENT_EDGES[(key[0], key[1])] += 1
    _SEGMENT_EDGES[(key[1], key[2])] += 1


def graph_snapshot() -> list[dict]:
    return [
        {
            "origin": e.origin,
            "connection": e.connection,
            "ticketed_destination": e.ticketed_destination,
            "observation_count": e.observation_count,
            "last_observed_at": e.last_observed_at,
            "best_observed_price": e.best_observed_price,
        }
        for e in sorted(_GRAPH.values(), key=lambda x: (-x.observation_count, x.origin))
    ]


def candidates_beyond(
    origin: str,
    intended: list[str] | set[str],
    limit: int,
    extra: list[str] | None = None,
) -> list[str]:
    """Order: observed graph, then extra (OpenFlights/Amadeus), then hub fixtures."""
    intended_u = {code.upper() for code in intended}
    origin_u = origin.upper()
    seen = {origin_u, *intended_u}
    ranked: list[str] = []

    def push(codes: list[str]) -> None:
        for code in codes:
            code = code.upper()
            if len(code) == 3 and code not in seen:
                seen.add(code)
                ranked.append(code)

    observed = [
        e.ticketed_destination
        for e in sorted(_GRAPH.values(), key=lambda x: -x.observation_count)
        if e.origin == origin_u and e.connection in intended_u
    ]
    push(observed)
    push(list(extra or []))
    for dest in intended:
        push(HUB_ONWARD.get(dest.upper(), []))
    return ranked[:limit]


def plan_expansion(
    origin: str,
    intended: list[str] | set[str],
    limit: int,
    extra: list[str] | None = None,
    reused: list | None = None,
    already_hidden: bool = False,
) -> tuple[list[str], list[str]]:
    """Destinations still worth shopping, plus ticketed C values already indexed through B."""
    from app.engines.index import ticketed_dests_through

    dests = {code.upper() for code in intended}
    covered = ticketed_dests_through(reused or [], dests)
    skipped = sorted(covered)
    if already_hidden or covered or limit <= 0:
        return [], skipped
    ranked = candidates_beyond(origin, intended, limit, extra=extra)
    return ranked[:limit], skipped
