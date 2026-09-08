"""CandidateGenerator, learner and budget: the search intelligence we own."""

from __future__ import annotations

from datetime import datetime, timedelta, timezone

from app.engines.budget import Ledger, provider_score, timed_shop, start_ledger
from app.engines.candidates import (
    DEAD_AFTER_CHECKS,
    WEIGHTS,
    Candidate,
    RouteStat,
    expected_value,
    freshness,
    hub_probabilities,
    is_dead,
    rank_candidates,
    score_candidate,
    select_candidates,
)
from app.engines.learn import build_batch, merge_savings, summarize_savings
from app.models import Offer, Segment

NOW = datetime(2026, 9, 8, 12, 0, tzinfo=timezone.utc)


def _seg(o: str, d: str, flight: str, carrier: str = "UA") -> Segment:
    return Segment(
        origin=o,
        dest=d,
        carrier=carrier,
        flight_number=flight,
        dep="2026-10-10T08:00",
        arr="2026-10-10T10:00",
        duration_min=120,
        rbd="Y",
    )


def _offer(oid: str, price: float, segs: list[Segment], currency: str = "USD", source: str = "duffel") -> Offer:
    return Offer(
        id=oid,
        kind="nonstop" if len(segs) == 1 else "connecting",
        channel="ndc",
        source=source,
        segments=segs,
        price=price,
        currency=currency,
        cabin="ECONOMY",
        fare_basis="Y",
        carrier=segs[0].carrier,
        duration_min=sum(s.duration_min for s in segs),
        stops=len(segs) - 1,
        first_flight=segs[0].flight_number,
        expires_at="2026-09-08T12:30:00Z",
    )


# ── scoring ────────────────────────────────────────────────────────────────────


def test_weights_sum_to_one():
    assert abs(sum(WEIGHTS.values()) - 1.0) < 1e-9


def test_score_formula_matches_spec_weights():
    stat = RouteStat(
        "JFK",
        "ORD",
        "DEN",
        observations=100,
        successful_connections=71,
        cheaper_than_direct_count=50,
        median_saving=87,
        average_saving_percent=30,
        last_cheaper_at=NOW,
    )
    score, parts = score_candidate(stat, hub_probability=1.0, provider_rate=0.9, now=NOW)
    assert abs(parts["connection"] - (71.5 / 101)) < 1e-4
    assert abs(parts["savings_probability"] - (50.5 / 101)) < 1e-4
    assert parts["expected_savings"] == 0.3
    assert parts["freshness"] == 1.0
    expected = (
        0.30 * parts["connection"]
        + 0.25 * parts["savings_probability"]
        + 0.20 * 0.3
        + 0.10 * 1.0
        + 0.10 * 1.0
        + 0.05 * 0.9
    )
    assert abs(score - expected) < 1e-3


def test_freshness_halves_every_week():
    assert freshness(None, NOW) == 0.0
    assert freshness(NOW, NOW) == 1.0
    assert abs(freshness(NOW - timedelta(days=7), NOW) - 0.5) < 1e-6
    assert abs(freshness(NOW - timedelta(days=14), NOW) - 0.25) < 1e-6


def test_one_lucky_hit_is_not_certainty():
    one = RouteStat("JFK", "ORD", "DEN", observations=1, successful_connections=1, cheaper_than_direct_count=1)
    many = RouteStat("JFK", "ORD", "SEA", observations=50, successful_connections=45, cheaper_than_direct_count=30)
    s1, p1 = score_candidate(one, 0.0, 0.5, NOW)
    s2, p2 = score_candidate(many, 0.0, 0.5, NOW)
    assert p1["connection"] < 0.8 < p2["connection"]
    assert s2 > s1


def test_dead_candidate_is_dropped_after_repeated_misses():
    bos = RouteStat("JFK", "ORD", "BOS", observations=DEAD_AFTER_CHECKS, successful_connections=0)
    assert is_dead(bos)
    assert not is_dead(RouteStat("JFK", "ORD", "MIA", observations=2, successful_connections=0))
    ranked = rank_candidates(
        "JFK",
        {"ORD"},
        {"BOS": bos},
        {"BOS": "hub", "DEN": "hub"},
        hub_prob={"BOS": 1.0, "DEN": 1.0},
        provider_rate={},
        now=NOW,
    )
    assert [c.code for c in ranked] == ["DEN"]


