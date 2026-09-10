use skiplagging::db::repo::{deal_from_row, HiddenDealRow};
use skiplagging::providers::bookers::{booker_links, google_flights_url, google_tfs};
use chrono::Utc;
use serde_json::json;
use sqlx::types::Json;

#[test]
fn google_tfs_matches_known_one_way() {
    assert_eq!(
        google_tfs("JFK", "LHR", "2026-02-18", 1, 1, None),
        "CBwQAhoeEgoyMDI2LTAyLTE4agcIARIDSkZLcgcIARIDTEhSQAFIAXABggELCP___________wGYAQI"
    );
}

#[test]
fn google_url_prefills_search_not_hash() {
    let url = google_flights_url("JFK", "ORD", "2026-10-10", "USD", 1, "ECONOMY", None);
    assert!(url.starts_with("https://www.google.com/travel/flights/search?tfs="));
    assert!(!url.split("tfs=").next().unwrap().contains("JFK"));
    assert!(url.contains("curr=USD"));
    assert!(!url.contains("#flt="));
}

#[test]
fn all_bookers_carry_route_and_date() {
    let links = booker_links("JFK", "ORD", "2026-10-10", 1, &[], "USD", "ECONOMY", None);
    let by_id: std::collections::HashMap<_, _> = links.iter().map(|b| (b.id.as_str(), b.url.as_str())).collect();
    assert!(by_id["google-flights"].contains("tfs="));
    assert!(by_id["kayak"].contains("JFK-ORD/2026-10-10"));
    assert!(by_id["skyscanner"].contains("/jfk/ord/261010/"));
    assert!(by_id["skyscanner"].contains("adultsv2=1") && by_id["skyscanner"].contains("rtn=0"));
    assert!(by_id["booking-com"].contains("JFK.AIRPORT-ORD.AIRPORT"));
    assert!(by_id["booking-com"].contains("depart=2026-10-10"));
    assert!(by_id["expedia"].contains("from:JFK,to:ORD,departure:10/10/2026"));
    assert!(by_id["expedia"].contains("trip=oneway"));
    assert!(by_id["kiwi"].contains("/jfk/ord/2026-10-10/no-return"));
    assert!(by_id["skiplagged-com"].contains("JFK/ORD/2026-10-10"));
    assert!(by_id["edreams"].contains("jfk-ord/2026-10-10/1-0-0"));
    for (sid, url) in &by_id {
        if *sid == "google-flights" || sid.starts_with("carrier-") {
            continue;
        }
        let u = url.to_uppercase();
        assert!(u.contains("JFK") && u.contains("ORD"), "{sid}");
        assert!(
            url.contains("2026") || url.contains("261010") || url.contains("10Oct26"),
            "{sid}"
        );
    }
}

#[test]
fn round_trip_bookers_use_return_date() {
    let links = booker_links(
        "JFK",
        "ORD",
        "2026-10-10",
        1,
        &[],
        "USD",
        "ECONOMY",
        Some("2026-10-17"),
    );
    let by_id: std::collections::HashMap<_, _> = links.iter().map(|b| (b.id.as_str(), b.url.as_str())).collect();
    assert!(by_id["google-flights"].contains(&google_tfs(
        "JFK",
        "ORD",
        "2026-10-10",
        1,
        1,
        Some("2026-10-17")
    )));
    assert!(by_id["kayak"].contains("JFK-ORD/2026-10-10/2026-10-17"));
    assert!(by_id["skyscanner"].contains("rtn=1"));
    assert!(by_id["skyscanner"].contains("261017"));
    assert!(by_id["expedia"].contains("trip=roundtrip"));
    assert!(by_id["kiwi"].contains("2026-10-17"));
    assert!(by_id["booking-com"].contains("return=2026-10-17"));
    assert!(by_id["trip-com"].contains("flighttype=rt"));
    assert!(by_id["priceline"].contains("ORD-JFK-20261017"));
    assert!(by_id["cheapoair"].contains("tripType=ROUNDTRIP"));
    assert!(by_id["edreams"].contains("2026-10-17"));
    assert!(by_id["despegar"].contains("roundtrip"));
    assert!(by_id["wego"].contains("2026-10-17"));
}

