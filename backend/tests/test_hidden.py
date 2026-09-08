from types import SimpleNamespace

from app.engines.candidates import plan_expansion
from app.engines.index import classify_for_search, observation_meta, ticketed_dests_through
from app.engines.shop import _cache_state, _needs_city_nonstop
from app.engines.hidden import (
    detect_hidden_city,
    hidden_city_savings,
    is_standard_to,
    itinerary_fingerprint,
    meaningful_saving,
    refresh_keeps_hidden_city,
)
from app.engines.shop import _classify_hidden
from app.providers.mock import jfk_dfw_den, jfk_dfw_lax, jfk_ord_den, jfk_ord_nonstop, jfk_ord_sea


def test_hidden_when_intermediate_is_intended():
    offer = jfk_ord_den("2026-10-10")
    hit = detect_hidden_city(offer, "ORD")
    assert hit is not None
    assert hit.exit_airport == "ORD"
    assert hit.exit_segment_index == 0
    assert [s.origin + s.dest for s in hit.unused_segments] == ["ORDDEN"]


def test_standard_when_ticketed_is_intended():
    offer = jfk_ord_den("2026-10-10")
    assert detect_hidden_city(offer, "DEN") is None
    assert is_standard_to(offer, "DEN")
    assert is_standard_to(jfk_ord_nonstop("2026-10-10"), "ORD")


def test_rejected_when_path_misses_intended():
    assert detect_hidden_city(jfk_dfw_lax("2026-10-10"), "ORD") is None
    assert not is_standard_to(jfk_dfw_lax("2026-10-10"), "ORD")


def test_refresh_invalid_when_connection_moves():
    original = jfk_ord_den("2026-10-10")
    mutated = jfk_dfw_den("2026-10-10")
    assert refresh_keeps_hidden_city(original, original, "ORD")
    assert not refresh_keeps_hidden_city(original, mutated, "ORD")


def test_savings_calculation():
    assert hidden_city_savings(240, 170) == 70
    assert hidden_city_savings(240, 185) == 55
    assert meaningful_saving(70)
    assert not meaningful_saving(19)


def test_fingerprint_groups_same_flights():
    a = jfk_ord_den("2026-10-10")
    b = a.model_copy(update={"id": "other", "price": 199})
    assert itinerary_fingerprint(a) == itinerary_fingerprint(b)
    assert itinerary_fingerprint(a) != itinerary_fingerprint(jfk_ord_sea("2026-10-10"))


def test_mock_jfk_ord_first_target():
    date = "2026-10-10"
    offers = [jfk_ord_nonstop(date), jfk_ord_den(date), jfk_ord_sea(date), jfk_dfw_lax(date)]
    standard = jfk_ord_nonstop(date)
    matches, rejected = _classify_hidden(offers, {"ORD"}, standard, None, 20)
    ids = {m.through_offer.id for m in matches}
    assert ids == {"mock-jfk-ord-den", "mock-jfk-ord-sea"}
    savings = {m.through_offer.id: m.gross_saving for m in matches}
    assert savings["mock-jfk-ord-den"] == 70
    assert savings["mock-jfk-ord-sea"] == 55
    assert any("mock-jfk-dfw-lax" in row for row in rejected)
    assert all(m.ticketed_destination in {"DEN", "SEA"} for m in matches)
    assert all(m.intended_destination == "ORD" for m in matches)
    assert all(m.warnings for m in matches)


def test_index_one_offer_serves_two_searches():
    through = jfk_ord_den("2026-10-10")
    meta = observation_meta(through)
    assert meta is not None
    assert meta["origin"] == "JFK"
    assert meta["ticketed"] == "DEN"
    assert meta["connections"] == ["ORD"]
    assert classify_for_search(through, {"JFK"}, {"DEN"}) == "standard"
    assert classify_for_search(through, {"JFK"}, {"ORD"}) == "hidden_city"
    assert classify_for_search(through, {"JFK"}, {"SEA"}) is None
    assert ticketed_dests_through([through, jfk_dfw_lax("2026-10-10")], {"ORD"}) == {"DEN"}


def test_plan_skips_expansion_when_hidden_already_found():
    dests, skipped = plan_expansion(
        "JFK",
        ["ORD"],
        8,
        extra=["DEN", "SEA", "SFO"],
        reused=[jfk_ord_den("2026-10-10")],
        already_hidden=True,
    )
    assert dests == []
    assert "DEN" in skipped


def test_plan_does_not_reshop_covered_destinations():
    dests, skipped = plan_expansion(
        "JFK",
        ["ORD"],
        8,
        extra=["DEN", "SFO"],
        reused=[jfk_ord_den("2026-10-10")],
        already_hidden=False,
    )
    assert dests == []
    assert "DEN" in skipped


def test_plan_expands_only_when_no_through_tickets():
    dests, skipped = plan_expansion(
        "EWR",
        ["BOS"],
        2,
        extra=["MIA", "ATL"],
        reused=[],
        already_hidden=False,
    )
    assert dests == ["MIA", "ATL"]
    assert skipped == []


def test_city_nonstop_only_on_first_metro_pair():
    city = SimpleNamespace(type="city")
    airport = SimpleNamespace(type="large_airport")
    assert _needs_city_nonstop(city, city, 0)
    assert not _needs_city_nonstop(city, city, 1)
    assert not _needs_city_nonstop(airport, airport, 0)


def test_cache_state_labels_index_reuse():
    assert _cache_state([1], []) == "index"
    assert _cache_state([1], [2]) == "index+live"
    assert _cache_state([], [2]) == "miss"
