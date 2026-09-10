use std::collections::HashSet;

use crate::engines::hidden::{
    detect_hidden_city, is_standard_to, itinerary_fingerprint, ticketed_destination,
};
use crate::models::Offer;

#[derive(Debug, Clone)]
pub struct ObservationMeta {
    pub origin: String,
    pub ticketed: String,
    pub connections: Vec<String>,
    pub fingerprint: String,
    pub price: f64,
    pub currency: String,
    pub source: String,
}

pub fn observation_meta(offer: &Offer) -> Option<ObservationMeta> {
    if offer.segments.is_empty() || offer.price.is_none() {
        return None;
    }
    let first = &offer.segments[0];
    let ticketed = ticketed_destination(offer);
    let mut idx = offer
        .outbound_end
        .unwrap_or((offer.segments.len() - 1) as i32);
    idx = idx.max(0).min((offer.segments.len() - 1) as i32);
    Some(ObservationMeta {
        origin: first.origin.to_uppercase(),
        ticketed,
        connections: offer.segments[..idx as usize]
            .iter()
            .map(|s| s.dest.to_uppercase())
            .collect(),
        fingerprint: itinerary_fingerprint(offer),
        price: offer.price.unwrap(),
        currency: offer.currency.clone(),
        source: offer.source.clone(),
    })
}

pub fn classify_for_search(
    offer: &Offer,
    origins: &HashSet<String>,
    intended: &HashSet<String>,
) -> Option<&'static str> {
    if offer.segments.is_empty() {
        return None;
    }
    let origin_u: HashSet<String> = origins.iter().map(|o| o.to_uppercase()).collect();
    if !origin_u.contains(&offer.segments[0].origin.to_uppercase()) {
        return None;
    }
    if is_standard_to(offer, intended) {
        return Some("standard");
    }
    if detect_hidden_city(offer, intended).is_some() {
        return Some("hidden_city");
    }
    None
}

pub fn ticketed_dests_through(offers: &[Offer], intended: &HashSet<String>) -> HashSet<String> {
    let mut covered = HashSet::new();
    for offer in offers {
        if detect_hidden_city(offer, intended).is_some() {
            covered.insert(ticketed_destination(offer));
        }
    }
    covered
}
