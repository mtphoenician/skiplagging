mod common;

use std::collections::HashSet;

use skiplagging::engines::candidates::plan_expansion;
use skiplagging::engines::discover::_via_stops;
use skiplagging::engines::hidden::{
    detect_hidden_city, hidden_city_savings, is_standard_to, itinerary_fingerprint,
    meaningful_saving, refresh_keeps_hidden_city, ticketed_destination,
};
use skiplagging::engines::index::{classify_for_search, observation_meta, ticketed_dests_through};
use skiplagging::engines::shop::{
    _cache_state, _classify_hidden, _needs_city_nonstop, _reuse_indexed_honest,
};
use skiplagging::models::{Airport, Offer, Segment};
use skiplagging::providers::mock::{
    jfk_dfw_den, jfk_dfw_lax, jfk_ord_den, jfk_ord_nonstop, jfk_ord_sea, MockProvider,
};

use common::{hs, shop_req};

fn two_stop_exit_b() -> Offer {
    fn s(o: &str, d: &str, flight: &str) -> Segment {
        Segment {
            origin: o.into(),
            dest: d.into(),
            carrier: "UA".into(),
            flight_number: flight.into(),
            dep: "2026-11-19T08:00".into(),
            arr: "2026-11-19T10:00".into(),
            duration_min: 120,
            rbd: "Y".into(),
            ..Segment::default()
        }
    }
    Offer {
        id: "two-stop-exit-b".into(),
        kind: "connecting".into(),
        channel: "ndc".into(),
        source: "duffel".into(),
        segments: vec![
            s("JFK", "CLT", "UA1"),
            s("CLT", "ORD", "UA2"),
            s("ORD", "DEN", "UA3"),
        ],
        price: Some(180.0),
        currency: "USD".into(),
        cabin: "ECONOMY".into(),
        fare_basis: "Y".into(),
        carrier: "UA".into(),
        duration_min: 480,
        stops: 2,
        first_flight: "UA1".into(),
        live: Some(true),
        ..Offer::default()
    }
}

fn three_stop_exit_b() -> Offer {
    fn s(o: &str, d: &str, flight: &str) -> Segment {
        Segment {
            origin: o.into(),
            dest: d.into(),
            carrier: "UA".into(),
            flight_number: flight.into(),
            dep: "2026-11-19T08:00".into(),
            arr: "2026-11-19T10:00".into(),
            duration_min: 90,
            rbd: "Y".into(),
            ..Segment::default()
        }
    }
    Offer {
        id: "three-stop-exit-b".into(),
        kind: "connecting".into(),
        channel: "gds".into(),
        source: "duffel".into(),
        segments: vec![
            s("JFK", "CLT", "UA1"),
            s("CLT", "ATL", "UA2"),
            s("ATL", "ORD", "UA3"),
            s("ORD", "DEN", "UA4"),
        ],
        price: Some(160.0),
        currency: "USD".into(),
        cabin: "ECONOMY".into(),
        fare_basis: "Y".into(),
        carrier: "UA".into(),
        duration_min: 600,
        stops: 3,
        first_flight: "UA1".into(),
        live: Some(true),
        ..Offer::default()
    }
}

#[test]
fn hidden_when_intermediate_is_intended() {
    let offer = jfk_ord_den("2026-10-10");
    let hit = detect_hidden_city(&offer, "ORD").unwrap();
    assert_eq!(hit.exit_airport, "ORD");
    assert_eq!(hit.exit_segment_index, 0);
    assert_eq!(
        hit.unused_segments
            .iter()
            .map(|s| format!("{}{}", s.origin, s.dest))
            .collect::<Vec<_>>(),
        ["ORDDEN"]
    );
}

#[test]
fn standard_when_ticketed_is_intended() {
    let offer = jfk_ord_den("2026-10-10");
    assert!(detect_hidden_city(&offer, "DEN").is_none());
    assert!(is_standard_to(&offer, "DEN"));
    assert!(is_standard_to(&jfk_ord_nonstop("2026-10-10"), "ORD"));
}

