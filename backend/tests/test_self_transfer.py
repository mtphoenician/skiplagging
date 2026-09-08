from app.engines.self_transfer import (
    bridge_pairs,
    combine_at_hubs,
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


def test_oceania_dest_keeps_asian_hubs_not_only_gulf():
    hubs = pick_transfer_hubs(
        "BEY",
        "CBR",
        {"DXB", "DOH", "AUH", "SHJ", "IST"},
        {"SYD", "MEL"},
        limit=4,
    )
    assert "KUL" in hubs
    assert hubs[:3] == ["KUL", "SIN", "DPS"]


def test_bridge_pairs_for_oceania():
    assert bridge_pairs(["KUL", "SIN", "DPS", "SYD"], "CBR")[:2] == [("KUL", "DPS"), ("KUL", "SYD")]


def test_bridge_pairs_skip_domestic():
    assert bridge_pairs(["ORD", "DTW", "ATL"], "ORD") == []
    assert bridge_pairs(["DXB", "DOH"], "JFK") == []


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