def test_ranking_prefers_learned_history_over_hub_list():
    stats = {
        "DEN": RouteStat("JFK", "ORD", "DEN", 20, 18, 12, median_saving=71, average_saving_percent=28, last_cheaper_at=NOW),
        "SEA": RouteStat("JFK", "ORD", "SEA", 20, 14, 7, median_saving=112, average_saving_percent=35, last_cheaper_at=NOW - timedelta(days=3)),
        "MIA": RouteStat("JFK", "ORD", "MIA", 20, 1, 0),
    }
    pool = {"PHX": "hub", "BOS": "openflights", "DEN": "hub"}
    ranked = rank_candidates(
        "JFK", {"ORD"}, stats, pool, hub_probabilities({"ORD"}, {"ORD": {"BOS"}}), {"DEN": 0.9}, now=NOW
    )
    codes = [c.code for c in ranked]
    assert codes[:2] == ["DEN", "SEA"]
    assert codes.index("PHX") < codes.index("MIA")  # unknown hub beats a known dud
    assert codes.index("BOS") > codes.index("PHX")  # route-map hint is weaker than the hub list
    assert all(c.code not in {"JFK", "ORD"} for c in ranked)


def test_selection_threshold_and_cold_start():
    hot = Candidate("DEN", 0.81, "stats", observations=20)
    warm = Candidate("SEA", 0.52, "stats", observations=20)
    cold_a = Candidate("PHX", 0.125, "hub")
    cold_b = Candidate("LAS", 0.125, "hub")
    cold_c = Candidate("SFO", 0.125, "hub")
    ranked = [hot, warm, cold_a, cold_b, cold_c]
    chosen = select_candidates(ranked, budget=8, min_score=0.35)
    assert [c.code for c in chosen] == ["DEN", "SEA", "PHX"]  # 2 above threshold, filled to 3
    assert hot.selected and warm.selected and cold_a.selected and not cold_b.selected

    # Nothing learned yet: still probe three hubs so the database starts learning.
    cold = [Candidate(c, 0.125, "hub") for c in ("DEN", "SEA", "SFO", "LAX")]
    assert [c.code for c in select_candidates(cold, budget=8)] == ["DEN", "SEA", "SFO"]
    # Deep pass excludes what the fast pass already paid for.
    assert [c.code for c in select_candidates(cold, budget=8, exclude={"DEN", "SEA", "SFO"})] == ["LAX"]
    assert select_candidates(cold, budget=0) == []


def test_expected_value_per_paid_request():
    assert expected_value(0.65, 0.025, 12) == 0.195
    assert expected_value(0.01, 0.01, 10) == 0.001
    assert expected_value(0.65, 0.025, 12) > 0.005 > expected_value(0.01, 0.01, 10)


# ── learning ───────────────────────────────────────────────────────────────────


def _jfk_ord_batch(probed: list[str]):
    honest = _offer("aa", 240, [_seg("JFK", "ORD", "AA100", "AA")])
    den = _offer("den", 170, [_seg("JFK", "ORD", "UA200"), _seg("ORD", "DEN", "UA300")])
    sea = _offer("sea", 300, [_seg("JFK", "ORD", "AS400", "AS"), _seg("ORD", "SEA", "AS500", "AS")])
    lax = _offer("lax", 150, [_seg("JFK", "DFW", "AA600", "AA"), _seg("DFW", "LAX", "AA700", "AA")])
    return build_batch(
        [honest, den, sea, lax],
        origins={"JFK"},
        intended={"ORD"},
        honest_price=240,
        honest_currency="USD",
        probed=probed,
        date="2026-10-10",
        adults=1,
        cabin="ECONOMY",
        now=NOW,
    )


def test_learner_records_every_fare_and_flight_edge():
    batch = _jfk_ord_batch(probed=["DEN", "SEA", "LAX", "PHX"])
    assert {f.offer_uid for f in batch.fares} == {"aa", "den", "sea", "lax"}
    den = next(f for f in batch.fares if f.offer_uid == "den")
    assert den.origin == "JFK" and den.ticketed == "DEN" and den.connections == ["ORD"]
    assert den.price == 170 and den.expires_at == "2026-09-08T12:30:00Z"
    assert ("JFK", "ORD", "UA", "UA200") in batch.edges
    assert ("ORD", "DEN", "UA", "UA300") in batch.edges
    assert batch.edges[("JFK", "ORD", "UA", "UA200")].travel_dates == {"2026-10-10"}


