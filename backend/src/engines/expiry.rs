use chrono::{DateTime, Utc};

use crate::models::Offer;

pub fn parse_offer_time(raw: Option<&str>) -> Option<DateTime<Utc>> {
    let text = raw?.trim();
    if text.is_empty() {
        return None;
    }
    let text = text.replace('Z', "+00:00");
    DateTime::parse_from_rfc3339(&text)
        .or_else(|_| DateTime::parse_from_rfc3339(&format!("{text}+00:00")))
        .ok()
        .map(|d| d.with_timezone(&Utc))
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(&text, "%Y-%m-%dT%H:%M:%S")
                .ok()
                .map(|n| n.and_utc())
        })
}

pub fn offer_unexpired(offer: &Offer) -> bool {
    offer_unexpired_at(offer, None)
}

pub fn offer_unexpired_at(offer: &Offer, now: Option<DateTime<Utc>>) -> bool {
    let Some(exp) = parse_offer_time(offer.expires_at.as_deref()) else {
        return true;
    };
    let moment = now.unwrap_or_else(Utc::now);
    exp > moment
}

pub fn unexpired(offers: &[Offer]) -> Vec<Offer> {
    unexpired_at(offers, None)
}

pub fn take_unexpired(offers: Vec<Offer>) -> Vec<Offer> {
    offers.into_iter().filter(offer_unexpired).collect()
}

pub fn unexpired_at(offers: &[Offer], now: Option<DateTime<Utc>>) -> Vec<Offer> {
    let moment = now.unwrap_or_else(Utc::now);
    offers
        .iter()
        .filter(|o| offer_unexpired_at(o, Some(moment)))
        .cloned()
        .collect()
}
