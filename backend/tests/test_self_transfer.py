from app.engines.self_transfer import (
    bridge_pairs,
    combine_at_hubs,
    connection_minutes,
    pick_transfer_hubs,
    stitch_chain,
)
from app.models import Offer, Segment


def _seg(o: str, d: str, dep: str, arr: str, flight: str, carrier: str = "AK") -> Segment:
    return Segment(
        origin=o,
        dest=d,
        carrier=carrier,
        flight_number=flight,
        dep=dep,
        arr=arr,
        duration_min=120,
        rbd="Y",
    )


def _offer(oid: str, price: float, segs: list[Segment], carrier: str, currency: str = "USD") -> Offer:
    return Offer(
        id=oid,
        kind="connecting" if len(segs) > 1 else "nonstop",
        channel="ndc",
        source="duffel",
        segments=segs,
        price=price,
        currency=currency,
        cabin="ECONOMY",
        fare_basis="Y",
        carrier=carrier,
        duration_min=sum(s.duration_min for s in segs),
        stops=len(segs) - 1,
        first_flight=segs[0].flight_number,
        live=True,
    )


def test_pick_hubs_ranks_overlap_first():
    hubs = pick_transfer_hubs("BEY", "CBR", {"KUL", "DXB"}, {"KUL", "SYD", "DPS"}, limit=4)
    assert hubs[0] == "KUL"
    assert "BEY" not in hubs and "CBR" not in hubs


def test_one_sided_overlap_uses_the_graph_not_a_hub_list():
    hubs = pick_transfer_hubs(
        "BEY",
        "CBR",
        {"DXB", "DOH"},
        {"SYD", "MEL"},
        limit=4,
    )
    assert set(hubs) <= {"DXB", "DOH", "SYD", "MEL"}
    assert "BEY" not in hubs and "CBR" not in hubs


def test_extra_hubs_fill_when_overlap_is_thin():
    hubs = pick_transfer_hubs(
        "BEY",
        "CBR",
        {"DXB"},
        {"SYD"},
        limit=4,
        extra_hubs=["SIN", "FRA", "KUL"],
    )
    assert hubs[0] in {"DXB", "SYD"}
    assert "SIN" in hubs


def test_bridge_pairs_only_when_no_shared_hub():
    assert bridge_pairs(
        ["KUL", "SIN", "DPS", "SYD"],
        from_origin={"KUL", "SIN"},
        into_dest={"DPS", "SYD"},
    )[:2] == [("KUL", "DPS"), ("KUL", "SYD")]


def test_bridge_pairs_skip_when_two_tickets_already_work():
    assert bridge_pairs(["KUL", "DXB"], from_origin={"KUL"}, into_dest={"KUL", "SYD"}) == []
    assert bridge_pairs(["ORD", "DTW", "ATL"]) == []
    assert bridge_pairs(["DXB", "DOH"], from_origin={"DXB"}, into_dest={"DOH"}) == [("DXB", "DOH")]


def test_pick_hubs_zero_limit():
    assert pick_transfer_hubs("BEY", "CBR", {"KUL"}, {"SYD"}, limit=0) == []


def test_stitch_two_tickets_three_stops_like_kayak():
    left = _offer(
        "airasia",
        280,
        [
            _seg("BEY", "SHJ", "2026-11-19T12:15", "2026-11-19T16:20", "G9412", "G9"),
            _seg("SHJ", "KUL", "2026-11-19T21:25", "2026-11-20T08:40", "D7107", "AK"),
        ],
        "AK",
    )
    right = _offer(
        "virgin",
        437,
        [
            _seg("KUL", "DPS", "2026-11-20T15:35", "2026-11-20T18:30", "VA80", "VA"),
            _seg("DPS", "CBR", "2026-11-20T21:25", "2026-11-21T07:00", "VA81", "VA"),
        ],
        "VA",
    )
    joined = stitch_chain([left, right], "CBR")
    assert joined is not None
    assert joined.kind == "self-transfer"
    assert joined.price == 717
    assert joined.stops == 3
    assert joined.self_transfer_airports == ["KUL"]
    assert [s.dest for s in joined.segments[:-1]] == ["SHJ", "KUL", "DPS"]
    assert joined.segments[-1].dest == "CBR"
    assert len(joined.separate_tickets) == 2
    assert "not hidden-city" in (joined.note or "").lower()


def test_stitch_three_tickets_two_self_transfers():
    a = _offer("t1", 200, [_seg("BEY", "KUL", "2026-11-19T12:15", "2026-11-20T08:40", "AK1", "AK")], "AK")
    b = _offer("t2", 220, [_seg("KUL", "DPS", "2026-11-20T15:35", "2026-11-20T18:30", "AK2", "AK")], "AK")
    c = _offer("t3", 297, [_seg("DPS", "CBR", "2026-11-20T21:25", "2026-11-21T07:00", "VA81", "VA")], "VA")
    joined = stitch_chain([a, b, c], "CBR")
    assert joined is not None
    assert joined.price == 717
    assert joined.self_transfer_airports == ["KUL", "DPS"]
    assert joined.stops == 2


