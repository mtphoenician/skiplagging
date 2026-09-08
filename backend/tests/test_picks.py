from app.engines.shop import (
    _dedupe_itineraries,
    _hidden_if_cheaper,
    _keep_on_route,
    _prefer_real_carriers,
    _via_b,
    layover_minutes,
    pick_honest,
)
from app.providers.duffel import _dur
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


def _offer(
    oid: str,
    price: float,
    segments: list[Segment],
    stops: int,
    currency: str = "USD",
    duration_min: int | None = None,
) -> Offer:
    return Offer(
        id=oid,
        kind="nonstop" if stops == 0 else "connecting",
        channel="ndc",
        source="duffel",
        segments=segments,
        price=price,
        currency=currency,
        cabin="ECONOMY",
        fare_basis="Y",
        carrier="BA",
        duration_min=duration_min if duration_min is not None else sum(s.duration_min for s in segments),
        stops=stops,
        first_flight=segments[0].flight_number,
    )


def _risk(**kwargs) -> RiskAssessment:
    return RiskAssessment(
        score=40,
        headline="test",
        one_way_only=True,
        carry_on_only=True,
        items=[],
        net_saving_estimate=80,
        expected_disruption_cost=0,
        expected_enforcement_cost=0,
        **kwargs,
    )


def _match(oid: str, through: Offer, local: Offer, saving: float) -> HiddenCityMatch:
    return HiddenCityMatch(
        id=oid,
        hidden_city=through.segments[-1].dest,
        hidden_city_name="Beyond",
        local_offer=local,
        through_offer=through,
        first_flight_match=True,
        gross_saving=saving,
        saving_pct=20,
        currency=through.currency,
        risk=_risk(),
    )


