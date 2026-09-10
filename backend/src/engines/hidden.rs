use crate::config::get_settings;
use crate::models::{Offer, Segment};

pub const HIDDEN_CITY_WARNINGS: &[&str] = &[
    "Hidden-city itineraries carry special restrictions.",
    "Do not check baggage unless your baggage is confirmed to be delivered at your intended stop.",
    "Skipping a segment may cancel all subsequent segments on the same ticket.",
    "Airline contracts and policies may restrict hidden-city ticketing.",
    "Schedule changes may alter the connection airport.",
    "This prototype does not provide legal or contractual advice.",
];

#[derive(Debug, Clone)]
pub struct HiddenCityHit {
    pub exit_airport: String,
    pub exit_segment_index: i32,
    pub unused_segments: Vec<Segment>,
}

pub trait Intended {
    fn codes(&self) -> std::collections::HashSet<String>;
    fn contains_code(&self, code: &str) -> bool {
        self.codes().contains(&code.to_uppercase())
    }
}

impl Intended for &str {
    fn codes(&self) -> std::collections::HashSet<String> {
        std::collections::HashSet::from([(*self).to_uppercase()])
    }
    fn contains_code(&self, code: &str) -> bool {
        self.eq_ignore_ascii_case(code)
    }
}

impl Intended for String {
    fn codes(&self) -> std::collections::HashSet<String> {
        std::collections::HashSet::from([self.to_uppercase()])
    }
    fn contains_code(&self, code: &str) -> bool {
        self.eq_ignore_ascii_case(code)
    }
}

impl Intended for std::collections::HashSet<String> {
    fn codes(&self) -> std::collections::HashSet<String> {
        self.iter().map(|s| s.to_uppercase()).collect()
    }
    fn contains_code(&self, code: &str) -> bool {
        self.contains(code) || self.contains(&code.to_uppercase())
    }
}

impl Intended for &std::collections::HashSet<String> {
    fn codes(&self) -> std::collections::HashSet<String> {
        (*self).iter().map(|s| s.to_uppercase()).collect()
    }
    fn contains_code(&self, code: &str) -> bool {
        self.contains(code) || self.contains(&code.to_uppercase())
    }
}

impl Intended for [&str] {
    fn codes(&self) -> std::collections::HashSet<String> {
        self.iter().map(|s| s.to_uppercase()).collect()
    }
    fn contains_code(&self, code: &str) -> bool {
        self.iter().any(|s| s.eq_ignore_ascii_case(code))
    }
}

impl<'a> Intended for std::collections::HashSet<&'a str> {
    fn codes(&self) -> std::collections::HashSet<String> {
        self.iter().map(|s| s.to_uppercase()).collect()
    }
    fn contains_code(&self, code: &str) -> bool {
        self.contains(code) || {
            let u = code.to_uppercase();
            self.contains(u.as_str())
        }
    }
}

pub fn ticketed_destination(offer: &Offer) -> String {
    if offer.segments.is_empty() {
        return String::new();
    }
    if offer.return_date.is_some() {
        if let Some(end) = offer.outbound_end {
            let idx = end.max(0).min((offer.segments.len() - 1) as i32) as usize;
            return offer.segments[idx].dest.to_uppercase();
        }
        let origin = offer.segments[0].origin.to_uppercase();
        if offer.segments.last().map(|s| s.dest.to_uppercase()) == Some(origin.clone()) {
            let mut last = offer.segments[0].dest.to_uppercase();
            for seg in &offer.segments {
                let dest = seg.dest.to_uppercase();
                if dest == origin {
                    break;
                }
                last = dest;
            }
            return last;
        }
    }
    offer
        .segments
        .last()
        .map(|s| s.dest.to_uppercase())
        .unwrap_or_default()
}

pub fn detect_hidden_city(offer: &Offer, intended: impl Intended) -> Option<HiddenCityHit> {
    if offer.return_date.is_some() {
        return None;
    }
    let segs = &offer.segments;
    if segs.len() < 2 {
        return None;
    }
    if intended.contains_code(&ticketed_destination(offer)) {
        return None;
    }
    for (index, segment) in segs[..segs.len() - 1].iter().enumerate() {
        if intended.contains_code(&segment.dest) {
            return Some(HiddenCityHit {
                exit_airport: segment.dest.to_uppercase(),
                exit_segment_index: index as i32,
                unused_segments: segs[index + 1..].to_vec(),
            });
        }
    }
    None
}

pub fn is_standard_to(offer: &Offer, intended: impl Intended) -> bool {
    !offer.segments.is_empty() && intended.contains_code(&ticketed_destination(offer))
}

pub fn hidden_city_savings(best_standard: Option<f64>, through_price: Option<f64>) -> Option<f64> {
    Some(((best_standard? - through_price?) * 100.0).round() / 100.0)
}

pub fn meaningful_saving(saving: Option<f64>, floor: Option<f64>) -> bool {
    let floor = floor.unwrap_or_else(|| get_settings().min_hidden_saving);
    saving.map(|s| s >= floor).unwrap_or(false)
}

pub fn itinerary_fingerprint(offer: &Offer) -> String {
    offer
        .segments
        .iter()
        .map(|segment| {
            let dep = if segment.dep.len() >= 16 {
                &segment.dep[..16]
            } else {
                &segment.dep
            };
            format!(
                "{}:{}:{}:{}",
                segment.flight_number,
                segment.origin.to_uppercase(),
                segment.dest.to_uppercase(),
                dep
            )
        })
        .collect::<Vec<_>>()
        .join("|")
}

pub fn refresh_keeps_hidden_city(
    original: &Offer,
    refreshed: &Offer,
    intended: impl Intended,
) -> bool {
    let dests = intended.codes();
    match (
        detect_hidden_city(original, &dests),
        detect_hidden_city(refreshed, &dests),
    ) {
        (Some(old), Some(new)) => old.exit_airport == new.exit_airport,
        _ => false,
    }
}
