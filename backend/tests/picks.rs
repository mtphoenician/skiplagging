mod common;

use std::collections::HashSet;

use skiplagging::engines::risk::assess;
use skiplagging::engines::shop::{
    _dedupe_itineraries, _drop_sandbox, _hidden_if_cheaper, _keep_on_route, _prefer_real_carriers,
    _route_pairs, _via_b, layover_minutes, pick_best, pick_honest, shop_tokens,
};
use skiplagging::models::{Offer, Segment};
use skiplagging::providers::duffel::{_dur, offer_request_body, DUFFEL_MAX_CONNECTIONS};
use skiplagging::providers::mock::jfk_ord_roundtrip;

use common::{city, hc_match, offer, offer_full, seg, shop_req};

fn connect(oid: &str, price: f64, a: Segment, b: Segment, stops: i32) -> Offer {
    offer(oid, Some(price), vec![a, b], stops)
}

#[test]
fn pick_prefers_cheapest_nonstop_over_dearer_connecting() {
    let cheap = offer(
        "n1",
        Some(200.0),
        vec![seg(
            "LHR",
            "JFK",
            "2026-10-22T08:00",
            "2026-10-22T11:00",
            "BA1",
        )],
        0,
    );
    let dear = offer(
        "n2",
        Some(400.0),
        vec![seg(
            "LHR",
            "JFK",
            "2026-10-22T09:00",
            "2026-10-22T12:00",
            "BA1",
        )],
        0,
    );
    let connect = connect(
        "c1",
        250.0,
        seg("LHR", "DUB", "2026-10-22T08:00", "2026-10-22T09:00", "BA2"),
        seg("DUB", "JFK", "2026-10-22T10:00", "2026-10-22T13:00", "BA3"),
        1,
    );
    let pick = pick_honest(&[dear, cheap], &[connect], None).unwrap();
    assert_eq!(pick.kind, "nonstop");
    assert_eq!(pick.offer.id, "n1");
    assert_eq!(pick.reason, "Best — cheapest nonstop");
}

#[test]
fn pick_prefers_cheaper_connecting_over_nonstop() {
    let nonstop = offer(
        "n1",
        Some(400.0),
        vec![seg(
            "LHR",
            "JFK",
            "2026-10-22T08:00",
            "2026-10-22T11:00",
            "BA1",
        )],
        0,
    );
    let connect = connect(
        "c1",
        180.0,
        seg("LHR", "DUB", "2026-10-22T08:00", "2026-10-22T09:00", "BA2"),
        seg("DUB", "JFK", "2026-10-22T10:00", "2026-10-22T13:00", "BA3"),
        1,
    );
    let pick = pick_honest(&[nonstop], &[connect], None).unwrap();
    assert_eq!(pick.kind, "connecting");
    assert_eq!(pick.offer.id, "c1");
}

#[test]
fn pick_uses_price_then_shortest_layover() {
    let long_wait = connect(
        "c1",
        180.0,
        seg("LHR", "DUB", "2026-10-22T08:00", "2026-10-22T09:00", "BA2"),
        seg("DUB", "JFK", "2026-10-22T14:00", "2026-10-22T17:00", "BA3"),
        1,
    );
    let short_wait = connect(
        "c2",
        220.0,
        seg("LHR", "AMS", "2026-10-22T08:00", "2026-10-22T09:00", "BA4"),
        seg("AMS", "JFK", "2026-10-22T10:00", "2026-10-22T13:00", "BA5"),
        1,
    );
    assert_eq!(layover_minutes(&long_wait), 300);
    assert_eq!(layover_minutes(&short_wait), 60);
    let pick = pick_honest(&[], &[long_wait, short_wait], None).unwrap();
    assert_eq!(pick.kind, "connecting");
    assert_eq!(pick.offer.id, "c1");
    assert_eq!(pick.layover_min, 300);
}

