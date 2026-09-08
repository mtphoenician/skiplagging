"""CandidateGenerator: which ticketed destinations C are worth a paid shop for A→B?

The naive plan shops A→every airport. We rank C by what our own database has
learned and shop only the top few. OpenFlights B→C spokes are a cold-start hint,
not a schedule; observed A→B→C tickets outweigh them as soon as we have any.

candidateScore =
      0.30 × historical_connection_probability   (A→C tickets that route via B)
    + 0.25 × historical_savings_probability      (…and undercut the honest A→B)
    + 0.20 × expected_savings                    (median saving % of A→B, clipped)
    + 0.10 × freshness                           (how recently that happened)
    + 0.10 × carrier_hub_probability             (OpenFlights published B→C)
    + 0.05 × provider_success_rate               (our suppliers actually answer A→C)
"""

from __future__ import annotations

from collections import defaultdict
from dataclasses import dataclass, field
from datetime import datetime, timezone

WEIGHTS = {
    "connection": 0.30,
    "savings_probability": 0.25,
    "expected_savings": 0.20,
    "freshness": 0.10,
    "hub": 0.10,
    "provider": 0.05,
}
DEFAULT_MIN_SCORE = 0.35
COLD_START_PROBES = 3
DEAD_AFTER_CHECKS = 5  # checked this many times, never routed via B → stop paying for it
FRESHNESS_HALF_LIFE_DAYS = 7.0


@dataclass(slots=True)
class RouteStat:
    """One hidden_city_route_stats row, provider-neutral."""

    origin: str
    intended: str
    ticketed: str
    observations: int = 0
    successful_connections: int = 0
    cheaper_than_direct_count: int = 0
    median_saving: float = 0.0
    average_saving_percent: float = 0.0
    last_success_at: datetime | None = None
    last_cheaper_at: datetime | None = None


@dataclass(slots=True)
class Candidate:
    code: str
    score: float
    source: str
    parts: dict[str, float] = field(default_factory=dict)
    observations: int = 0
    successful_connections: int = 0
    cheaper_than_direct_count: int = 0
    median_saving: float = 0.0
    selected: bool = False


def _smooth(successes: int, trials: int) -> float:
    """Laplace-style rate so one lucky hit is not a 100% probability."""
    if trials <= 0:
        return 0.0
    return (successes + 0.5) / (trials + 1.0)


def freshness(last: datetime | None, now: datetime) -> float:
    if last is None:
        return 0.0
    age_days = max((now - last).total_seconds(), 0.0) / 86400.0
    return 0.5 ** (age_days / FRESHNESS_HALF_LIFE_DAYS)


def score_candidate(
    stat: RouteStat | None,
    hub_probability: float,
    provider_rate: float,
    now: datetime,
) -> tuple[float, dict[str, float]]:
    if stat is None:
        parts = {
            "connection": 0.0,
            "savings_probability": 0.0,
            "expected_savings": 0.0,
            "freshness": 0.0,
            "hub": hub_probability,
            "provider": provider_rate,
        }
    else:
        parts = {
            "connection": _smooth(stat.successful_connections, stat.observations),
            "savings_probability": _smooth(stat.cheaper_than_direct_count, stat.observations),
            "expected_savings": max(0.0, min(stat.average_saving_percent / 100.0, 1.0)),
            "freshness": freshness(stat.last_cheaper_at or stat.last_success_at, now),
            "hub": hub_probability,
            "provider": provider_rate,
        }
    score = sum(WEIGHTS[k] * v for k, v in parts.items())
    return round(score, 4), {k: round(v, 4) for k, v in parts.items()}


def is_dead(stat: RouteStat | None) -> bool:
    return bool(stat and stat.observations >= DEAD_AFTER_CHECKS and stat.successful_connections == 0)


def hub_probabilities(intended: list[str] | set[str], route_map: dict[str, set[str]] | None = None) -> dict[str, float]:
    """1.0 when OpenFlights published a historical B→C nonstop; else 0. Not a schedule."""
    out: dict[str, float] = defaultdict(float)
    for b in intended:
        for c in (route_map or {}).get(b.upper(), set()):
            out[c.upper()] = 1.0
    return dict(out)


def rank_candidates(
    origin: str,
    intended: list[str] | set[str],
    stats: dict[str, RouteStat],
    pool: dict[str, str],
    hub_prob: dict[str, float],
    provider_rate: dict[str, float],
    now: datetime | None = None,
) -> list[Candidate]:
    """Score every C in `pool` (code → where it came from) plus every C we hold stats for."""
    now = now or datetime.now(timezone.utc)
    origin_u = origin.upper()
    excluded = {origin_u, *(b.upper() for b in intended)}
    codes: dict[str, str] = {}
    for code, source in pool.items():
        c = code.upper()
        if len(c) == 3 and c not in excluded:
            codes.setdefault(c, source)
    for c in stats:
        if len(c) == 3 and c not in excluded:
            codes.setdefault(c.upper(), "stats")
    out: list[Candidate] = []
    for code, source in codes.items():
        stat = stats.get(code)
        if is_dead(stat):
            continue
        score, parts = score_candidate(
            stat, hub_prob.get(code, 0.0), provider_rate.get(code, 0.5), now
        )
        out.append(
            Candidate(
                code=code,
                score=score,
                source="stats" if stat and stat.observations else source,
                parts=parts,
                observations=stat.observations if stat else 0,
                successful_connections=stat.successful_connections if stat else 0,
                cheaper_than_direct_count=stat.cheaper_than_direct_count if stat else 0,
                median_saving=stat.median_saving if stat else 0.0,
            )
        )
    out.sort(key=lambda c: (-c.score, -c.observations, c.code))
    return out


def select_candidates(
    ranked: list[Candidate],
    budget: int,
    min_score: float = DEFAULT_MIN_SCORE,
    cold_start: int = COLD_START_PROBES,
    exclude: set[str] | None = None,
) -> list[Candidate]:
    """Top-N above the threshold; on a cold start still probe a few hubs so we learn."""
    if budget <= 0:
        return []
    skip = {c.upper() for c in (exclude or set())}
    usable = [c for c in ranked if c.code not in skip]
    chosen = [c for c in usable if c.score >= min_score][:budget]
    if len(chosen) < min(cold_start, budget):
        picked = {c.code for c in chosen}
        for c in usable:
            if len(chosen) >= min(cold_start, budget):
                break
            if c.code not in picked:
                chosen.append(c)
                picked.add(c.code)
    for c in ranked:
        c.selected = c in chosen
    return chosen


def expected_value(p_useful: float, p_purchase: float, commission: float) -> float:
    """EV of one more paid request. Compare with settings.search_cost_usd."""
    return round(max(p_useful, 0.0) * max(p_purchase, 0.0) * max(commission, 0.0), 4)


# ── legacy in-process graph (kept for the current-process debug view) ──────────


@dataclass
class HiddenCityCandidateEdge:
    origin: str
    connection: str
    ticketed_destination: str
    observation_count: int = 0
    last_observed_at: str = ""
    best_observed_price: float | None = None


_GRAPH: dict[tuple[str, str, str], HiddenCityCandidateEdge] = {}


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
    """Cold-start order with no database: observed this process, then caller extras."""
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
