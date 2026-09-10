use crate::models::Offer;
use std::collections::HashSet;

pub const TEST_CARRIERS: &[&str] = &["ZZ", "XX", "YY"];

pub fn is_test_carrier(code: &str) -> bool {
    TEST_CARRIERS.iter().any(|c| c.eq_ignore_ascii_case(code))
}

fn sandbox_note(note: &str) -> bool {
    note.to_ascii_lowercase().contains("live_mode=false")
}

pub fn is_sandbox_offer(offer: &Offer) -> bool {
    if offer.live == Some(false) {
        return true;
    }
    offer.source.eq_ignore_ascii_case("duffel") && sandbox_note(offer.note.as_deref().unwrap_or(""))
}

fn carrier_codes(offer: &Offer) -> HashSet<String> {
    let mut codes = HashSet::new();
    if !offer.carrier.is_empty() {
        codes.insert(offer.carrier.to_uppercase());
    }
    if let Some(v) = &offer.validating_airline {
        if !v.is_empty() {
            codes.insert(v.to_uppercase());
        }
    }
    for segment in &offer.segments {
        if !segment.carrier.is_empty() {
            codes.insert(segment.carrier.to_uppercase());
        }
        if let Some(op) = &segment.operating_carrier {
            if !op.is_empty() {
                codes.insert(op.to_uppercase());
            }
        }
    }
    codes
}

pub fn is_live_fare(offer: &Offer) -> bool {
    if is_sandbox_offer(offer) {
        return false;
    }
    if offer.source.eq_ignore_ascii_case("mock") {
        return false;
    }
    let codes = carrier_codes(offer);
    if !codes.is_empty() && codes.iter().all(|c| is_test_carrier(c)) {
        return false;
    }
    if offer.source.eq_ignore_ascii_case("duffel") {
        return offer.live == Some(true);
    }
    false
}
