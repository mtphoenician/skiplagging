from sqlalchemy import delete

from app.db import repo
from app.db.session import init_db, session_factory
from app.db.tables import HiddenDealRow
from app.models import HiddenCityMatch, Offer, RiskAssessment, Segment


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


def _offer(oid: str, price: float, segments: list[Segment], stops: int) -> Offer:
    return Offer(
        id=oid,
        kind="hidden-city" if stops else "nonstop",
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


async def test_hidden_deals_persist_and_list():
    await init_db()
    local = _offer("n1", 400, [_seg("ORD", "DCA", "BA1")], 0)
    through = _offer("h1", 220, [_seg("ORD", "DCA", "BA1"), _seg("DCA", "BOS", "BA2")], 1)
    risk = RiskAssessment(
        score=40,
        headline="test",
        one_way_only=True,
        carry_on_only=True,
        items=[],
        net_saving_estimate=180,
        expected_disruption_cost=0,
        expected_enforcement_cost=0,
    )
    match = HiddenCityMatch(
        id="m1",
        hidden_city="BOS",
        hidden_city_name="Boston (BOS)",
        local_offer=local,
        through_offer=through,
        first_flight_match=True,
        gross_saving=180,
        saving_pct=45,
        currency="USD",
        risk=risk,
    )
    try:
        async with session_factory()() as session:
            n = await repo.persist_hidden_deals(
                session,
                [match],
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