#[test]
fn assess_none_local_price_does_not_raise() {
    let local = offer(
        "n",
        None,
        vec![seg(
            "JFK",
            "ORD",
            "2026-10-10T08:00",
            "2026-10-10T10:00",
            "BA1",
        )],
        0,
    );
    let through = connect(
        "h",
        100.0,
        seg("JFK", "ORD", "2026-10-10T08:00", "2026-10-10T10:00", "BA1"),
        seg("ORD", "DEN", "2026-10-10T11:00", "2026-10-10T13:00", "BA1"),
        1,
    );
    let risk = assess(&local, &through, "DEN", "ORD", "", "");
    assert!(!risk.items.is_empty());
}

#[test]
fn international_docs_risk_when_countries_differ() {
    let local = offer(
        "n",
        Some(400.0),
        vec![seg(
            "JFK",
            "LHR",
            "2026-10-10T08:00",
            "2026-10-10T20:00",
            "BA1",
        )],
        0,
    );
    let through = connect(
        "h",
        200.0,
        seg("JFK", "LHR", "2026-10-10T08:00", "2026-10-10T20:00", "BA1"),
        seg("LHR", "CDG", "2026-10-10T22:00", "2026-10-10T23:20", "BA2"),
        1,
    );
    let domestic = assess(&local, &through, "DEN", "ORD", "US", "US");
    let intl = assess(&local, &through, "CDG", "LHR", "US", "GB");
    assert!(intl.items.iter().any(|i| i.id == "documents"));
    assert!(!domestic.items.iter().any(|i| i.id == "documents"));
    assert!(intl.score < domestic.score);
    assert!(intl.expected_disruption_cost > domestic.expected_disruption_cost);
}

#[test]
fn layover_minutes_round_trip_ignores_the_stay_at_b() {
    let rt = jfk_ord_roundtrip("2026-10-10", "2026-10-17");
    assert_eq!(layover_minutes(&rt), 0);
}

#[test]
fn pick_same_price_prefers_shorter_layover() {
    let long_wait = connect(
        "c1",
        200.0,
        seg("LHR", "DUB", "2026-10-22T08:00", "2026-10-22T09:00", "BA2"),
        seg("DUB", "JFK", "2026-10-22T14:00", "2026-10-22T17:00", "BA3"),
        1,
    );
    let short_wait = connect(
        "c2",
        200.0,
        seg("LHR", "AMS", "2026-10-22T08:00", "2026-10-22T09:00", "BA4"),
        seg("AMS", "JFK", "2026-10-22T10:00", "2026-10-22T13:00", "BA5"),
        1,
    );
    let pick = pick_honest(&[], &[long_wait, short_wait], None).unwrap();
    assert_eq!(pick.offer.id, "c2");
    assert_eq!(pick.layover_min, 60);
}

#[test]
fn pick_converts_foreign_currency_to_usd() {
    skiplagging::fx::mark_rates_trusted();
    let usd = offer(
        "n1",
        Some(400.0),
        vec![seg(
            "LHR",
            "JFK",
            "2026-10-22T08:00",
            "2026-10-22T11:00",
            "BA1",
        )],
        0,
    );
    let gbp = offer_full(
        "n2",
        Some(80.0),
        vec![seg(
            "LHR",
            "JFK",
            "2026-10-22T09:00",
            "2026-10-22T12:00",
            "BA1",
        )],
        0,
        "GBP",
        "duffel",
        Some(true),
        None,
    );
    let pick = pick_honest(&[usd, gbp], &[], None).unwrap();
    assert_eq!(pick.offer.id, "n2");
    assert_eq!(pick.offer.currency, "USD");
    assert!(pick.offer.price.unwrap() < 400.0);
}

#[test]
fn untrusted_gbp_is_not_ranked_on_fallback() {
    skiplagging::fx::with_untrusted_rates(|| {
        let usd = offer(
            "n1",
            Some(240.0),
            vec![seg(
                "JFK",
                "ORD",
                "2026-10-22T08:00",
                "2026-10-22T10:00",
                "AA1",
            )],
            0,
        );
        let gbp = offer_full(
            "n2",
            Some(170.0),
            vec![seg(
                "LHR",
                "JFK",
                "2026-10-22T09:00",
                "2026-10-22T12:00",
                "BA1",
            )],
            0,
            "GBP",
            "duffel",
            Some(true),
            None,
        );
        let pick = pick_honest(&[usd, gbp], &[], Some("USD")).unwrap();
        assert_eq!(pick.offer.id, "n1");
        assert_eq!(pick.offer.currency, "USD");
        assert_eq!(pick.offer.price, Some(240.0));
    });
}

