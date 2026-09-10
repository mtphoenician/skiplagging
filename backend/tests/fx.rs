mod common;

use serde_json::json;
use skiplagging::engines::hidden::ticketed_destination;
use skiplagging::engines::refresh::refresh_priced_offer;
use skiplagging::fx::{apply_frankfurter_body, offer_in_usd, to_usd, FETCH_TIMEOUT};
use skiplagging::models::{SearchQuery, SearchQueryError, Segment};
use skiplagging::providers::duffel::{_one, _usd_quote};
use skiplagging::providers::sandbox::is_sandbox_offer;

use common::{offer_full, shop_req};

#[test]
fn search_query_forces_usd() {
    let q: SearchQuery = serde_json::from_value(json!({
        "origin": "JFK",
        "destination": "ORD",
        "date": "2026-10-10",
        "currency": "GBP"
    }))
    .unwrap();
    assert_eq!(q.currency, "USD");
}

#[test]
fn search_query_round_trip_order() {
    let mut ok: SearchQuery = serde_json::from_value(json!({
        "origin": "JFK",
        "destination": "ORD",
        "date": "2026-10-10",
        "return_date": "2026-10-17"
    }))
    .unwrap();
    ok.validate().unwrap();
    assert_eq!(ok.return_date.as_deref(), Some("2026-10-17"));
    let mut bad: SearchQuery = serde_json::from_value(json!({
        "origin": "JFK",
        "destination": "ORD",
        "date": "2026-10-17",
        "return_date": "2026-10-10"
    }))
    .unwrap();
    assert!(matches!(
        bad.validate(),
        Err(SearchQueryError::ReturnBeforeOutbound)
    ));
}

#[test]
fn search_query_rejects_impossible_dates() {
    for date in ["2026-13-01", "2026-02-30", "2026-00-10", "2026-10-32"] {
        let mut q: SearchQuery = serde_json::from_value(json!({
            "origin": "JFK",
            "destination": "ORD",
            "date": date
        }))
        .unwrap();
        assert!(
            matches!(q.validate(), Err(SearchQueryError::BadField("date"))),
            "{date}"
        );
    }
}

#[test]
fn to_usd_identity_and_gbp() {
    skiplagging::fx::mark_rates_trusted();
    assert_eq!(to_usd(Some(240.0), Some("USD")), Some(240.0));
    let converted = to_usd(Some(80.0), Some("GBP")).unwrap();
    assert!(converted > 80.0);
    assert!(converted < 200.0);
}

#[test]
fn to_usd_omits_foreign_until_fx_fetch() {
    skiplagging::fx::with_untrusted_rates(|| {
        assert_eq!(to_usd(Some(240.0), Some("USD")), Some(240.0));
        assert_eq!(to_usd(Some(80.0), Some("GBP")), None);
        assert!(offer_in_usd(&offer_full(
            "g1",
            Some(100.0),
            vec![Segment {
                origin: "LHR".into(),
                dest: "JFK".into(),
                carrier: "BA".into(),
                flight_number: "BA1".into(),
                dep: "2026-10-10T08:00".into(),
                arr: "2026-10-10T11:00".into(),
                duration_min: 180,
                rbd: "Y".into(),
                ..Segment::default()
            }],
            0,
            "GBP",
            "duffel",
            Some(true),
            Some(180),
        ))
        .is_none());
    });
}

#[test]
fn offer_in_usd_converts_gbp() {
    skiplagging::fx::mark_rates_trusted();
    let offer = offer_full(
        "g1",
        Some(100.0),
        vec![Segment {
            origin: "LHR".into(),
            dest: "JFK".into(),
            carrier: "BA".into(),
            flight_number: "BA1".into(),
            dep: "2026-10-10T08:00".into(),
            arr: "2026-10-10T11:00".into(),
            duration_min: 180,
            rbd: "Y".into(),
            ..Segment::default()
        }],
        0,
        "GBP",
        "duffel",
        Some(true),
        Some(180),
    );
    let converted = offer_in_usd(&offer).unwrap();
    assert_eq!(converted.currency, "USD");
    assert!(converted.price.unwrap() > 100.0);
}

