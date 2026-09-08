from types import SimpleNamespace
from unittest.mock import MagicMock

import httpx
import pytest

from app.engines.refresh import refresh_priced_offer
from app.fx import offer_in_usd, to_usd
from app.models import Offer, SearchQuery, Segment
from app.providers.base import ShopRequest
from app.providers.duffel import DuffelProvider


def test_search_query_forces_usd():
    q = SearchQuery(origin="JFK", destination="ORD", date="2026-10-10", currency="GBP")
    assert q.currency == "USD"


def test_search_query_round_trip_order():
    q = SearchQuery(origin="JFK", destination="ORD", date="2026-10-10", return_date="2026-10-17")
    assert q.return_date == "2026-10-17"
    with pytest.raises(Exception):
        SearchQuery(origin="JFK", destination="ORD", date="2026-10-17", return_date="2026-10-10")


def test_to_usd_identity_and_gbp():
    assert to_usd(240, "USD") == 240
    converted = to_usd(80, "GBP")
    assert converted is not None
    assert converted > 80
    assert converted < 200


def test_offer_in_usd_converts_gbp():
    offer = Offer(
        id="g1",
        kind="nonstop",
        channel="ndc",
        source="duffel",
        segments=[
            Segment(
                origin="LHR",
                dest="JFK",
                carrier="BA",
                flight_number="BA1",
                dep="2026-10-10T08:00",
                arr="2026-10-10T11:00",
                duration_min=180,
                rbd="Y",
            )
        ],
        price=100,
        base_price=70,
        taxes=30,
        currency="GBP",
        cabin="ECONOMY",
        fare_basis="Y",
        carrier="BA",
        duration_min=180,
        stops=0,
        first_flight="BA1",
        live=True,
    )
    converted = offer_in_usd(offer)
    assert converted is not None
    assert converted.currency == "USD"
    assert converted.price is not None and converted.price > 100


DUFFEL_OFFER = {
    "id": "off_live_1",
    "live_mode": True,
    "total_amount": "170.00",
    "total_currency": "GBP",
    "base_amount": "140.00",
    "tax_amount": "30.00",
    "expires_at": "2026-10-10T12:00:00Z",
    "owner": {"iata_code": "AA"},
    "slices": [
        {
            "duration": "PT5H",
            "segments": [
                {
                    "origin": {"iata_code": "JFK"},
                    "destination": {"iata_code": "ORD"},
                    "marketing_carrier": {"iata_code": "AA"},
                    "operating_carrier": {"iata_code": "AA"},
                    "marketing_carrier_flight_number": "100",
                    "departing_at": "2026-10-10T08:00",
                    "arriving_at": "2026-10-10T10:15",
                    "duration": "PT2H15M",
                    "aircraft": {"iata_code": "32N"},
                },
                {
                    "origin": {"iata_code": "ORD"},
                    "destination": {"iata_code": "DEN"},
                    "marketing_carrier": {"iata_code": "AA"},
                    "operating_carrier": {"iata_code": "AA"},
                    "marketing_carrier_flight_number": "200",
                    "departing_at": "2026-10-10T11:30",
                    "arriving_at": "2026-10-10T13:00",
                    "duration": "PT2H30M",
                    "aircraft": {"iata_code": "32N"},
                },
            ],
        }
    ],
}


@pytest.mark.asyncio
async def test_duffel_get_offer_parses_and_refresh_converts_to_usd():
    async def handler(request: httpx.Request) -> httpx.Response:
        if request.method == "GET" and "/air/offers/" in str(request.url):
            return httpx.Response(200, json={"data": DUFFEL_OFFER})
        return httpx.Response(404, json={"errors": [{"message": "not found"}]})

    transport = httpx.MockTransport(handler)
    async with httpx.AsyncClient(transport=transport) as client:
        settings = SimpleNamespace(duffel_token="tok", duffel_enabled=True)
        provider = DuffelProvider(MagicMock(duffel_token="tok"), client)
        got = await provider.get_offer("duffel-off_live_1")
        assert got is not None
        assert got.live is True
        assert got.currency == "GBP"
        assert got.price == 170.0
        assert [s.dest for s in got.segments] == ["ORD", "DEN"]
        usd = await refresh_priced_offer(got, settings, client)  # type: ignore[arg-type]
        assert usd is not None
        assert usd.currency == "USD"
        assert usd.price is not None and usd.price > 170


def test_duffel_two_slice_sets_return_date_without_req():
    from app.engines.hidden import ticketed_destination
    from app.providers.duffel import _one

    raw = {
        "id": "off_rt",
        "live_mode": True,
        "total_amount": "430.00",
        "total_currency": "USD",
        "slices": [
            {
                "duration": "PT2H15M",
                "segments": [
                    {
                        "origin": {"iata_code": "JFK"},
                        "destination": {"iata_code": "ORD"},
                        "marketing_carrier": {"iata_code": "AA"},
                        "operating_carrier": {"iata_code": "AA"},
                        "marketing_carrier_flight_number": "100",
                        "departing_at": "2026-10-10T08:00",
                        "arriving_at": "2026-10-10T10:15",
                        "duration": "PT2H15M",
                    }
                ],
            },
            {
                "departure_date": "2026-10-17",
                "duration": "PT2H15M",
                "segments": [
                    {
                        "origin": {"iata_code": "ORD"},
                        "destination": {"iata_code": "JFK"},
                        "marketing_carrier": {"iata_code": "AA"},
                        "operating_carrier": {"iata_code": "AA"},
                        "marketing_carrier_flight_number": "101",
                        "departing_at": "2026-10-17T18:00",
                        "arriving_at": "2026-10-17T21:15",
                        "duration": "PT2H15M",
                    }
                ],
            },
        ],
    }
    req = ShopRequest(origin="JFK", dest="ORD", date="2026-10-10")
    got = _one(raw, req, "2026-09-08T12:00:00+00:00")
    assert got is not None
    assert got.return_date == "2026-10-17"
    assert got.outbound_end == 0
    assert ticketed_destination(got) == "ORD"
    assert got.stops == 0

