use chrono::DateTime;

use crate::engines::hidden::{ticketed_destination, HIDDEN_CITY_WARNINGS};
use crate::models::{HiddenCityMatch, Offer, RiskAssessment, RiskItem};

pub fn assess(
    local: &Offer,
    through: &Offer,
    _hidden_city: &str,
    _true_dest: &str,
    origin_country: &str,
    hidden_country: &str,
) -> RiskAssessment {
    let mut items = vec![
        RiskItem {
            id: "baggage".into(),
            label: "Checked bag continues to C".into(),
            severity: "high".into(),
            predictability: "high".into(),
            why: "Baggage is tagged to the ticketed itinerary, not the passenger's private destination.".into(),
        },
        RiskItem {
            id: "gate-check".into(),
            label: "Carry-on unexpectedly gate-checked".into(),
            severity: "high".into(),
            predictability: "medium".into(),
            why: "Overhead capacity or operational limits can force a cabin bag into the hold toward C.".into(),
        },
        RiskItem {
            id: "irrops".into(),
            label: "Reroute avoids B".into(),
            severity: "very-high".into(),
            predictability: "low".into(),
            why: "During disruption the carrier protects ticketed C, not intermediate B. The hidden city can vanish.".into(),
        },
        RiskItem {
            id: "coupons".into(),
            label: "Later coupons cancelled".into(),
            severity: "very-high".into(),
            predictability: "high".into(),
            why: "Sequential coupon use treats the ticket as one product. Skipping B→C can void everything after it.".into(),
        },
        RiskItem {
            id: "return".into(),
            label: "Return or continuing travel lost".into(),
            severity: "very-high".into(),
            predictability: "high".into(),
            why: "A one-way A→B→C ticket is structurally different from a return. Do not attach later coupons.".into(),
        },
        RiskItem {
            id: "flexibility".into(),
            label: "Lost flexibility during disruption".into(),
            severity: "very-high".into(),
            predictability: "inherent".into(),
            why: "The passenger values B; the carrier values C. Those objectives diverge the moment the plan breaks.".into(),
        },
    ];
    let international = !origin_country.is_empty()
        && !hidden_country.is_empty()
        && origin_country != hidden_country;
    if international {
        items.push(RiskItem {
            id: "documents".into(),
            label: "International documentation for C".into(),
            severity: "high".into(),
            predictability: "high".into(),
            why: "The airline processes the passenger as travelling to ticketed C, including passport and visa checks.".into(),
        });
    }
    items.push(RiskItem {
        id: "enforcement".into(),
        label: "Questioning, repricing, or refusal".into(),
        severity: if international { "high" } else { "medium" }.into(),
        predictability: "low".into(),
        why: "Carrier contracts of carriage may allow cancellation, repricing, refusal of carriage or fare-difference collection when a passenger does not complete the ticketed itinerary.".into(),
    });

    let gross = ((local.price.unwrap_or(0.0) - through.price.unwrap_or(0.0)).max(0.0) * 100.0).round()
        / 100.0;
    let extra_fees = if through.bags_included == 0 { 18.0 } else { 0.0 };
    let mut disruption = (gross * 0.22).min(local.price.unwrap_or(0.0) * 0.18);
    disruption = (disruption * 100.0).round() / 100.0;
    let mut enforcement = ((gross * 0.12).min(80.0) * 100.0).round() / 100.0;
    if international {
        disruption += 25.0;
        enforcement += 20.0;
    }
    let net = ((gross - extra_fees - disruption - enforcement) * 100.0).round() / 100.0;

    let mut score = 58;
    score -= 18;
    if international {
        score -= 14;
    }
    if through.segments.len() > 2 {
        score -= 10;
    }
    if through.duration_min > 0 && !through.segments.is_empty() {
        if let Some(connect) = connect_minutes(through) {
            if connect < 50 {
                score -= 8;
            }
        }
    }
    score = score.max(8).min(72);

    let headline = if score >= 50 {
        "Usable only as a fragile one-way, carry-on itinerary"
    } else if score >= 35 {
        "High operational fragility — savings can disappear"
    } else {
        "Poor candidate — document, coupon or disruption risk dominates"
    };

    RiskAssessment {
        score,
        headline: headline.into(),
        one_way_only: true,
        carry_on_only: true,
        items,
        net_saving_estimate: net,
        expected_disruption_cost: disruption,
        expected_enforcement_cost: enforcement,
    }
}

pub fn attach_risk(
    match_id: &str,
    hidden_city: &str,
    hidden_name: &str,
    local: &Offer,
    through: &Offer,
    true_dest: &str,
    origin_country: &str,
    hidden_country: &str,
    exit_segment_index: i32,
) -> HiddenCityMatch {
    let mut first_match = !local.first_flight.is_empty() && local.first_flight == through.first_flight;
    if !first_match && !local.segments.is_empty() && !through.segments.is_empty() {
        first_match = local.segments[0].origin == through.segments[0].origin
            && local.segments[0].dest == through.segments[0].dest
            && local.segments[0].carrier == through.segments[0].carrier;
    }
    let gross = ((local.price.unwrap_or(0.0) - through.price.unwrap_or(0.0)) * 100.0).round() / 100.0;
    let pct = if let Some(lp) = local.price {
        if lp != 0.0 {
            (100.0 * gross / lp * 10.0).round() / 10.0
        } else {
            0.0
        }
    } else {
        0.0
    };
    HiddenCityMatch {
        id: match_id.to_string(),
        hidden_city: hidden_city.to_string(),
        hidden_city_name: hidden_name.to_string(),
        local_offer: local.clone(),
        through_offer: through.clone(),
        first_flight_match: first_match,
        gross_saving: gross,
        saving_pct: pct,
        currency: through.currency.clone(),
        risk: assess(local, through, hidden_city, true_dest, origin_country, hidden_country),
        bookers: vec![],
        ticketed_destination: ticketed_destination(through),
        intended_destination: true_dest.to_uppercase(),
        exit_segment_index,
        warnings: HIDDEN_CITY_WARNINGS.iter().map(|s| (*s).to_string()).collect(),
        result_type: "hidden_city".into(),
    }
}

fn connect_minutes(offer: &Offer) -> Option<i32> {
    if offer.segments.len() < 2 {
        return None;
    }
    let a = offer.segments[0].arr.replace('Z', "+00:00");
    let b = offer.segments[1].dep.replace('Z', "+00:00");
    let t0 = DateTime::parse_from_rfc3339(&a)
        .ok()
        .or_else(|| DateTime::parse_from_rfc3339(&format!("{a}+00:00")).ok())?;
    let t1 = DateTime::parse_from_rfc3339(&b)
        .ok()
        .or_else(|| DateTime::parse_from_rfc3339(&format!("{b}+00:00")).ok())?;
    Some(((t1 - t0).num_seconds() / 60) as i32)
}