#[test]
fn hidden_when_intended_is_the_second_stop() {
    let through = two_stop_exit_b();
    let hit = detect_hidden_city(&through, "ORD").unwrap();
    assert_eq!(hit.exit_airport, "ORD");
    assert_eq!(hit.exit_segment_index, 1);
    assert_eq!(
        hit.unused_segments
            .iter()
            .map(|s| s.dest.as_str())
            .collect::<Vec<_>>(),
        ["DEN"]
    );
    let honest = jfk_ord_nonstop("2026-11-19");
    let (matches, _) = _classify_hidden(&[through], &hs(&["ORD"]), &honest, None, 20.0);
    assert_eq!(matches[0].through_offer.id, "two-stop-exit-b");
    assert_eq!(matches[0].exit_segment_index, 1);
}

#[test]
fn hidden_when_intended_is_the_third_stop() {
    let through = three_stop_exit_b();
    let hit = detect_hidden_city(&through, &hs(&["ORD", "MDW"])).unwrap();
    assert_eq!(hit.exit_airport, "ORD");
    assert_eq!(hit.exit_segment_index, 2);
    let honest = jfk_ord_nonstop("2026-11-19");
    let (matches, _) = _classify_hidden(&[through], &hs(&["ORD", "MDW"]), &honest, None, 20.0);
    assert_eq!(
        matches
            .iter()
            .map(|m| m.through_offer.id.as_str())
            .collect::<Vec<_>>(),
        ["three-stop-exit-b"]
    );
}

#[test]
fn rejected_when_path_misses_intended() {
    assert!(detect_hidden_city(&jfk_dfw_lax("2026-10-10"), "ORD").is_none());
    assert!(!is_standard_to(&jfk_dfw_lax("2026-10-10"), "ORD"));
}

#[test]
fn round_trip_is_never_hidden_city() {
    let mut through = jfk_ord_den("2026-10-10");
    through.return_date = Some("2026-10-17".into());
    through.outbound_end = Some(1);
    assert!(detect_hidden_city(&through, "ORD").is_none());
}

#[test]
fn ticketed_destination_uses_outbound_on_round_trip() {
    let mut offer = jfk_ord_nonstop("2026-10-10");
    let mut ret = offer.segments[0].clone();
    ret.origin = "ORD".into();
    ret.dest = "JFK".into();
    ret.dep = "2026-10-17T18:00".into();
    ret.arr = "2026-10-17T21:00".into();
    offer.return_date = Some("2026-10-17".into());
    offer.outbound_end = Some(0);
    offer.segments.push(ret);
    assert_eq!(ticketed_destination(&offer), "ORD");
    assert!(is_standard_to(&offer, "ORD"));
}

#[tokio::test]
async fn mock_round_trip_is_honest_only() {
    let mock = MockProvider::new();
    let mut req = shop_req("JFK", "ORD", "2026-10-10");
    req.return_date = Some("2026-10-17".into());
    let offers = mock.shop(&req).await.unwrap();
    assert_eq!(offers.len(), 1);
    let offer = &offers[0];
    assert_eq!(offer.return_date.as_deref(), Some("2026-10-17"));
    assert_eq!(offer.outbound_end, Some(0));
    assert_eq!(ticketed_destination(offer), "ORD");
    assert!(detect_hidden_city(offer, "ORD").is_none());
    assert_eq!(offer.stops, 0);
}

#[test]
fn refresh_invalid_when_connection_moves() {
    let original = jfk_ord_den("2026-10-10");
    let mutated = jfk_dfw_den("2026-10-10");
    assert!(refresh_keeps_hidden_city(&original, &original, "ORD"));
    assert!(!refresh_keeps_hidden_city(&original, &mutated, "ORD"));
}

#[tokio::test]
async fn mock_refresh_reprices_den_and_rejects_moved_hub() {
    let mock = MockProvider::new();
    let den = jfk_ord_den("2026-10-10");
    let fresh = mock.refresh_offer(&den).await.unwrap().unwrap();
    assert_eq!(fresh.price, Some(175.0));
    assert!(refresh_keeps_hidden_city(&den, &fresh, "ORD"));
    let mut mutated = den.clone();
    mutated.id = "mock-jfk-ord-den-mutated".into();
    let moved = mock.refresh_offer(&mutated).await.unwrap().unwrap();
    assert_eq!(moved.segments[0].dest, "DFW");
    assert!(!refresh_keeps_hidden_city(&den, &moved, "ORD"));
    let mut live = den.clone();
    live.id = "duffel-off_1".into();
    live.source = "duffel".into();
    assert!(mock.refresh_offer(&live).await.unwrap().is_none());
}

