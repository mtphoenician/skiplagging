#![allow(dead_code)]

use std::collections::HashSet;
use std::sync::Arc;

use axum::Router;
use skiplagging::models::{
    Airport, HiddenCityMatch, Offer, RiskAssessment, Segment, ShopRequest,
};

pub fn hs(codes: &[&str]) -> HashSet<String> {
    codes.iter().map(|s| (*s).to_string()).collect()
}

pub fn seg(origin: &str, dest: &str, dep: &str, arr: &str, flight: &str) -> Segment {
    seg_carrier(origin, dest, dep, arr, flight, "BA")
}

pub fn seg_carrier(
    origin: &str,
    dest: &str,
    dep: &str,
    arr: &str,
    flight: &str,
    carrier: &str,
) -> Segment {
    Segment {
        origin: origin.into(),
        dest: dest.into(),
        carrier: carrier.into(),
        operating_carrier: None,
        flight_number: flight.into(),
        dep: dep.into(),
        arr: arr.into(),
        duration_min: 60,
        rbd: "Y".into(),
        ..Segment::default()
    }
}

pub fn offer(
    oid: &str,
    price: Option<f64>,
    segments: Vec<Segment>,
    stops: i32,
) -> Offer {
    offer_full(oid, price, segments, stops, "USD", "duffel", Some(true), None)
}

pub fn offer_full(
    oid: &str,
    price: Option<f64>,
    segments: Vec<Segment>,
    stops: i32,
    currency: &str,
    source: &str,
    live: Option<bool>,
    duration_min: Option<i32>,
) -> Offer {
    let first = segments
        .first()
        .map(|s| s.flight_number.clone())
        .unwrap_or_default();
    let carrier = segments
        .first()
        .map(|s| s.carrier.clone())
        .unwrap_or_default();
    let duration = duration_min.unwrap_or_else(|| segments.iter().map(|s| s.duration_min).sum());
    Offer {
        id: oid.into(),
        kind: if stops == 0 {
            "nonstop".into()
        } else {
            "connecting".into()
        },
        channel: "ndc".into(),
        source: source.into(),
        segments,
        price,
        currency: currency.into(),
        cabin: "ECONOMY".into(),
        fare_basis: "Y".into(),
        carrier,
        duration_min: duration,
        stops,
        first_flight: first,
        live,
        ..Offer::default()
    }
}

pub fn risk() -> RiskAssessment {
    RiskAssessment {
        score: 40,
        headline: "test".into(),
        one_way_only: true,
        carry_on_only: true,
        items: vec![],
        net_saving_estimate: 80.0,
        expected_disruption_cost: 0.0,
        expected_enforcement_cost: 0.0,
    }
}

pub fn hc_match(oid: &str, through: Offer, local: Offer, saving: f64) -> HiddenCityMatch {
    let hidden = through.segments.last().map(|s| s.dest.clone()).unwrap_or_default();
    HiddenCityMatch {
        id: oid.into(),
        hidden_city: hidden,
        hidden_city_name: "Beyond".into(),
        local_offer: local,
        through_offer: through,
        first_flight_match: true,
        gross_saving: saving,
        saving_pct: 20.0,
        currency: "USD".into(),
        risk: risk(),
        bookers: vec![],
        ticketed_destination: String::new(),
        intended_destination: String::new(),
        exit_segment_index: 0,
        warnings: vec![],
        result_type: "hidden_city".into(),
    }
}

pub fn shop_req(origin: &str, dest: &str, date: &str) -> ShopRequest {
    ShopRequest {
        origin: origin.into(),
        dest: dest.into(),
        date: date.into(),
        ..ShopRequest::default()
    }
}

pub fn city(iata: &str, members: &[&str]) -> Airport {
    Airport {
        iata: iata.into(),
        type_: "city".into(),
        members: members.iter().map(|m| (*m).to_string()).collect(),
        ..Airport::default()
    }
}

pub async fn pool() -> sqlx::PgPool {
    let settings = skiplagging::config::Settings::load();
    let pool = skiplagging::db::pool::connect_with(&settings)
        .await
        .expect("PostgreSQL (createdb skiplagging; cargo run --bin ingest)");
    skiplagging::db::schema::init_db(&pool).await.expect("init_db");
    pool
}

pub async fn app() -> Router {
    let settings = Arc::new(skiplagging::config::Settings::load());
    let pool = pool().await;
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .unwrap();
    let cache = skiplagging::cache::build_cache(&settings);
    skiplagging::http::router(skiplagging::http::AppState {
        settings,
        pool,
        http,
        cache,
    })
}