const DUFFEL_OFFER: &str = r#"{
    "id": "off_live_1",
    "live_mode": true,
    "total_amount": "170.00",
    "total_currency": "GBP",
    "base_amount": "140.00",
    "tax_amount": "30.00",
    "expires_at": "2026-10-10T12:00:00Z",
    "owner": {"iata_code": "AA"},
    "slices": [{
        "duration": "PT5H",
        "segments": [
            {
                "origin": {"iata_code": "JFK"},
                "destination": {"iata_code": "ORD"},
                "marketing_carrier": {"iata_code": "AA"},
                "operating_carrier": {"iata_code": "AA"},
                "marketing_carrier_flight_number": "100",
                "departing_at": "2026-10-10T08:00",
                "arriving_at": "2026-10-10T10:15",
                "duration": "PT2H15M",
                "aircraft": {"iata_code": "32N"}
            },
            {
                "origin": {"iata_code": "ORD"},
                "destination": {"iata_code": "DEN"},
                "marketing_carrier": {"iata_code": "AA"},
                "operating_carrier": {"iata_code": "AA"},
                "marketing_carrier_flight_number": "200",
                "departing_at": "2026-10-10T11:30",
                "arriving_at": "2026-10-10T13:00",
                "duration": "PT2H30M",
                "aircraft": {"iata_code": "32N"}
            }
        ]
    }]
}"#;

#[test]
fn duffel_get_offer_parses_and_converts_to_usd() {
    skiplagging::fx::mark_rates_trusted();
    let raw: serde_json::Value = serde_json::from_str(DUFFEL_OFFER).unwrap();
    let req = shop_req("JFK", "ORD", "2026-10-10");
    let got = _one(&raw, &req, "2026-09-08T12:00:00+00:00").unwrap();
    assert_eq!(got.live, Some(true));
    assert_eq!(got.currency, "GBP");
    assert_eq!(got.price, Some(170.0));
    assert_eq!(got.adults, 1);
    assert_eq!(
        got.segments
            .iter()
            .map(|s| s.dest.as_str())
            .collect::<Vec<_>>(),
        ["ORD", "DEN"]
    );
    let usd = offer_in_usd(&got).unwrap();
    assert_eq!(usd.currency, "USD");
    assert!(usd.price.unwrap() > 170.0);
}

#[test]
fn duffel_missing_live_mode_is_not_sandbox() {
    let mut raw: serde_json::Value = serde_json::from_str(DUFFEL_OFFER).unwrap();
    raw.as_object_mut().unwrap().remove("live_mode");
    raw["passengers"] = json!([{"type": "adult"}, {"type": "adult"}]);
    let got = _one(
        &raw,
        &shop_req("JFK", "ORD", "2026-10-10"),
        "2026-09-08T12:00:00+00:00",
    )
    .unwrap();
    assert_eq!(got.live, None);
    assert!(!is_sandbox_offer(&got));
    assert_eq!(got.adults, 2);
    assert!(got.note.as_deref().unwrap().contains("live_mode=unknown"));
}

#[tokio::test]
async fn refresh_mock_converts_to_usd() {
    let settings = skiplagging::config::Settings::load();
    let den = skiplagging::providers::mock::jfk_ord_den("2026-10-10");
    let usd = refresh_priced_offer(&den, &settings, None).await.unwrap();
    assert_eq!(usd.currency, "USD");
    assert_eq!(usd.price, Some(175.0));
}