#[test]
fn stale_deal_google_hash_is_replaced_on_read() {
    let row = HiddenDealRow {
        id: 1,
        origin: "JFK".into(),
        destination: "ORD".into(),
        hidden_city: "DEN".into(),
        origin_city: "New York".into(),
        dest_city: "Chicago".into(),
        hidden_city_name: "Denver (DEN)".into(),
        date: "2026-11-18".into(),
        honest_price: 240.0,
        through_price: 170.0,
        currency: "USD".into(),
        saving: 70.0,
        saving_pct: 29.0,
        first_flight: "AA123".into(),
        source: String::new(),
        bookers: Json(json!([{
            "id": "google-flights",
            "name": "Google Flights",
            "layer": "meta-search",
            "role": "compare",
            "url": "https://www.google.com/travel/flights#flt=JFK.DEN.2026-11-18",
            "issues_ticket": false
        }])),
        local_payload: Json(json!({})),
        through_payload: Json(json!({})),
        retrieved_at: Utc::now(),
    };
    let deal = deal_from_row(&row).unwrap();
    let google = deal
        .bookers
        .iter()
        .find(|b| b.id == "google-flights")
        .unwrap()
        .url
        .as_str();
    assert!(google.contains("tfs="));
    assert!(!google.contains("#flt="));
    let kayak = deal
        .bookers
        .iter()
        .find(|b| b.id == "kayak")
        .unwrap()
        .url
        .as_str();
    assert!(kayak.contains("JFK-DEN"));
}

#[test]
fn bookers_have_no_carrier_website_roster() {
    let src = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/providers/bookers.rs"
    ));
    for banned in ["aa.com", "delta.com", "united.com", "lufthansa.com", "emirates.com"] {
        assert!(!src.contains(banned), "{banned}");
    }
    let links = booker_links(
        "XXX",
        "YYY",
        "2026-11-02",
        1,
        &[("ZZ".into(), "Zed Air".into()), ("BA".into(), "British Airways".into())],
        "USD",
        "ECONOMY",
        None,
    );
    let hosts: Vec<_> = links.iter().map(|u| u.url.as_str()).collect();
    assert!(hosts.iter().any(|u| u.contains("XXX") && u.contains("YYY") && u.contains("2026-11-02")));
    let google = links.iter().find(|b| b.id == "google-flights").unwrap().url.as_str();
    assert!(google.contains("tfs="));
    assert!(!google.contains("#flt="));
    assert!(hosts.iter().all(|u| !u.contains("Zed")));
    assert!(links.iter().any(|b| b.id == "carrier-BA" && b.name.starts_with("British")));
    assert!(links.iter().any(|b| b.layer == "meta-search"));
    assert!(links.iter().any(|b| b.layer == "ota" && b.issues_ticket));
    let names: std::collections::HashSet<_> = links.iter().map(|b| b.name.as_str()).collect();
    for n in [
        "Google Flights",
        "Kayak",
        "Skyscanner",
        "Momondo",
        "Cheapflights",
        "Wego",
        "Expedia",
        "Booking.com",
        "Trip.com",
        "Priceline",
        "Kiwi.com",
        "CheapOair",
        "eDreams",
        "Traveloka",
        "MakeMyTrip",
        "Despegar",
        "Skiplagged.com",
    ] {
        assert!(names.contains(n), "{n}");
    }
    assert!(links.iter().any(|b| b.url.contains("booking.com")
        && b.url.contains("XXX.AIRPORT")
        && b.url.contains("YYY.AIRPORT")));
    let paris = booker_links("PAR", "LON", "2026-11-02", 1, &[], "USD", "ECONOMY", None);
    let booking = paris.iter().find(|b| b.id == "booking-com").unwrap().url.as_str();
    assert!(booking.contains("PAR.CITY") && booking.contains("LON.CITY"));
    assert!(paris
        .iter()
        .find(|b| b.id == "google-flights")
        .unwrap()
        .url
        .contains("tfs="));
    assert!(links.iter().any(|b| b.url.contains("trip.com") && b.url.contains("xxx") && b.url.contains("yyy")));
    assert!(links.iter().any(|b| b.url.contains("kiwi.com") && b.url.contains("xxx") && b.url.contains("yyy")));
    assert!(links.iter().any(|b| b.url.contains("momondo.com")));
}

#[test]
fn invalid_month_does_not_panic() {
    let links = booker_links("JFK", "ORD", "2026-13-01", 1, &[], "USD", "ECONOMY", None);
    assert!(!links.is_empty());
    let google = links.iter().find(|b| b.id == "google-flights").unwrap();
    assert!(google.url.contains("tfs="));
}

#[test]
fn hardcoded_graph_and_demo_are_gone() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    assert!(!root.join("graph/airports.rs").exists());
    assert!(!root.join("graph/hubs.rs").exists());
    assert!(!root.join("providers/demo.rs").exists());
}
