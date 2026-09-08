from sqlalchemy import delete

from app.db import repo
from app.db.session import init_db, session_factory
from app.db.tables import HiddenDealRow
from app.fx import to_usd
from app.models import HiddenCityMatch, Offer, RiskAssessment, Segment
from app.providers.sandbox import is_live_fare, is_sandbox_offer


def _seg(origin: str, dest: str, flight: str) -> Segment:
    return Segment(
        origin=origin,
        dest=dest,
        carrier="BA",
        flight_number=flight,
        dep="2026-10-28T08:00",
        arr="2026-10-28T11:00",
        duration_min=180,
        rbd="Y",
    )


def _offer(
    oid: str,
    price: float,
    segments: list[Segment],
    stops: int,
    *,
    currency: str = "USD",
    source: str = "duffel",
    live: bool | None = True,
) -> Offer:
    return Offer(
        id=oid,
        kind="hidden-city" if stops else "nonstop",
        channel="ndc",
        source=source,
        segments=segments,
        price=price,
        currency=currency,
        cabin="ECONOMY",
        fare_basis="Y",
        carrier=segments[0].carrier,
        duration_min=sum(s.duration_min for s in segments),
        stops=stops,
        first_flight=segments[0].flight_number,
        live=live,
    )


def _match(local: Offer, through: Offer, hidden: str, saving: float, pct: float) -> HiddenCityMatch:
    return HiddenCityMatch(
        id="m1",
        hidden_city=hidden,
        hidden_city_name="Boston (BOS)",
        local_offer=local,
        through_offer=through,
        first_flight_match=True,
        gross_saving=saving,
        saving_pct=pct,
        currency=through.currency,
        risk=RiskAssessment(
            score=40,
            headline="test",
            one_way_only=True,
            carry_on_only=True,
            items=[],
            net_saving_estimate=saving,
            expected_disruption_cost=0,
            expected_enforcement_cost=0,
        ),
    )


async def test_hidden_deals_persist_and_list():
    await init_db()
    local = _offer("n1", 400, [_seg("ORD", "DCA", "BA1")], 0)
    through = _offer("h1", 220, [_seg("ORD", "DCA", "BA1"), _seg("DCA", "BOS", "BA2")], 1)
    try:
        async with session_factory()() as session:
            n = await repo.persist_hidden_deals(
                session,
                [_match(local, through, "BOS", 180, 45)],
                origin="ORD",
                origin_city="Chicago",
                dest="DCA",
                dest_city="Washington",
                date="2026-10-28",
            )
            assert n == 1
            deals = await repo.list_hidden_deals(session, origin="ORD")
        assert deals
        top = next(d for d in deals if d.origin == "ORD" and d.hidden_city == "BOS" and d.first_flight == "BA1")
        assert top.through_price == 220
        assert top.honest_price == 400
        assert top.saving == 180
        assert top.currency == "USD"
    finally:
        async with session_factory()() as session:
            await session.execute(
                delete(HiddenDealRow).where(
                    HiddenDealRow.origin == "ORD",
                    HiddenDealRow.destination == "DCA",
                    HiddenDealRow.hidden_city == "BOS",
                    HiddenDealRow.date == "2026-10-28",
                    HiddenDealRow.first_flight == "BA1",
                )
            )
            await session.commit()


async def test_persist_converts_to_usd_and_skips_sandbox():
    await init_db()
    gbp_local = _offer("n-gbp", 400, [_seg("LHR", "DUB", "BA9")], 0, currency="GBP")
    gbp_through = _offer(
        "h-gbp",
        220,
        [_seg("LHR", "DUB", "BA9"), _seg("DUB", "BOS", "BA8")],
        1,
        currency="GBP",
    )
    fake_local = _offer("n-fake", 400, [_seg("JFK", "ORD", "BA3")], 0, live=False)
    fake_through = _offer(
        "h-fake",
        100,
        [_seg("JFK", "ORD", "BA3"), _seg("ORD", "DEN", "BA4")],
        1,
        live=False,
    )
    mock_local = _offer("n-mock", 400, [_seg("JFK", "BOS", "AA1")], 0, source="mock", live=None)
    mock_through = _offer(
        "h-mock",
        200,
        [_seg("JFK", "BOS", "AA1"), _seg("BOS", "MIA", "AA2")],
        1,
        source="mock",
        live=None,
    )
    try:
        async with session_factory()() as session:
            n_live = await repo.persist_hidden_deals(
                session,
                [_match(gbp_local, gbp_through, "BOS", 180, 45)],
                origin="LHR",
                origin_city="London",
                dest="DUB",
                dest_city="Dublin",
                date="2026-11-02",
            )
            n_fake = await repo.persist_hidden_deals(
                session,
                [_match(fake_local, fake_through, "DEN", 300, 75)],
                origin="JFK",
                origin_city="New York",
                dest="ORD",
                dest_city="Chicago",
                date="2026-11-02",
            )
            n_mock = await repo.persist_hidden_deals(
                session,
                [_match(mock_local, mock_through, "MIA", 200, 50)],
                origin="JFK",
                origin_city="New York",
                dest="BOS",
                dest_city="Boston",
                date="2026-11-02",
            )
            assert n_live == 1
            assert n_fake == 0
            assert n_mock == 0
            deals = await repo.list_hidden_deals(session, origin="LHR", dest="DUB")
        assert len(deals) == 1
        assert deals[0].currency == "USD"
        assert deals[0].honest_price == to_usd(400, "GBP")
        assert deals[0].through_price == to_usd(220, "GBP")
        assert deals[0].saving == round(deals[0].honest_price - deals[0].through_price, 2)
    finally:
        async with session_factory()() as session:
            await session.execute(
                delete(HiddenDealRow).where(
                    HiddenDealRow.origin.in_(["LHR", "JFK"]),
                    HiddenDealRow.date == "2026-11-02",
                    HiddenDealRow.first_flight.in_(["BA9", "BA3", "AA1"]),
                )
            )
            await session.commit()