#[test]
fn pick_uses_requested_currency_when_present() {
    skiplagging::fx::mark_rates_trusted();
    let usd = offer(
        "n1",
        Some(400.0),
        vec![seg(
            "LHR",
            "JFK",
            "2026-10-22T08:00",
            "2026-10-22T11:00",
            "BA1",
        )],
        0,
    );
    let gbp = offer_full(
        "n2",
        Some(80.0),
        vec![seg(
            "LHR",
            "JFK",
            "2026-10-22T09:00",
            "2026-10-22T12:00",
            "BA1",
        )],
        0,
        "GBP",
        "duffel",
        Some(true),
        None,
    );
    let pick = pick_honest(&[usd, gbp], &[], Some("USD")).unwrap();
    assert_eq!(pick.offer.id, "n2");
    assert_eq!(pick.offer.currency, "USD");
}

#[test]
fn hidden_only_when_cheaper_than_honest_pick() {
    let honest = pick_honest(
        &[offer(
            "n1",
            Some(300.0),
            vec![seg(
                "LHR",
                "BOS",
                "2026-10-22T08:00",
                "2026-10-22T11:00",
                "BA1",
            )],
            0,
        )],
        &[],
        None,
    )
    .unwrap();
    let cheap_through = connect(
        "h1",
        220.0,
        seg("LHR", "BOS", "2026-10-22T08:00", "2026-10-22T11:00", "BA9"),
        seg("BOS", "JFK", "2026-10-22T12:00", "2026-10-22T13:00", "BA10"),
        1,
    );
    let mut dear_through = cheap_through.clone();
    dear_through.id = "h2".into();
    dear_through.price = Some(350.0);
    let cheap_match = hc_match("m1", cheap_through, honest.offer.clone(), 80.0);
    let dear_match = hc_match("m2", dear_through, honest.offer.clone(), -50.0);
    assert_eq!(
        _hidden_if_cheaper(&[cheap_match.clone()], Some(&honest))
            .unwrap()
            .id,
        "m1"
    );
    assert!(_hidden_if_cheaper(&[dear_match], Some(&honest)).is_none());
    assert!(_hidden_if_cheaper(&[cheap_match], None).is_none());
    assert!(pick_honest(&[], &[], None).is_none());
}

#[test]
fn hidden_shown_when_converted_price_is_cheaper() {
    skiplagging::fx::mark_rates_trusted();
    let honest = pick_honest(
        &[offer(
            "n1",
            Some(300.0),
            vec![seg(
                "LHR",
                "BOS",
                "2026-10-22T08:00",
                "2026-10-22T11:00",
                "BA1",
            )],
            0,
        )],
        &[],
        None,
    )
    .unwrap();
    let gbp_through = offer_full(
        "h1",
        Some(80.0),
        vec![
            seg("LHR", "BOS", "2026-10-22T08:00", "2026-10-22T11:00", "BA9"),
            seg("BOS", "JFK", "2026-10-22T12:00", "2026-10-22T13:00", "BA10"),
        ],
        1,
        "GBP",
        "duffel",
        Some(true),
        None,
    );
    assert!(_hidden_if_cheaper(
        &[hc_match("m1", gbp_through, honest.offer.clone(), 220.0)],
        Some(&honest)
    )
    .is_some());
}