def test_learner_scores_probes_success_and_savings():
    batch = _jfk_ord_batch(probed=["DEN", "SEA", "LAX", "PHX"])
    den = batch.stats[("JFK", "ORD", "DEN")]
    assert den.observations == 1 and den.successful_connections == 1 and den.cheaper_than_direct_count == 1
    assert den.savings == [70.0] and den.saving_pcts == [round(100 * 70 / 240, 2)]
    assert den.best_through_price == 170
    sea = batch.stats[("JFK", "ORD", "SEA")]
    assert sea.observations == 1 and sea.successful_connections == 1 and sea.cheaper_than_direct_count == 0
    # JFK→DFW→LAX misses ORD: a paid check that taught us LAX does not route via ORD…
    lax = batch.stats[("JFK", "ORD", "LAX")]
    assert lax.observations == 1 and lax.successful_connections == 0
    # …but does teach that DFW is a connection on tickets to LAX.
    assert batch.stats[("JFK", "DFW", "LAX")].successful_connections == 1
    # PHX returned nothing: still one observation, no success (negative learning).
    phx = batch.stats[("JFK", "ORD", "PHX")]
    assert phx.observations == 1 and phx.successful_connections == 0
    # Never invent a shorter ticket: no row for the honest A→B itself.
    assert all(key[2] != "ORD" for key in batch.stats)


def test_learner_counts_index_hits_without_double_counting_probes():
    batch = _jfk_ord_batch(probed=[])  # everything came from the index, nothing paid
    den = batch.stats[("JFK", "ORD", "DEN")]
    assert den.observations == 1 and den.successful_connections == 1


def test_learner_ignores_other_currency_and_offers_from_other_origins():
    honest = _offer("aa", 240, [_seg("JFK", "ORD", "AA100", "AA")])
    gbp = _offer("gbp", 100, [_seg("JFK", "ORD", "BA1", "BA"), _seg("ORD", "DEN", "BA2", "BA")], currency="GBP")
    other = _offer("ewr", 90, [_seg("EWR", "ORD", "UA9"), _seg("ORD", "DEN", "UA10")])
    batch = build_batch(
        [honest, gbp, other],
        origins={"JFK"},
        intended={"ORD"},
        honest_price=240,
        honest_currency="USD",
        probed=["DEN"],
        date="2026-10-10",
        adults=1,
        cabin="ECONOMY",
        now=NOW,
    )
    den = batch.stats[("JFK", "ORD", "DEN")]
    assert den.successful_connections == 1 and den.cheaper_than_direct_count == 0  # £100 is not "cheaper" than $240
    assert ("JFK", "ORD", "DEN") in batch.stats and ("EWR", "ORD", "DEN") not in batch.stats
    assert {f.offer_uid for f in batch.fares} == {"aa", "gbp"}


def test_savings_summary_keeps_history_bounded():
    kept = merge_savings(list(range(300)), [1000.0])
    assert len(kept) == 200 and kept[-1] == 1000.0
    assert summarize_savings([70, 90, 110]) == (90.0, 90.0, 110.0)
    assert summarize_savings([]) == (0.0, 0.0, 0.0)


# ── budget ─────────────────────────────────────────────────────────────────────


async def test_ledger_records_paid_calls_and_free_mock(anyio_backend="asyncio"):
    ledger = start_ledger(0.005)

    async def ok():
        return [_offer("x", 1, [_seg("JFK", "ORD", "AA1", "AA")])]

    async def boom():
        raise RuntimeError("supplier down")

    got = await timed_shop("duffel", "JFK", "DEN", "2026-10-10", "expand", ok())
    assert len(got) == 1
    assert await timed_shop("duffel", "JFK", "SEA", "2026-10-10", "expand", boom()) == []
    assert await timed_shop("mock", "JFK", "ORD", "2026-10-10", "direct", ok())
    assert len(ledger.calls) == 3
    assert len(ledger.paid()) == 2
    assert ledger.total_cost == 0.01
    failed = next(c for c in ledger.calls if c.dest == "SEA")
    assert failed.ok is False and failed.offers == 0


def test_provider_score_rewards_hits_and_penalises_latency():
    assert provider_score(0, 0, 0) == 0.5
    assert provider_score(9, 10, 800) > provider_score(5, 10, 800)
    assert provider_score(9, 10, 800) > provider_score(9, 10, 2500)
    assert Ledger().total_cost == 0