#[test]
fn savings_calculation() {
    assert_eq!(hidden_city_savings(Some(240.0), Some(170.0)), Some(70.0));
    assert_eq!(hidden_city_savings(Some(240.0), Some(185.0)), Some(55.0));
    assert!(meaningful_saving(Some(70.0), None));
    assert!(!meaningful_saving(Some(19.0), None));
}

#[test]
fn fingerprint_groups_same_flights() {
    let a = jfk_ord_den("2026-10-10");
    let mut b = a.clone();
    b.id = "other".into();
    b.price = Some(199.0);
    assert_eq!(itinerary_fingerprint(&a), itinerary_fingerprint(&b));
    assert_ne!(
        itinerary_fingerprint(&a),
        itinerary_fingerprint(&jfk_ord_sea("2026-10-10"))
    );
}

#[test]
fn mock_jfk_ord_first_target() {
    let date = "2026-10-10";
    let offers = vec![
        jfk_ord_nonstop(date),
        jfk_ord_den(date),
        jfk_ord_sea(date),
        jfk_dfw_lax(date),
    ];
    let standard = jfk_ord_nonstop(date);
    let (matches, rejected) = _classify_hidden(&offers, &hs(&["ORD"]), &standard, None, 20.0);
    let ids: HashSet<_> = matches
        .iter()
        .map(|m| m.through_offer.id.as_str())
        .collect();
    assert_eq!(ids, HashSet::from(["mock-jfk-ord-den", "mock-jfk-ord-sea"]));
    let savings: std::collections::HashMap<_, _> = matches
        .iter()
        .map(|m| (m.through_offer.id.as_str(), m.gross_saving))
        .collect();
    assert_eq!(savings["mock-jfk-ord-den"], 70.0);
    assert_eq!(savings["mock-jfk-ord-sea"], 55.0);
    assert!(rejected.iter().any(|row| row.contains("mock-jfk-dfw-lax")));
    assert!(matches
        .iter()
        .all(|m| m.ticketed_destination == "DEN" || m.ticketed_destination == "SEA"));
    assert!(matches.iter().all(|m| m.intended_destination == "ORD"));
    assert!(matches.iter().all(|m| !m.warnings.is_empty()));
}

#[test]
fn index_one_offer_serves_two_searches() {
    let through = jfk_ord_den("2026-10-10");
    let meta = observation_meta(&through).unwrap();
    assert_eq!(meta.origin, "JFK");
    assert_eq!(meta.ticketed, "DEN");
    assert_eq!(meta.connections, ["ORD"]);
    assert_eq!(
        classify_for_search(&through, &hs(&["JFK"]), &hs(&["DEN"])),
        Some("standard")
    );
    assert_eq!(
        classify_for_search(&through, &hs(&["JFK"]), &hs(&["ORD"])),
        Some("hidden_city")
    );
    assert_eq!(
        classify_for_search(&through, &hs(&["JFK"]), &hs(&["SEA"])),
        None
    );
    assert_eq!(
        ticketed_dests_through(&[through, jfk_dfw_lax("2026-10-10")], &hs(&["ORD"])),
        hs(&["DEN"])
    );
}

#[test]
fn plan_skips_expansion_when_hidden_already_found() {
    let extra = ["DEN".into(), "SEA".into(), "SFO".into()];
    let (dests, skipped) = plan_expansion(
        "JFK",
        &hs(&["ORD"]),
        8,
        Some(&extra),
        &[jfk_ord_den("2026-10-10")],
        true,
    );
    assert!(dests.is_empty());
    assert!(skipped.iter().any(|c| c == "DEN"));
}

#[test]
fn plan_skips_covered_destinations_but_shops_the_rest() {
    let extra = ["DEN".into(), "SFO".into()];
    let (dests, skipped) = plan_expansion(
        "JFK",
        &hs(&["ORD"]),
        8,
        Some(&extra),
        &[jfk_ord_den("2026-10-10")],
        false,
    );
    assert!(dests.iter().any(|c| c == "SFO"));
    assert!(!dests.iter().any(|c| c == "DEN"));
    assert!(skipped.iter().any(|c| c == "DEN"));
}

#[test]
fn plan_expands_only_when_no_through_tickets() {
    let extra = ["MIA".into(), "ATL".into()];
    let (dests, skipped) = plan_expansion("EWR", &hs(&["BOS"]), 2, Some(&extra), &[], false);
    assert_eq!(dests, ["MIA", "ATL"]);
    assert!(skipped.is_empty());
}