#[test]
fn hidden_not_shown_when_converted_price_is_not_cheaper() {
    skiplagging::fx::mark_rates_trusted();
    let honest = pick_honest(
        &[offer(
            "n1",
            Some(300.0),
            vec![seg(
                "LHR",
                "BOS",
                "2026-10-22T08:00",
                "2026-10-22T11:00",
                "BA1",
            )],
            0,
        )],
        &[],
        None,
    )
    .unwrap();
    let gbp_through = offer_full(
        "h1",
        Some(800.0),
        vec![
            seg("LHR", "BOS", "2026-10-22T08:00", "2026-10-22T11:00", "BA9"),
            seg("BOS", "JFK", "2026-10-22T12:00", "2026-10-22T13:00", "BA10"),
        ],
        1,
        "GBP",
        "duffel",
        Some(true),
        None,
    );
    assert!(_hidden_if_cheaper(
        &[hc_match("m1", gbp_through, honest.offer.clone(), -500.0)],
        Some(&honest)
    )
    .is_none());
}

#[test]
fn via_b_allows_any_intermediate_stop() {
    let through = connect(
        "h1",
        220.0,
        seg("LHR", "BOS", "2026-10-22T08:00", "2026-10-22T11:00", "BA9"),
        seg("BOS", "JFK", "2026-10-22T12:00", "2026-10-22T13:00", "BA10"),
        1,
    );
    assert!(_via_b(&through, "LHR", "BOS", "JFK"));
    assert!(_via_b(
        &through,
        HashSet::from(["LHR", "LGW"]),
        HashSet::from(["BOS", "BDL"]),
        "JFK"
    ));
    assert!(!_via_b(&through, "LHR", "JFK", "BOS"));
    assert!(!_via_b(&through, "LGW", "BOS", "JFK"));
    let two_stop = offer(
        "h2",
        Some(190.0),
        vec![
            seg("LHR", "DUB", "2026-10-22T08:00", "2026-10-22T09:00", "BA1"),
            seg("DUB", "BOS", "2026-10-22T10:00", "2026-10-22T13:00", "BA2"),
            seg("BOS", "JFK", "2026-10-22T14:00", "2026-10-22T15:00", "BA3"),
        ],
        2,
    );
    assert!(_via_b(&two_stop, "LHR", "BOS", "JFK"));
    assert!(!_via_b(&two_stop, "LHR", "DUB", "BOS"));
    let nonstop = offer(
        "n1",
        Some(300.0),
        vec![seg(
            "LHR",
            "JFK",
            "2026-10-22T08:00",
            "2026-10-22T11:00",
            "BA1",
        )],
        0,
    );
    assert!(!_via_b(&nonstop, "LHR", "BOS", "JFK"));
}

#[test]
fn keep_on_route_drops_other_airports() {
    let paris_london = offer(
        "n1",
        Some(110.0),
        vec![seg(
            "CDG",
            "LGW",
            "2026-09-19T10:00",
            "2026-09-19T10:55",
            "BA1",
        )],
        0,
    );
    let junk = offer(
        "n2",
        Some(41.0),
        vec![seg(
            "VIY",
            "SEN",
            "2026-09-19T13:56",
            "2026-09-19T13:57",
            "BA1",
        )],
        0,
    );
    let kept = _keep_on_route(
        &[paris_london, junk],
        &HashSet::from(["CDG".into(), "ORY".into(), "PAR".into()]),
        &HashSet::from(["LGW".into(), "LHR".into(), "LON".into()]),
    );
    assert_eq!(
        kept.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(),
        ["n1"]
    );
}

#[test]
fn codeshare_keeps_one_itinerary() {
    let ib = offer(
        "ib",
        Some(41.0),
        vec![seg(
            "CDG",
            "LHR",
            "2026-09-19T13:56",
            "2026-09-19T14:56",
            "IB1",
        )],
        0,
    );
    let mut ba = ib.clone();
    ba.id = "ba".into();
    ba.price = Some(42.0);
    ba.carrier = "BA".into();
    ba.first_flight = "BA1".into();
    ba.segments[0].flight_number = "BA1".into();
    ba.segments[0].carrier = "BA".into();
    let kept = _dedupe_itineraries(&[ib, ba]);
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].id, "ib");
}