def test_stitch_rejects_tight_self_transfer():
    left = _offer("a", 100, [_seg("BEY", "KUL", "2026-11-19T08:00", "2026-11-19T20:00", "AK1")], "AK")
    right = _offer("b", 100, [_seg("KUL", "CBR", "2026-11-19T21:00", "2026-11-20T10:00", "VA1", "VA")], "VA")
    assert stitch_chain([left, right], "CBR") is None


def test_stitch_rejects_inbound_that_already_visits_b():
    left = _offer(
        "via-cbr",
        400,
        [
            _seg("BEY", "CBR", "2026-11-19T08:00", "2026-11-20T10:00", "QF1", "QF"),
            _seg("CBR", "SYD", "2026-11-20T12:00", "2026-11-20T13:00", "QF2", "QF"),
        ],
        "QF",
    )
    right = _offer("back", 50, [_seg("SYD", "CBR", "2026-11-20T18:00", "2026-11-20T19:30", "VA9", "VA")], "VA")
    assert stitch_chain([left, right], "CBR") is None


def test_stitch_rejects_mixed_currency():
    left = _offer("a", 100, [_seg("BEY", "KUL", "2026-11-19T08:00", "2026-11-19T20:00", "AK1")], "AK")
    right = _offer("b", 100, [_seg("KUL", "CBR", "2026-11-20T08:00", "2026-11-20T22:00", "VA1", "VA")], "VA", currency="EUR")
    assert stitch_chain([left, right], "CBR") is None


def test_stitch_rejects_cabin_mismatch():
    left = _offer("a", 100, [_seg("BEY", "KUL", "2026-11-19T08:00", "2026-11-19T20:00", "AK1")], "AK")
    right = _offer("b", 100, [_seg("KUL", "CBR", "2026-11-20T08:00", "2026-11-20T22:00", "VA1", "VA")], "VA")
    right.cabin = "BUSINESS"
    assert stitch_chain([left, right], "CBR") is None


def test_combine_keeps_cheapest():
    left = _offer("a", 100, [_seg("BEY", "KUL", "2026-11-19T08:00", "2026-11-19T20:00", "AK1")], "AK")
    cheap = _offer("b1", 200, [_seg("KUL", "CBR", "2026-11-20T08:00", "2026-11-20T22:00", "VA1", "VA")], "VA")
    dear = _offer("b2", 900, [_seg("KUL", "CBR", "2026-11-20T09:00", "2026-11-20T23:00", "VA2", "VA")], "VA")
    out = combine_at_hubs({"KUL": [left]}, {"KUL": [dear, cheap]}, {"CBR"})
    assert out[0].price == 300


def test_stitch_rejects_six_flights():
    left = _offer(
        "a",
        200,
        [
            _seg("BEY", "SHJ", "2026-11-19T12:15", "2026-11-19T16:20", "G91"),
            _seg("SHJ", "KUL", "2026-11-19T21:25", "2026-11-20T08:40", "D71"),
            _seg("KUL", "SIN", "2026-11-20T11:00", "2026-11-20T12:30", "AK3"),
        ],
        "AK",
    )
    right = _offer(
        "b",
        200,
        [
            _seg("SIN", "DPS", "2026-11-20T16:00", "2026-11-20T18:30", "VA1", "VA"),
            _seg("DPS", "SYD", "2026-11-20T21:25", "2026-11-21T04:00", "VA2", "VA"),
            _seg("SYD", "CBR", "2026-11-21T08:00", "2026-11-21T09:00", "VA3", "VA"),
        ],
        "VA",
    )
    assert stitch_chain([left, right], "CBR") is None


def test_combine_keeps_three_ticket_only_when_cheaper():
    left = _offer("a", 100, [_seg("BEY", "KUL", "2026-11-19T08:00", "2026-11-19T20:00", "AK1")], "AK")
    two_right = _offer("b", 200, [_seg("KUL", "CBR", "2026-11-20T08:00", "2026-11-20T22:00", "VA1", "VA")], "VA")
    mid = _offer("m", 50, [_seg("KUL", "DPS", "2026-11-20T08:00", "2026-11-20T12:00", "AK2")], "AK")
    dear_last = _offer("c", 400, [_seg("DPS", "CBR", "2026-11-20T16:00", "2026-11-21T06:00", "VA9", "VA")], "VA")
    out = combine_at_hubs(
        {"KUL": [left]},
        {"KUL": [two_right], "DPS": [dear_last]},
        {"CBR"},
        mids={("KUL", "DPS"): [mid]},
    )
    assert out
    assert all(len(o.separate_tickets) == 2 for o in out)
    cheap_last = _offer("c2", 80, [_seg("DPS", "CBR", "2026-11-20T16:00", "2026-11-21T06:00", "VA8", "VA")], "VA")
    cheaper = combine_at_hubs(
        {"KUL": [left]},
        {"KUL": [two_right], "DPS": [cheap_last]},
        {"CBR"},
        mids={("KUL", "DPS"): [mid]},
    )
    assert cheaper[0].price == 230
    assert cheaper[0].separate_tickets and len(cheaper[0].separate_tickets) == 3


def test_connection_minutes_missing_times():
    assert connection_minutes(None, "2026-11-20T08:00") is None  # type: ignore[arg-type]
    assert connection_minutes("", "2026-11-20T08:00") is None