#[test]
fn city_nonstop_only_on_first_metro_pair() {
    let city = Airport {
        type_: "city".into(),
        ..Airport::default()
    };
    let airport = Airport {
        type_: "large_airport".into(),
        ..Airport::default()
    };
    assert!(_needs_city_nonstop(&city, &city, 0));
    assert!(!_needs_city_nonstop(&city, &city, 1));
    assert!(!_needs_city_nonstop(&airport, &airport, 0));
}

#[test]
fn cache_state_labels_index_reuse() {
    assert_eq!(_cache_state(&[1], &[] as &[i32]), "index");
    assert_eq!(_cache_state(&[1], &[2]), "index+live");
    assert_eq!(_cache_state(&[] as &[i32], &[2]), "miss");
}

#[test]
fn discover_via_stops_include_later_connections() {
    let offer = Offer {
        id: "two-stop".into(),
        kind: "connecting".into(),
        channel: "ndc".into(),
        source: "duffel".into(),
        segments: vec![
            Segment {
                origin: "JFK".into(),
                dest: "CLT".into(),
                carrier: "UA".into(),
                flight_number: "UA1".into(),
                dep: "2026-11-19T08:00".into(),
                arr: "2026-11-19T10:00".into(),
                duration_min: 120,
                rbd: "Y".into(),
                ..Segment::default()
            },
            Segment {
                origin: "CLT".into(),
                dest: "ORD".into(),
                carrier: "UA".into(),
                flight_number: "UA2".into(),
                dep: "2026-11-19T11:00".into(),
                arr: "2026-11-19T13:00".into(),
                duration_min: 120,
                rbd: "Y".into(),
                ..Segment::default()
            },
            Segment {
                origin: "ORD".into(),
                dest: "DEN".into(),
                carrier: "UA".into(),
                flight_number: "UA3".into(),
                dep: "2026-11-19T14:00".into(),
                arr: "2026-11-19T16:00".into(),
                duration_min: 120,
                rbd: "Y".into(),
                ..Segment::default()
            },
        ],
        price: Some(180.0),
        currency: "USD".into(),
        cabin: "ECONOMY".into(),
        fare_basis: "Y".into(),
        carrier: "UA".into(),
        duration_min: 480,
        stops: 2,
        first_flight: "UA1".into(),
        live: Some(true),
        ..Offer::default()
    };
    let (ticketed, vias) = _via_stops(&offer, "JFK");
    assert_eq!(ticketed, "DEN");
    assert_eq!(vias, [(0, "CLT".into()), (1, "ORD".into())]);
}

fn live_honest(age_secs: i64) -> Offer {
    let mut offer = Offer {
        id: "live-ab".into(),
        kind: "nonstop".into(),
        channel: "ndc".into(),
        source: "duffel".into(),
        segments: vec![Segment {
            origin: "JFK".into(),
            dest: "ORD".into(),
            carrier: "AA".into(),
            flight_number: "AA100".into(),
            dep: "2026-11-19T08:00".into(),
            arr: "2026-11-19T10:15".into(),
            duration_min: 135,
            rbd: "Y".into(),
            ..Segment::default()
        }],
        price: Some(240.0),
        currency: "USD".into(),
        cabin: "ECONOMY".into(),
        carrier: "AA".into(),
        duration_min: 135,
        stops: 0,
        first_flight: "AA100".into(),
        live: Some(true),
        ..Offer::default()
    };
    offer.retrieved_at = Some(
        (chrono::Utc::now() - chrono::Duration::seconds(age_secs))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    );
    offer
}

#[test]
fn reuse_indexed_honest_only_fresh_live_duffel() {
    let fresh = live_honest(5);
    assert!(!_reuse_indexed_honest(&[fresh.clone()], true, true));
    assert!(!_reuse_indexed_honest(&[fresh.clone()], false, false));
    assert!(_reuse_indexed_honest(&[fresh.clone()], false, true));
    let stale = live_honest(90);
    assert!(!_reuse_indexed_honest(&[stale], false, true));
    let mut mock = live_honest(5);
    mock.source = "mock".into();
    assert!(!_reuse_indexed_honest(&[mock], false, true));
    let mut sandbox = live_honest(5);
    sandbox.live = Some(false);
    assert!(!_reuse_indexed_honest(&[sandbox], false, true));
    let mut missing = live_honest(5);
    missing.retrieved_at = None;
    assert!(!_reuse_indexed_honest(&[missing], false, true));
}