#[test]
fn drop_sandbox_duffel_keeps_live_and_mock() {
    let mut fake = offer_full(
        "ba-test",
        Some(71.0),
        vec![seg(
            "JFK",
            "ORD",
            "2026-10-10T06:58",
            "2026-10-10T08:08",
            "BA0105",
        )],
        0,
        "GBP",
        "duffel",
        Some(false),
        None,
    );
    fake.note = Some("Duffel offer. live_mode=False.".into());
    let mut live = offer(
        "ba-live",
        Some(410.0),
        vec![seg(
            "JFK",
            "LHR",
            "2026-10-10T18:00",
            "2026-10-11T06:10",
            "BA0112",
        )],
        0,
    );
    live.live = Some(true);
    let mock = offer_full(
        "aa",
        Some(240.0),
        vec![seg(
            "JFK",
            "ORD",
            "2026-10-10T08:00",
            "2026-10-10T10:15",
            "AA100",
        )],
        0,
        "USD",
        "mock",
        None,
        None,
    );
    let kept = _drop_sandbox(&[fake, live, mock]);
    assert_eq!(
        kept.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(),
        ["ba-live", "aa"]
    );
}

#[test]
fn prefer_real_carriers_skips_duffel_airways() {
    let mut zz = offer(
        "zz",
        Some(41.0),
        vec![seg(
            "CDG",
            "LHR",
            "2026-09-19T13:56",
            "2026-09-19T14:56",
            "ZZ1",
        )],
        0,
    );
    zz.carrier = "ZZ".into();
    let ba = offer(
        "ba",
        Some(110.0),
        vec![seg(
            "CDG",
            "LHR",
            "2026-09-19T10:00",
            "2026-09-19T11:00",
            "BA1",
        )],
        0,
    );
    let kept = _prefer_real_carriers(vec![zz, ba]);
    assert_eq!(
        kept.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(),
        ["ba"]
    );
}

#[test]
fn duffel_offer_request_uses_supplier_max_connections() {
    let connecting = offer_request_body(&shop_req("JFK", "DEN", "2026-11-19"));
    assert_eq!(
        connecting["data"]["max_connections"],
        DUFFEL_MAX_CONNECTIONS
    );
    assert_eq!(DUFFEL_MAX_CONNECTIONS, 2);
    assert!(connecting["data"]["slices"][0]
        .get("max_connections")
        .is_none());
    let mut ns = shop_req("JFK", "ORD", "2026-11-19");
    ns.nonstop = true;
    let nonstop = offer_request_body(&ns);
    assert_eq!(nonstop["data"]["max_connections"], 0);
}

#[test]
fn duffel_duration_parses_overnight() {
    assert_eq!(_dur("PT2H30M"), 150);
    assert_eq!(_dur("P1DT30M"), 1470);
    assert_eq!(_dur("P1DT2H30M"), 1590);
    assert_eq!(_dur(""), 0);
    assert_eq!(_dur("PT2H30.5M"), 0);
}

#[test]
fn best_is_nonstop_when_cheapest_is_connecting() {
    let nonstop = offer_full(
        "n1",
        Some(400.0),
        vec![seg(
            "LHR",
            "JFK",
            "2026-10-22T08:00",
            "2026-10-22T11:00",
            "BA1",
        )],
        0,
        "USD",
        "duffel",
        Some(true),
        Some(420),
    );
    let connect = offer_full(
        "c1",
        Some(180.0),
        vec![
            seg("LHR", "DUB", "2026-10-22T08:00", "2026-10-22T09:00", "BA2"),
            seg("DUB", "JFK", "2026-10-22T10:00", "2026-10-22T13:00", "BA3"),
        ],
        1,
        "USD",
        "duffel",
        Some(true),
        Some(600),
    );
    let cheap = pick_honest(&[nonstop.clone()], &[connect.clone()], None).unwrap();
    let best = pick_best(&[nonstop], &[connect], Some(&cheap)).unwrap();
    assert_eq!(cheap.offer.id, "c1");
    assert_eq!(best.offer.id, "n1");
    assert_eq!(best.reason, "Best — cheapest nonstop");
}

