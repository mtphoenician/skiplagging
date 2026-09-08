from datetime import datetime, timezone, timedelta

from app.engines.expiry import offer_unexpired, unexpired
from app.models import Offer, Segment
from app.providers.duffel import offer_request_body
from app.providers.base import ShopRequest


def _offer(expires: str | None) -> Offer:
    return Offer(
        id="o",
        kind="nonstop",
        channel="ndc",
        source="duffel",
        segments=[
            Segment(
                origin="JFK",
                dest="ORD",
                carrier="AA",
                flight_number="AA1",
                dep="2026-10-10T08:00",
                arr="2026-10-10T10:00",
                duration_min=135,
                rbd="Y",
            )
        ],
        price=200,
        currency="USD",
        cabin="ECONOMY",
        fare_basis="Y",
        carrier="AA",
        duration_min=135,
        stops=0,
        first_flight="AA1",
        expires_at=expires,
        live=True,
    )


def test_missing_expires_at_is_kept():
    now = datetime(2026, 9, 8, 12, tzinfo=timezone.utc)
    assert offer_unexpired(_offer(None), now)


def test_past_expires_at_is_dropped():
    now = datetime(2026, 9, 8, 12, tzinfo=timezone.utc)
    dead = _offer((now - timedelta(minutes=1)).strftime("%Y-%m-%dT%H:%M:%SZ"))
    live = _offer((now + timedelta(minutes=30)).strftime("%Y-%m-%dT%H:%M:%SZ"))
    assert not offer_unexpired(dead, now)
    assert offer_unexpired(live, now)
    assert [o.id for o in unexpired([dead, live], now)] == ["o"]
    assert unexpired([dead, live], now)[0].expires_at == live.expires_at


def test_duffel_round_trip_request_has_two_slices():
    req = ShopRequest(origin="JFK", dest="ORD", date="2026-10-10", return_date="2026-10-17")
    slices = offer_request_body(req)["data"]["slices"]
    assert len(slices) == 2
    assert slices[0]["origin"] == "JFK" and slices[0]["destination"] == "ORD"
    assert slices[1]["origin"] == "ORD" and slices[1]["destination"] == "JFK"
    assert slices[1]["departure_date"] == "2026-10-17"


def test_sandbox_gap_names_the_test_token():
    from types import SimpleNamespace

    from app.engines.shop import _gaps
    from app.models import LiveTraffic

    traffic = LiveTraffic(
        airport="BEY",
        source="opensky",
        api_time=None,
        note="OpenSky empty",
        aircraft=[],
        trackers=[],
    )
    settings = SimpleNamespace(duffel_sandbox=True)
    gaps = _gaps(settings, object(), None, [], [], traffic, [], None, False)
    assert any("sandbox" in g and "duffel_test_" in g for g in gaps)
    assert not any("test inventory" in g.lower() for g in gaps)


def test_round_trip_gap_says_hidden_city_is_one_way():
    from types import SimpleNamespace

    from app.engines.shop import _gaps
    from app.models import LiveTraffic

    traffic = LiveTraffic(
        airport="JFK",
        source="opensky",
        api_time=None,
        note="OpenSky empty",
        aircraft=[],
        trackers=[],
    )
    settings = SimpleNamespace(duffel_sandbox=False)
    gaps = _gaps(settings, object(), None, [_offer(None)], [], traffic, [], None, True)
    assert any("one-way" in g.lower() for g in gaps)
