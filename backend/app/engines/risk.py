from __future__ import annotations

from app.engines.hidden import HIDDEN_CITY_WARNINGS, ticketed_destination
from app.models import HiddenCityMatch, Offer, RiskAssessment, RiskItem


def assess(
    local: Offer,
    through: Offer,
    hidden_city: str,
    true_dest: str,
    origin_country: str = "",
    hidden_country: str = "",
) -> RiskAssessment:
    """
    S_net = P_AB - P_ABC - extra fees - E[disruption] - E[enforcement]

    Costs are expected-value estimates for ranking, not predictions of
    any carrier's action. Detection/evasion advice is intentionally absent.
    """
    items: list[RiskItem] = [
        RiskItem(
            id="baggage",
            label="Checked bag continues to C",
            severity="high",
            predictability="high",
            why="Baggage is tagged to the ticketed itinerary, not the passenger's private destination.",
        ),
        RiskItem(
            id="gate-check",
            label="Carry-on unexpectedly gate-checked",
            severity="high",
            predictability="medium",
            why="Overhead capacity or operational limits can force a cabin bag into the hold toward C.",
        ),
        RiskItem(
            id="irrops",
            label="Reroute avoids B",
            severity="very-high",
            predictability="low",
            why="During disruption the carrier protects ticketed C, not intermediate B. The hidden city can vanish.",
        ),
        RiskItem(
            id="coupons",
            label="Later coupons cancelled",
            severity="very-high",
            predictability="high",
            why="Sequential coupon use treats the ticket as one product. Skipping B→C can void everything after it.",
        ),
        RiskItem(
            id="return",
            label="Return or continuing travel lost",
            severity="very-high",
            predictability="high",
            why="A one-way A→B→C ticket is structurally different from a return. Do not attach later coupons.",
        ),
        RiskItem(
            id="flexibility",
            label="Lost flexibility during disruption",
            severity="very-high",
            predictability="inherent",
            why="The passenger values B; the carrier values C. Those objectives diverge the moment the plan breaks.",
        ),
    ]

    international = bool(origin_country and hidden_country and origin_country != hidden_country)
    if international:
        items.append(
            RiskItem(
                id="documents",
                label="International documentation for C",
                severity="high",
                predictability="high",
                why="The airline processes the passenger as travelling to ticketed C, including passport and visa checks.",
            )
        )

    items.append(
        RiskItem(
            id="enforcement",
            label="Questioning, repricing, or refusal",
            severity="medium" if not international else "high",
            predictability="low",
            why="Carrier contracts of carriage may allow cancellation, repricing, refusal of carriage or fare-difference collection when a passenger does not complete the ticketed itinerary.",
        )
    )

    gross = max((local.price or 0) - (through.price or 0), 0.0)
    extra_fees = 18.0 if through.bags_included == 0 else 0.0
    # Fragility rises when C is international, when connect is tight, when near departure.
    disruption = round(min(gross * 0.22, local.price * 0.18), 2)
    enforcement = round(min(gross * 0.12, 80.0), 2)
    if international:
        disruption += 25
        enforcement += 20
    net = round(gross - extra_fees - disruption - enforcement, 2)

    # 0 = fragile, 100 = relatively contained (still never "safe")
    score = 58
    score -= 18  # inherent coupon / irrops
    if international:
        score -= 14
    if len(through.segments) > 2:
        score -= 10
    if through.duration_min and through.segments:
        connect = _connect_minutes(through)
        if connect is not None and connect < 50:
            score -= 8
    score = max(8, min(score, 72))

    if score >= 50:
        headline = "Usable only as a fragile one-way, carry-on itinerary"
    elif score >= 35:
        headline = "High operational fragility — savings can disappear"
    else:
        headline = "Poor candidate — document, coupon or disruption risk dominates"

    return RiskAssessment(
        score=score,
        headline=headline,
        one_way_only=True,
        carry_on_only=True,
        items=items,
        net_saving_estimate=net,
        expected_disruption_cost=disruption,
        expected_enforcement_cost=enforcement,
    )


def attach_risk(
    match_id: str,
    hidden_city: str,
    hidden_name: str,
    local: Offer,
    through: Offer,
    true_dest: str,
    origin_country: str = "",
    hidden_country: str = "",
    exit_segment_index: int = 0,
) -> HiddenCityMatch:
    first_match = bool(local.first_flight and local.first_flight == through.first_flight)
    if not first_match and local.segments and through.segments:
        first_match = (
            local.segments[0].origin == through.segments[0].origin
            and local.segments[0].dest == through.segments[0].dest
            and local.segments[0].carrier == through.segments[0].carrier
        )
    gross = round((local.price or 0) - (through.price or 0), 2)
    pct = round(100.0 * gross / local.price, 1) if local.price else 0.0
    return HiddenCityMatch(
        id=match_id,
        hidden_city=hidden_city,
        hidden_city_name=hidden_name,
        local_offer=local,
        through_offer=through,
        first_flight_match=first_match,
        gross_saving=gross,
        saving_pct=pct,
        currency=through.currency,
        risk=assess(local, through, hidden_city, true_dest, origin_country, hidden_country),
        ticketed_destination=ticketed_destination(through),
        intended_destination=true_dest.upper(),
        exit_segment_index=exit_segment_index,
        warnings=list(HIDDEN_CITY_WARNINGS),
        result_type="hidden_city",
    )


def _connect_minutes(offer: Offer) -> int | None:
    if len(offer.segments) < 2:
        return None
    try:
        a = offer.segments[0].arr
        b = offer.segments[1].dep
        from datetime import datetime

        t0 = datetime.fromisoformat(a.replace("Z", "+00:00"))
        t1 = datetime.fromisoformat(b.replace("Z", "+00:00"))
        return int((t1 - t0).total_seconds() // 60)
    except ValueError:
        return None
