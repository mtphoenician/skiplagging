mod common;

use chrono::{Duration, SecondsFormat, TimeZone, Utc};
use skiplagging::engines::expiry::{offer_unexpired_at, unexpired_at};
use skiplagging::engines::shop::_gaps;
use skiplagging::models::{LiveTraffic, Offer};
use skiplagging::providers::duffel::offer_request_body;

use common::{offer, seg, shop_req};

fn priced(expires: Option<&str>) -> Offer {
    let mut o = offer(
        "o",
        Some(200.0),
        vec![seg("JFK", "ORD", "2026-10-10T08:00", "2026-10-10T10:00", "AA1")],
        0,
    );
    o.carrier = "AA".into();
    o.expires_at = expires.map(|s| s.to_string());
    o.live = Some(true);
    o
}

#[test]
fn missing_expires_at_is_kept() {
    let now = Utc.with_ymd_and_hms(2026, 9, 8, 12, 0, 0).unwrap();
    assert!(offer_unexpired_at(&priced(None), Some(now)));
}

#[test]
fn past_expires_at_is_dropped() {
    let now = Utc.with_ymd_and_hms(2026, 9, 8, 12, 0, 0).unwrap();
    let dead = priced(Some(
        &(now - Duration::minutes(1)).to_rfc3339_opts(SecondsFormat::Secs, true),
    ));
    let live = priced(Some(
        &(now + Duration::minutes(30)).to_rfc3339_opts(SecondsFormat::Secs, true),
    ));
    assert!(!offer_unexpired_at(&dead, Some(now)));
    assert!(offer_unexpired_at(&live, Some(now)));
    let kept = unexpired_at(&[dead, live.clone()], Some(now));
    assert_eq!(kept.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(), ["o"]);
    assert_eq!(kept[0].expires_at, live.expires_at);
}

#[test]
fn duffel_round_trip_request_has_two_slices() {
    let mut req = shop_req("JFK", "ORD", "2026-10-10");
    req.return_date = Some("2026-10-17".into());
    let slices = &offer_request_body(&req)["data"]["slices"];
    assert_eq!(slices.as_array().unwrap().len(), 2);
    assert_eq!(slices[0]["origin"], "JFK");
    assert_eq!(slices[0]["destination"], "ORD");
    assert_eq!(slices[1]["origin"], "ORD");
    assert_eq!(slices[1]["destination"], "JFK");
    assert_eq!(slices[1]["departure_date"], "2026-10-17");
}

#[test]
fn sandbox_gap_names_the_test_token() {
    let mut settings = skiplagging::config::Settings::load();
    settings.duffel_token = "duffel_test_abc".into();
    let traffic = LiveTraffic {
        airport: "BEY".into(),
        source: "opensky".into(),
        note: "OpenSky empty".into(),
        ..LiveTraffic::default()
    };
    let gaps = _gaps(&settings, None, None, &[], &[], &traffic, &[], None, false);
    assert!(gaps.iter().any(|g| g.contains("sandbox") && g.contains("duffel_test_")));
    assert!(!gaps.iter().any(|g| g.to_lowercase().contains("test inventory")));
}

#[test]
fn round_trip_gap_says_hidden_city_is_one_way() {
    let mut settings = skiplagging::config::Settings::load();
    settings.duffel_token = String::new();
    let traffic = LiveTraffic {
        airport: "JFK".into(),
        source: "opensky".into(),
        note: "OpenSky empty".into(),
        ..LiveTraffic::default()
    };
    let gaps = _gaps(
        &settings,
        None,
        None,
        &[priced(None)],
        &[],
        &traffic,
        &[],
        None,
        true,
    );
    assert!(gaps.iter().any(|g| g.to_lowercase().contains("one-way")));
}