#[test]
fn duffel_two_slice_sets_return_date_without_req() {
    let raw = json!({
        "id": "off_rt",
        "live_mode": true,
        "total_amount": "430.00",
        "total_currency": "USD",
        "slices": [
            {
                "duration": "PT2H15M",
                "segments": [{
                    "origin": {"iata_code": "JFK"},
                    "destination": {"iata_code": "ORD"},
                    "marketing_carrier": {"iata_code": "AA"},
                    "operating_carrier": {"iata_code": "AA"},
                    "marketing_carrier_flight_number": "100",
                    "departing_at": "2026-10-10T08:00",
                    "arriving_at": "2026-10-10T10:15",
                    "duration": "PT2H15M"
                }]
            },
            {
                "departure_date": "2026-10-17",
                "duration": "PT2H15M",
                "segments": [{
                    "origin": {"iata_code": "ORD"},
                    "destination": {"iata_code": "JFK"},
                    "marketing_carrier": {"iata_code": "AA"},
                    "operating_carrier": {"iata_code": "AA"},
                    "marketing_carrier_flight_number": "101",
                    "departing_at": "2026-10-17T18:00",
                    "arriving_at": "2026-10-17T21:15",
                    "duration": "PT2H15M"
                }]
            }
        ]
    });
    let req = shop_req("JFK", "ORD", "2026-10-10");
    let got = _one(&raw, &req, "2026-09-08T12:00:00+00:00").unwrap();
    assert_eq!(got.return_date.as_deref(), Some("2026-10-17"));
    assert_eq!(got.outbound_end, Some(0));
    assert_eq!(ticketed_destination(&got), "ORD");
    assert_eq!(got.stops, 0);
}

#[test]
fn frankfurter_timeout_is_four_seconds() {
    assert_eq!(FETCH_TIMEOUT, std::time::Duration::from_secs(4));
}

#[test]
fn live_frankfurter_does_not_rank_on_fallback_for_missing_codes() {
    assert!(apply_frankfurter_body(&json!({
        "rates": { "GBP": 0.79, "EUR": 0.92 }
    })));
    assert!(skiplagging::fx::rates_are_live());
    let gbp = to_usd(Some(170.0), Some("GBP")).unwrap();
    assert!(gbp > 170.0 && gbp < 250.0);
    assert_eq!(to_usd(Some(100.0), Some("AED")), None);
}

#[test]
fn duffel_usd_quote_ranks_without_frankfurter() {
    skiplagging::fx::with_untrusted_rates(|| {
        assert_eq!(to_usd(Some(170.0), Some("GBP")), None);
        let mut offer = offer_full(
            "g1",
            Some(170.0),
            vec![Segment {
                origin: "LHR".into(),
                dest: "JFK".into(),
                carrier: "BA".into(),
                flight_number: "BA1".into(),
                dep: "2026-10-10T08:00".into(),
                arr: "2026-10-10T11:00".into(),
                duration_min: 180,
                rbd: "Y".into(),
                ..Segment::default()
            }],
            0,
            "GBP",
            "duffel",
            Some(true),
            Some(180),
        );
        offer.quoted_usd = Some(199.0);
        let usd = offer_in_usd(&offer).unwrap();
        assert_eq!(usd.currency, "USD");
        assert_eq!(usd.price, Some(199.0));
    });
}

#[test]
fn duffel_prefers_native_usd_amount_over_gbp_total() {
    let mut raw: serde_json::Value = serde_json::from_str(DUFFEL_OFFER).unwrap();
    raw["available_amount"] = json!("199.00");
    raw["available_currency"] = json!("USD");
    assert_eq!(_usd_quote(&raw), Some(199.0));
    let got = _one(
        &raw,
        &shop_req("JFK", "ORD", "2026-10-10"),
        "2026-09-08T12:00:00+00:00",
    )
    .unwrap();
    assert_eq!(got.currency, "GBP");
    assert_eq!(got.price, Some(170.0));
    assert_eq!(got.quoted_usd, Some(199.0));
    skiplagging::fx::with_untrusted_rates(|| {
        let usd = offer_in_usd(&got).unwrap();
        assert_eq!(usd.price, Some(199.0));
        assert_eq!(usd.currency, "USD");
    });
}

#[test]
fn duffel_tax_usd_is_not_a_fare() {
    let raw = json!({
        "total_amount": "170.00",
        "total_currency": "GBP",
        "tax_amount": "30.00",
        "tax_currency": "USD"
    });
    assert_eq!(_usd_quote(&raw), None);
}