def test_pick_prefers_cheapest_nonstop_over_dearer_connecting():
    cheap = _offer("n1", 200, [_seg("LHR", "JFK", "2026-10-22T08:00", "2026-10-22T11:00")], 0)
    dear = _offer("n2", 400, [_seg("LHR", "JFK", "2026-10-22T09:00", "2026-10-22T12:00")], 0)
    connect = _offer(
        "c1",
        250,
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


def test_pick_prefers_cheaper_connecting_over_nonstop():
    nonstop = _offer("n1", 400, [_seg("LHR", "JFK", "2026-10-22T08:00", "2026-10-22T11:00")], 0)
    connect = _offer(
        "c1",
        180,
        [
            _seg("LHR", "DUB", "2026-10-22T08:00", "2026-10-22T09:00", "BA2"),
            _seg("DUB", "JFK", "2026-10-22T10:00", "2026-10-22T13:00", "BA3"),
        ],
        1,
    )
    pick = pick_honest([nonstop], [connect])
    assert pick is not None
    assert pick.kind == "connecting"
    assert pick.offer.id == "c1"


def test_pick_uses_price_then_shortest_layover():
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
    assert pick.offer.id == "c1"
    assert pick.layover_min == 300


def test_pick_same_price_prefers_shorter_layover():
    long_wait = _offer(
        "c1",
        200,
        [
            _seg("LHR", "DUB", "2026-10-22T08:00", "2026-10-22T09:00", "BA2"),
            _seg("DUB", "JFK", "2026-10-22T14:00", "2026-10-22T17:00", "BA3"),
        ],
        1,
    )
    short_wait = _offer(
        "c2",
        200,
        [
            _seg("LHR", "AMS", "2026-10-22T08:00", "2026-10-22T09:00", "BA4"),
            _seg("AMS", "JFK", "2026-10-22T10:00", "2026-10-22T13:00", "BA5"),
        ],
        1,
    )
    pick = pick_honest([], [long_wait, short_wait])
    assert pick is not None
    assert pick.offer.id == "c2"
    assert pick.layover_min == 60


def test_pick_ignores_other_currency_amounts():
    usd = _offer("n1", 400, [_seg("LHR", "JFK", "2026-10-22T08:00", "2026-10-22T11:00")], 0)
    gbp = _offer("n2", 80, [_seg("LHR", "JFK", "2026-10-22T09:00", "2026-10-22T12:00")], 0, currency="GBP")
    pick = pick_honest([usd, gbp], [])
    assert pick is not None
    assert pick.offer.currency == "USD"
    assert pick.offer.id == "n1"


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
    cheap_match = _match("m1", cheap_through, honest.offer, 80)
    dear_match = _match("m2", dear_through, honest.offer, -50)
    assert _hidden_if_cheaper([cheap_match], honest) is cheap_match
    assert _hidden_if_cheaper([dear_match], honest) is None
    assert pick_honest([], []) is None


def test_hidden_not_shown_when_currency_differs():
    honest = pick_honest(
        [_offer("n1", 300, [_seg("LHR", "BOS", "2026-10-22T08:00", "2026-10-22T11:00")], 0)],
        [],
    )
    gbp_through = _offer(
        "h1",
        80,
        [
            _seg("LHR", "BOS", "2026-10-22T08:00", "2026-10-22T11:00", "BA9"),
            _seg("BOS", "JFK", "2026-10-22T12:00", "2026-10-22T13:00", "BA10"),
        ],
        1,
        currency="GBP",
    )
    assert _hidden_if_cheaper([_match("m1", gbp_through, honest.offer, 220)], honest) is None


def test_via_b_requires_first_sector_to_true_dest():
    through = _offer(
        "h1",
        220,
        [
            _seg("LHR", "BOS", "2026-10-22T08:00", "2026-10-22T11:00", "BA9"),
            _seg("BOS", "JFK", "2026-10-22T12:00", "2026-10-22T13:00", "BA10"),
        ],
        1,
    )
    assert _via_b(through, "LHR", "BOS", "JFK")
    assert _via_b(through, {"LHR", "LGW"}, {"BOS", "BDL"}, "JFK")
    assert not _via_b(through, "LHR", "JFK", "BOS")
    assert not _via_b(through, "LGW", "BOS", "JFK")
    nonstop = _offer("n1", 300, [_seg("LHR", "JFK", "2026-10-22T08:00", "2026-10-22T11:00")], 0)
    assert not _via_b(nonstop, "LHR", "BOS", "JFK")


def test_keep_on_route_drops_other_airports():
    paris_london = _offer("n1", 110, [_seg("CDG", "LGW", "2026-09-19T10:00", "2026-09-19T10:55")], 0)
    junk = _offer("n2", 41, [_seg("VIY", "SEN", "2026-09-19T13:56", "2026-09-19T13:57")], 0)
    kept = _keep_on_route([paris_london, junk], {"CDG", "ORY", "PAR"}, {"LGW", "LHR", "LON"})
    assert [o.id for o in kept] == ["n1"]


def test_codeshare_keeps_one_itinerary():
    ib = _offer("ib", 41, [_seg("CDG", "LHR", "2026-09-19T13:56", "2026-09-19T14:56", "IB1")], 0)
    ba = ib.model_copy(update={"id": "ba", "price": 42, "carrier": "BA", "first_flight": "BA1"})
    ba.segments[0] = ba.segments[0].model_copy(update={"flight_number": "BA1", "carrier": "BA"})
    kept = _dedupe_itineraries([ib, ba])
    assert len(kept) == 1
    assert kept[0].id == "ib"


def test_prefer_real_carriers_skips_duffel_airways():
    zz = _offer("zz", 41, [_seg("CDG", "LHR", "2026-09-19T13:56", "2026-09-19T14:56", "ZZ1")], 0)
    zz = zz.model_copy(update={"carrier": "ZZ"})
    ba = _offer("ba", 110, [_seg("CDG", "LHR", "2026-09-19T10:00", "2026-09-19T11:00", "BA1")], 0)
    kept = _prefer_real_carriers([zz, ba])
    assert [o.id for o in kept] == ["ba"]


def test_duffel_duration_parses_overnight():
    assert _dur("PT2H30M") == 150
    assert _dur("P1DT30M") == 1470
    assert _dur("P1DT2H30M") == 1590
    assert _dur("") == 0