#[test]
fn best_omitted_when_cheapest_is_already_the_nonstop() {
    let cheap_ns = offer_full(
        "n1",
        Some(200.0),
        vec![seg(
            "LHR",
            "JFK",
            "2026-10-22T08:00",
            "2026-10-22T11:00",
            "BA1",
        )],
        0,
        "USD",
        "duffel",
        Some(true),
        Some(420),
    );
    let dear_ns = offer_full(
        "n2",
        Some(350.0),
        vec![seg(
            "LHR",
            "JFK",
            "2026-10-22T12:00",
            "2026-10-22T15:00",
            "BA1",
        )],
        0,
        "USD",
        "duffel",
        Some(true),
        Some(420),
    );
    let cheap = pick_honest(&[cheap_ns.clone(), dear_ns.clone()], &[], None).unwrap();
    assert!(pick_best(&[cheap_ns, dear_ns], &[], Some(&cheap)).is_none());
}

#[test]
fn best_stays_cheapest_nonstop_not_faster_dearer() {
    let slow = offer_full(
        "n1",
        Some(200.0),
        vec![seg(
            "LHR",
            "JFK",
            "2026-10-22T08:00",
            "2026-10-22T16:00",
            "BA1",
        )],
        0,
        "USD",
        "duffel",
        Some(true),
        Some(480),
    );
    let fast = offer_full(
        "n2",
        Some(220.0),
        vec![seg(
            "LHR",
            "JFK",
            "2026-10-22T09:00",
            "2026-10-22T15:00",
            "BA1",
        )],
        0,
        "USD",
        "duffel",
        Some(true),
        Some(360),
    );
    let cheap = pick_honest(&[slow.clone(), fast.clone()], &[], None).unwrap();
    assert_eq!(cheap.offer.id, "n1");
    assert!(pick_best(&[slow, fast], &[], Some(&cheap)).is_none());
}

#[test]
fn city_shop_tokens_include_member_airports() {
    let paris = city("PAR", &["CDG", "ORY", "BVA"]);
    let london = city("LON", &["LHR", "LGW", "STN", "LCY"]);
    assert_eq!(shop_tokens(&paris), ["PAR", "CDG", "ORY", "BVA"]);
    assert_eq!(shop_tokens(&london), ["LON", "LHR", "LGW", "STN"]);
    let pairs = _route_pairs(&shop_tokens(&paris), &shop_tokens(&london));
    assert_eq!(pairs[0], ("PAR".into(), "LON".into()));
    assert!(pairs.contains(&("CDG".into(), "LHR".into())));
    assert!(pairs.contains(&("CDG".into(), "LGW".into())));
    assert!(pairs.contains(&("ORY".into(), "LGW".into())));
    assert!(pairs[1..].iter().all(|(o, d)| o != "PAR" || d != "LON"));
}

#[test]
fn best_is_fewest_stops_when_cheapest_connects_more() {
    let two_stop = offer_full(
        "c2",
        Some(150.0),
        vec![
            seg("LHR", "DUB", "2026-10-22T08:00", "2026-10-22T09:00", "BA2"),
            seg("DUB", "BOS", "2026-10-22T10:00", "2026-10-22T13:00", "BA3"),
            seg("BOS", "JFK", "2026-10-22T14:00", "2026-10-22T15:00", "BA6"),
        ],
        2,
        "USD",
        "duffel",
        Some(true),
        Some(420),
    );
    let one_stop = offer_full(
        "c1",
        Some(200.0),
        vec![
            seg("LHR", "AMS", "2026-10-22T08:00", "2026-10-22T09:00", "BA4"),
            seg("AMS", "JFK", "2026-10-22T10:00", "2026-10-22T13:00", "BA5"),
        ],
        1,
        "USD",
        "duffel",
        Some(true),
        Some(300),
    );
    let cheap = pick_honest(&[], &[two_stop.clone(), one_stop.clone()], None).unwrap();
    let best = pick_best(&[], &[two_stop, one_stop], Some(&cheap)).unwrap();
    assert_eq!(cheap.offer.id, "c2");
    assert_eq!(best.offer.id, "c1");
    assert_eq!(best.reason, "Best — cheapest one stop");
}
