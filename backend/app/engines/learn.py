"""Route learner. Every provider response teaches the database something.

Input: the complete priced itineraries we just saw, the user's A and B, the honest
A→B baseline, and which C values we paid to probe. Output: an append-only fare
history, flight edges, and (A, B, C) route statistics. Prices are never rewritten
and never summed from legs.
"""

from __future__ import annotations

from collections import defaultdict
from dataclasses import dataclass, field
from datetime import datetime, timezone
from statistics import median

from app.engines.hidden import detect_hidden_city, itinerary_fingerprint, ticketed_destination
from app.models import Offer

MAX_SAVINGS_KEPT = 200
MAX_DATES_KEPT = 60


@dataclass(slots=True)
class FareObservation:
    offer_uid: str
    fingerprint: str
    origin: str
    ticketed: str
    connections: list[str]
    stops: int
    carrier: str
    provider: str
    price: float
    currency: str
    date: str
    adults: int
    cabin: str
    fare_brand: str | None
    expires_at: str | None


@dataclass(slots=True)
class EdgeObservation:
    origin: str
    dest: str
    carrier: str
    flight_number: str
    count: int = 0
    travel_dates: set[str] = field(default_factory=set)


@dataclass(slots=True)
class StatDelta:
    origin: str
    intended: str
    ticketed: str
    observations: int = 0
    successful_connections: int = 0
    cheaper_than_direct_count: int = 0
    savings: list[float] = field(default_factory=list)
    saving_pcts: list[float] = field(default_factory=list)
    currency: str = ""
    best_through_price: float | None = None
    success: bool = False
    cheaper: bool = False


@dataclass
class LearnBatch:
    observed_at: datetime
    fares: list[FareObservation] = field(default_factory=list)
    edges: dict[tuple[str, str, str, str], EdgeObservation] = field(default_factory=dict)
    stats: dict[tuple[str, str, str], StatDelta] = field(default_factory=dict)

    def stat(self, origin: str, intended: str, ticketed: str) -> StatDelta:
        key = (origin.upper(), intended.upper(), ticketed.upper())
        row = self.stats.get(key)
        if row is None:
            row = StatDelta(origin=key[0], intended=key[1], ticketed=key[2])
            self.stats[key] = row
        return row


def build_batch(
    offers: list[Offer],
    *,
    origins: set[str],
    intended: set[str],
    honest_price: float | None,
    honest_currency: str | None,
    probed: list[str],
    date: str,
    adults: int,
    cabin: str,
    probe_origin: str | None = None,
    probe_intended: str | None = None,
    now: datetime | None = None,
) -> LearnBatch:
    """`origins`/`intended` accept member airports (city searches). `probe_*` are the
    tokens we actually shopped, used to record a paid check that found nothing."""
    now = now or datetime.now(timezone.utc)
    batch = LearnBatch(observed_at=now)
    origin_set = {o.upper()[:3] for o in origins}
    intended_set = {b.upper() for b in intended}
    probe_a = (probe_origin or sorted(origin_set)[0]).upper()[:3] if origin_set else ""
    probe_b = (probe_intended or sorted(intended_set)[0]).upper()[:3] if intended_set else ""
    probed_u = {p.upper() for p in probed}

    priced = [o for o in offers if o.price is not None and o.segments]
    for offer in priced:
        first, last = offer.segments[0], offer.segments[-1]
        if first.origin.upper() not in origin_set:
            continue
        batch.fares.append(
            FareObservation(
                offer_uid=offer.id,
                fingerprint=itinerary_fingerprint(offer),
                origin=first.origin.upper(),
                ticketed=last.dest.upper(),
                connections=[s.dest.upper() for s in offer.segments[:-1]],
                stops=offer.stops,
                carrier=(offer.carrier or "").upper(),
                provider=offer.source,
                price=float(offer.price or 0),
                currency=offer.currency,
                date=date,
                adults=adults,
                cabin=cabin,
                fare_brand=offer.fare_basis or None,
                expires_at=offer.expires_at,
            )
        )
        for seg in offer.segments:
            key = (seg.origin.upper(), seg.dest.upper(), (seg.carrier or "").upper(), seg.flight_number or "")
            edge = batch.edges.get(key)
            if edge is None:
                edge = EdgeObservation(origin=key[0], dest=key[1], carrier=key[2], flight_number=key[3])
                batch.edges[key] = edge
            edge.count += 1
            edge.travel_dates.add(date)

    # One "observation" per (A, C) response, not per offer, so a supplier returning
    # 40 variants of the same routing does not look like 40 independent successes.
    by_ticketed: dict[str, list[Offer]] = defaultdict(list)
    for offer in priced:
        if offer.segments[0].origin.upper() not in origin_set:
            continue
        c = ticketed_destination(offer)
        if c and c not in intended_set:
            by_ticketed[c].append(offer)

    # Every paid probe A→C is one check of the hypothesis "tickets to C route via B".
    probe_rows: set[tuple[str, str, str]] = set()
    if probe_a and probe_b:
        for c in probed_u:
            if c in intended_set or c in origin_set:
                continue
            row = batch.stat(probe_a, probe_b, c)
            row.observations += 1
            probe_rows.add((probe_a, probe_b, c))

    for c, group in by_ticketed.items():
        via_b: dict[tuple[str, str], list[Offer]] = defaultdict(list)
        other_connections: set[tuple[str, str]] = set()
        for offer in group:
            a = offer.segments[0].origin.upper()
            hit = detect_hidden_city(offer, intended_set)
            if hit:
                via_b[(a, hit.exit_airport)].append(offer)
            for s in offer.segments[:-1]:
                x = s.dest.upper()
                if x not in intended_set and x != a:
                    other_connections.add((a, x))
        for (a, b), hits in via_b.items():
            row = batch.stat(a, b, c)
            if (a, b, c) not in probe_rows:
                row.observations += 1
            row.successful_connections += 1
            row.success = True
            cheapest = min(hits, key=lambda o: o.price or 1e12)
            if cheapest.price is not None:
                if row.best_through_price is None or cheapest.price < row.best_through_price:
                    row.best_through_price = cheapest.price
            if (
                honest_price is not None
                and honest_currency
                and cheapest.price is not None
                and cheapest.currency == honest_currency
                and cheapest.price < honest_price
            ):
                saving = round(honest_price - cheapest.price, 2)
                row.cheaper_than_direct_count += 1
                row.cheaper = True
                row.currency = honest_currency
                row.savings.append(saving)
                row.saving_pcts.append(round(100.0 * saving / honest_price, 2))
        # A→X→C also teaches that X is a connection on tickets to C, even though we
        # hold no honest A→X price in this search.
        for a, x in other_connections:
            row = batch.stat(a, x, c)
            row.observations += 1
            row.successful_connections += 1
            row.success = True
    for row in batch.stats.values():
        row.observations = max(row.observations, row.successful_connections)
        row.successful_connections = max(row.successful_connections, row.cheaper_than_direct_count)
    return batch


def merge_savings(existing: list[float], new: list[float]) -> list[float]:
    merged = [*existing, *new]
    return merged[-MAX_SAVINGS_KEPT:]


def summarize_savings(savings: list[float]) -> tuple[float, float, float]:
    """(average, median, maximum) of the kept saving samples."""
    if not savings:
        return 0.0, 0.0, 0.0
    return (
        round(sum(savings) / len(savings), 2),
        round(float(median(savings)), 2),
        round(max(savings), 2),
    )