async def test_list_ranks_usd_and_hides_sandbox_rows():
    await init_db()
    krw_local = _offer("n-krw", 80000, [_seg("ICN", "NRT", "KE1")], 0, currency="KRW", source="amadeus", live=None)
    krw_through = _offer(
        "h-krw",
        10000,
        [_seg("ICN", "NRT", "KE1"), _seg("NRT", "HNL", "KE2")],
        1,
        currency="KRW",
        source="amadeus",
        live=None,
    )
    usd_local = _offer("n-usd", 400, [_seg("ICN", "NRT", "DL1")], 0, source="amadeus", live=None)
    usd_through = _offer(
        "h-usd",
        150,
        [_seg("ICN", "NRT", "DL1"), _seg("NRT", "LAX", "DL2")],
        1,
        source="amadeus",
        live=None,
    )
    fake = _offer(
        "h-sand",
        50,
        [_seg("ICN", "NRT", "ZZ1"), _seg("NRT", "SEA", "ZZ2")],
        1,
        live=False,
    )
    try:
        async with session_factory()() as session:
            session.add(
                HiddenDealRow(
                    origin="ICN",
                    destination="NRT",
                    hidden_city="HNL",
                    origin_city="Seoul",
                    dest_city="Tokyo",
                    hidden_city_name="Honolulu (HNL)",
                    date="2026-11-03",
                    honest_price=80000,
                    through_price=10000,
                    currency="KRW",
                    saving=70000,
                    saving_pct=87,
                    first_flight="KE1",
                    source="amadeus",
                    local_payload=krw_local.model_dump(),
                    through_payload=krw_through.model_dump(),
                )
            )
            session.add(
                HiddenDealRow(
                    origin="ICN",
                    destination="NRT",
                    hidden_city="LAX",
                    origin_city="Seoul",
                    dest_city="Tokyo",
                    hidden_city_name="Los Angeles (LAX)",
                    date="2026-11-03",
                    honest_price=400,
                    through_price=150,
                    currency="USD",
                    saving=250,
                    saving_pct=62,
                    first_flight="DL1",
                    source="amadeus",
                    local_payload=usd_local.model_dump(),
                    through_payload=usd_through.model_dump(),
                )
            )
            session.add(
                HiddenDealRow(
                    origin="ICN",
                    destination="NRT",
                    hidden_city="SEA",
                    origin_city="Seoul",
                    dest_city="Tokyo",
                    hidden_city_name="Seattle (SEA)",
                    date="2026-11-03",
                    honest_price=400,
                    through_price=50,
                    currency="USD",
                    saving=350,
                    saving_pct=87,
                    first_flight="ZZ1",
                    source="duffel",
                    local_payload=usd_local.model_dump(),
                    through_payload=fake.model_dump(),
                )
            )
            await session.commit()
            deals = await repo.list_hidden_deals(session, origin="ICN", dest="NRT")
        cities = [d.hidden_city for d in deals]
        assert "SEA" not in cities
        assert cities[0] == "LAX"
        assert deals[0].saving == 250
        assert deals[0].currency == "USD"
    finally:
        async with session_factory()() as session:
            await session.execute(
                delete(HiddenDealRow).where(
                    HiddenDealRow.origin == "ICN",
                    HiddenDealRow.destination == "NRT",
                    HiddenDealRow.date == "2026-11-03",
                )
            )
            await session.commit()


def test_live_fare_rejects_test_inventory():
    live = _offer("ok", 200, [_seg("JFK", "LHR", "BA1")], 0, live=True)
    fake = live.model_copy(update={"live": False, "note": "Duffel offer. live_mode=False."})
    mock = live.model_copy(update={"source": "mock", "live": None})
    zz = live.model_copy(update={"carrier": "ZZ", "live": True})
    zz.segments[0] = zz.segments[0].model_copy(update={"carrier": "ZZ"})
    assert is_live_fare(live)
    assert is_sandbox_offer(fake)
    assert not is_live_fare(fake)
    assert not is_live_fare(mock)
    assert not is_live_fare(zz)
