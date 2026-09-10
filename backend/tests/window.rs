use chrono::{Duration, TimeZone, Utc};
use skiplagging::engines::window::{human_mins, urgency_rank, window_for, FarePoint};
use skiplagging::models::Offer;

fn offer_at(dep: &str, seats: Option<i32>, expires: Option<&str>, price: f64) -> Offer {
    let mut o = common::jfk_ord_den_through(dep);
    o.seats = seats;
    o.expires_at = expires.map(|s| s.to_string());
    o.price = Some(price);
    o
}

mod common {
    use skiplagging::models::{Offer, Segment};

    pub fn jfk_ord_den_through(dep: &str) -> Offer {
        Offer {
            id: "t".into(),
            kind: "hidden-city".into(),
            segments: vec![
                Segment {
                    origin: "JFK".into(),
                    dest: "ORD".into(),
                    carrier: "AA".into(),
                    flight_number: "AA100".into(),
                    dep: dep.into(),
                    arr: dep.into(),
                    duration_min: 150,
                    ..Segment::default()
                },
                Segment {
                    origin: "ORD".into(),
                    dest: "DEN".into(),
                    carrier: "AA".into(),
                    flight_number: "AA200".into(),
                    dep: dep.into(),
                    arr: dep.into(),
                    duration_min: 150,
                    ..Segment::default()
                },
            ],
            price: Some(180.0),
            currency: "USD".into(),
            ..Offer::default()
        }
    }
}

fn now() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 10, 12, 0, 0).unwrap()
}

#[test]
fn two_seats_is_now() {
    let dep = (now() + Duration::days(14)).to_rfc3339();
    let w = window_for(
        &offer_at(&dep, Some(2), None, 180.0),
        Some(180.0),
        &[],
        now(),
    );
    assert_eq!(w.urgency, "now");
    assert!(w.label.contains("seats"));
    assert!(w.headline.contains("through ticket"));
}

#[test]
fn quote_expiring_is_now() {
    let dep = (now() + Duration::days(20)).to_rfc3339();
    let exp = (now() + Duration::minutes(12)).to_rfc3339();
    let w = window_for(
        &offer_at(&dep, Some(9), Some(&exp), 180.0),
        Some(180.0),
        &[],
        now(),
    );
    assert_eq!(w.urgency, "now");
    assert!(w.headline.contains("dies"));
}

#[test]
fn departs_tonight_is_now() {
    let dep = (now() + Duration::hours(11)).to_rfc3339();
    let w = window_for(
        &offer_at(&dep, Some(9), None, 180.0),
        Some(180.0),
        &[],
        now(),
    );
    assert_eq!(w.urgency, "now");
    assert!(w.headline.contains("close in"));
}

#[test]
fn three_days_out_is_soon() {
    let dep = (now() + Duration::hours(48)).to_rfc3339();
    let w = window_for(
        &offer_at(&dep, Some(9), None, 180.0),
        Some(180.0),
        &[],
        now(),
    );
    assert_eq!(w.urgency, "soon");
}

#[test]
fn far_out_few_seats_stays_urgent() {
    let dep = (now() + Duration::days(40)).to_rfc3339();
    let w = window_for(
        &offer_at(&dep, Some(4), None, 180.0),
        Some(180.0),
        &[],
        now(),
    );
    assert_eq!(w.urgency, "soon");
}

#[test]
fn rising_price_bumps_open_to_watch() {
    let dep = (now() + Duration::days(30)).to_rfc3339();
    let series = vec![
        FarePoint {
            fingerprint: String::new(),
            origin: "JFK".into(),
            ticketed: "DEN".into(),
            date: "2026-10-10".into(),
            price: 140.0,
            observed_at: now() - Duration::days(20),
        },
        FarePoint {
            fingerprint: String::new(),
            origin: "JFK".into(),
            ticketed: "DEN".into(),
            date: "2026-10-10".into(),
            price: 170.0,
            observed_at: now() - Duration::days(2),
        },
    ];
    let w = window_for(
        &offer_at(&dep, Some(9), None, 180.0),
        Some(180.0),
        &series,
        now(),
    );
    assert_eq!(w.trend, "rising");
    assert!(urgency_rank(&w.urgency) >= urgency_rank("watch"));
    assert!(w.low.unwrap() <= 140.5);
    assert!(w.points.len() >= 2);
}

#[test]
fn sparkline_keeps_current_and_low() {
    let dep = (now() + Duration::days(16)).to_rfc3339();
    let mut series = Vec::new();
    for i in 0..40 {
        series.push(FarePoint {
            fingerprint: String::new(),
            origin: "JFK".into(),
            ticketed: "DEN".into(),
            date: "2026-10-10".into(),
            price: 200.0 - i as f64,
            observed_at: now() - Duration::days(40 - i),
        });
    }
    let w = window_for(
        &offer_at(&dep, None, None, 150.0),
        Some(150.0),
        &series,
        now(),
    );
    assert!(w.points.len() <= 24);
    assert!(w.points.last().unwrap().price <= 151.0);
    assert!(w.low.unwrap() <= w.high.unwrap());
}

#[test]
fn human_minutes_reads() {
    assert_eq!(human_mins(12), "12 min");
    assert_eq!(human_mins(180), "3h");
    assert_eq!(human_mins(3 * 1440), "3d");
}

#[test]
fn urgency_order() {
    assert!(urgency_rank("now") > urgency_rank("soon"));
    assert!(urgency_rank("soon") > urgency_rank("watch"));
    assert!(urgency_rank("watch") > urgency_rank("open"));
}
