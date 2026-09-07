from app.engines.shop import _hidden_if_cheaper, layover_minutes, pick_honest
from app.models import HiddenCityMatch, Offer, RiskAssessment, Segment


def _seg(origin: str, dest: str, dep: str, arr: str, flight: str = "BA1") -> Segment:
    return Segment(
        origin=origin,
        dest=dest,
        carrier="BA",
        flight_number=flight,
        dep=dep,
        arr=arr,
        duration_min=60,
        rbd="Y",
    )


def _offer(oid: str, price: float, segments: list[Segment], stops: int) -> Offer:
    return Offer(
        id=oid,
        kind="nonstop" if stops == 0 else "connecting",
        channel="ndc",
        source="duffel",
        segments=segments,
        price=price,
        currency="USD",
        cabin="ECONOMY",
        fare_basis="Y",
        carrier="BA",
        duration_min=sum(s.duration_min for s in segments),
        stops=stops,
        first_flight=segments[0].flight_number,
    )


def test_pick_prefers_cheapest_nonstop():
    cheap = _offer("n1", 200, [_seg("LHR", "JFK", "2026-10-22T08:00", "2026-10-22T11:00")], 0)
    dear = _offer("n2", 400, [_seg("LHR", "JFK", "2026-10-22T09:00", "2026-10-22T12:00")], 0)
    connect = _offer(
        "c1",
        150,
        [
            _seg("LHR", "DUB", "2026-10-22T08:00", "2026-10-22T09:00", "BA2"),
            _seg("DUB", "JFK", "2026-10-22T10:00", "2026-10-22T13:00", "BA3"),
        ],
        1,
    )
    pick = pick_honest([dear, cheap], [connect])
    assert pick is not None
    assert pick.kind == "nonstop"
    assert pick.offer.id == "n1"
    assert pick.reason == "Cheapest nonstop"


def test_pick_uses_shortest_layover_when_no_nonstop():
    long_wait = _offer(
        "c1",
        180,
        [
            _seg("LHR", "DUB", "2026-10-22T08:00", "2026-10-22T09:00", "BA2"),
            _seg("DUB", "JFK", "2026-10-22T14:00", "2026-10-22T17:00", "BA3"),
        ],
        1,
    )
    short_wait = _offer(
        "c2",
        220,
        [
            _seg("LHR", "AMS", "2026-10-22T08:00", "2026-10-22T09:00", "BA4"),
            _seg("AMS", "JFK", "2026-10-22T10:00", "2026-10-22T13:00", "BA5"),
        ],
        1,
    )
    assert layover_minutes(long_wait) == 300
    assert layover_minutes(short_wait) == 60
    pick = pick_honest([], [long_wait, short_wait])
    assert pick is not None
    assert pick.kind == "connecting"
    assert pick.offer.id == "c2"
    assert pick.layover_min == 60


def test_hidden_only_when_cheaper_than_honest_pick():
    honest = pick_honest(
        [_offer("n1", 300, [_seg("LHR", "BOS", "2026-10-22T08:00", "2026-10-22T11:00")], 0)],
        [],
    )
    cheap_through = _offer(
        "h1",
        220,
        [
            _seg("LHR", "BOS", "2026-10-22T08:00", "2026-10-22T11:00", "BA9"),
            _seg("BOS", "JFK", "2026-10-22T12:00", "2026-10-22T13:00", "BA10"),
        ],
        1,
    )
    dear_through = cheap_through.model_copy(update={"id": "h2", "price": 350})
    risk = RiskAssessment(
        score=40,
        headline="test",
        one_way_only=True,
        carry_on_only=True,
        items=[],
        net_saving_estimate=80,
        expected_disruption_cost=0,
        expected_enforcement_cost=0,
    )
    cheap_match = HiddenCityMatch(
        id="m1",
        hidden_city="JFK",
        hidden_city_name="New York (JFK)",
        local_offer=honest.offer,
        through_offer=cheap_through,
        first_flight_match=True,
        gross_saving=80,
        saving_pct=26,
        currency="USD",
        risk=risk,
    )
    dear_match = cheap_match.model_copy(update={"id": "m2", "through_offer": dear_through, "gross_saving": -50})
    assert _hidden_if_cheaper([cheap_match], honest) is cheap_match
    assert _hidden_if_cheaper([dear_match], honest) is None
    assert pick_honest([], []) is None
