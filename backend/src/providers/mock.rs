use chrono::Utc;

use crate::models::{Offer, Segment, ShopRequest};

fn seg(origin: &str, dest: &str, dep: &str, arr: &str, flight: &str, carrier: &str, minutes: i32) -> Segment {
    Segment {
        origin: origin.into(),
        dest: dest.into(),
        carrier: carrier.into(),
        operating_carrier: Some(carrier.into()),
        flight_number: flight.into(),
        dep: dep.into(),
        arr: arr.into(),
        duration_min: minutes,
        rbd: "Y".into(),
        aircraft: String::new(),
        fare_basis: None,
    }
}

fn offer(oid: &str, price: f64, segments: Vec<Segment>) -> Offer {
    let n = segments.len();
    let duration = segments.iter().map(|s| s.duration_min).sum::<i32>()
        + if n > 1 { 40 } else { 0 };
    let carrier = segments[0].carrier.clone();
    let first = segments[0].flight_number.clone();
    Offer {
        id: oid.into(),
        kind: if n == 1 {
            "nonstop".into()
        } else {
            "connecting".into()
        },
        channel: "ndc".into(),
        source: "mock".into(),
        layer: "priced-offer".into(),
        segments,
        price: Some(price),
        base_price: Some((price * 0.82 * 100.0).round() / 100.0),
        taxes: Some((price * 0.18 * 100.0).round() / 100.0),
        currency: "USD".into(),
        cabin: "ECONOMY".into(),
        fare_basis: "Y".into(),
        seats: Some(7),
        validating_airline: Some(carrier.clone()),
        carrier,
        duration_min: duration,
        stops: n.saturating_sub(1) as i32,
        first_flight: first,
        retrieved_at: Some(Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
        note: Some("Mock fixture. Complete priced itinerary — not a reconstructed segment sum.".into()),
        ..Offer::default()
    }
}

fn t(date: &str, hhmm: &str) -> String {
    format!("{date}T{hhmm}:00")
}

pub fn jfk_ord_nonstop(date: &str) -> Offer {
    offer(
        "mock-jfk-ord-nonstop",
        240.0,
        vec![seg("JFK", "ORD", &t(date, "08:00"), &t(date, "10:15"), "AA100", "AA", 135)],
    )
}

pub fn jfk_ord_den(date: &str) -> Offer {
    offer(
        "mock-jfk-ord-den",
        170.0,
        vec![
            seg("JFK", "ORD", &t(date, "08:00"), &t(date, "10:15"), "UA200", "UA", 135),
            seg("ORD", "DEN", &t(date, "11:30"), &t(date, "13:20"), "UA300", "UA", 110),
        ],
    )
}

pub fn jfk_ord_sea(date: &str) -> Offer {
    offer(
        "mock-jfk-ord-sea",
        185.0,
        vec![
            seg("JFK", "ORD", &t(date, "09:00"), &t(date, "11:20"), "AS400", "AS", 140),
            seg("ORD", "SEA", &t(date, "12:40"), &t(date, "15:10"), "AS500", "AS", 150),
        ],
    )
}

pub fn jfk_dfw_lax(date: &str) -> Offer {
    offer(
        "mock-jfk-dfw-lax",
        150.0,
        vec![
            seg("JFK", "DFW", &t(date, "07:00"), &t(date, "10:30"), "AA600", "AA", 210),
            seg("DFW", "LAX", &t(date, "11:40"), &t(date, "13:10"), "AA700", "AA", 90),
        ],
    )
}

pub fn jfk_dfw_den(date: &str) -> Offer {
    offer(
        "mock-jfk-dfw-den",
        150.0,
        vec![
            seg("JFK", "DFW", &t(date, "08:00"), &t(date, "11:10"), "UA200", "UA", 190),
            seg("DFW", "DEN", &t(date, "12:20"), &t(date, "13:40"), "UA300", "UA", 80),
        ],
    )
}

pub fn jfk_ord_roundtrip(date: &str, return_date: &str) -> Offer {
    let mut offer = offer(
        "mock-jfk-ord-roundtrip",
        430.0,
        vec![
            seg("JFK", "ORD", &t(date, "08:00"), &t(date, "10:15"), "AA100", "AA", 135),
            seg("ORD", "JFK", &t(return_date, "18:00"), &t(return_date, "21:15"), "AA101", "AA", 135),
        ],
    );
    offer.kind = "nonstop".into();
    offer.stops = 0;
    offer.return_date = Some(return_date.into());
    offer.outbound_end = Some(0);
    offer.duration_min = 270;
    offer.note = Some("Mock round-trip fixture. Hidden-city is one-way only.".into());
    offer
}

pub struct MockProvider;

impl MockProvider {
    pub fn new() -> Self {
        Self
    }

    pub async fn shop(&self, req: &ShopRequest) -> anyhow::Result<Vec<Offer>> {
        let origin = req.origin.to_uppercase();
        let dest = req.dest.to_uppercase();
        let date = &req.date;
        if req.return_date.is_some() {
            if origin == "JFK" && matches!(dest.as_str(), "ORD" | "CHI" | "MDW") {
                return Ok(vec![jfk_ord_roundtrip(date, req.return_date.as_deref().unwrap())]);
            }
            return Ok(vec![]);
        }
        if origin == "JFK" && matches!(dest.as_str(), "ORD" | "CHI" | "MDW") {
            return Ok(vec![
                jfk_ord_nonstop(date),
                jfk_ord_den(date),
                jfk_ord_sea(date),
                jfk_dfw_lax(date),
            ]);
        }
        if origin == "JFK" && dest == "DEN" {
            return Ok(vec![jfk_ord_den(date)]);
        }
        if origin == "JFK" && dest == "SEA" {
            return Ok(vec![jfk_ord_sea(date)]);
        }
        Ok(vec![])
    }

    pub async fn refresh_offer(&self, offer: &Offer) -> anyhow::Result<Option<Offer>> {
        let date = offer
            .segments
            .first()
            .and_then(|s| {
                if s.dep.len() >= 10 {
                    Some(s.dep[..10].to_string())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| Utc::now().date_naive().to_string());
        if offer.id == "mock-jfk-ord-den" {
            let mut fresh = jfk_ord_den(&date);
            fresh.price = Some(175.0);
            fresh.id = offer.id.clone();
            return Ok(Some(fresh));
        }
        if offer.id == "mock-jfk-ord-den-mutated" {
            return Ok(Some(jfk_dfw_den(&date)));
        }
        if offer.source != "mock" {
            return Ok(None);
        }
        let Some(price) = offer.price else {
            return Ok(None);
        };
        let mut fresh = offer.clone();
        fresh.price = Some(((price + 5.0) * 100.0).round() / 100.0);
        Ok(Some(fresh))
    }
}

impl Default for MockProvider {
    fn default() -> Self {
        Self
    }
}
