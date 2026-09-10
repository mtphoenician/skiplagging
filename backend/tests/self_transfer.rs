mod common;

use std::collections::{HashMap, HashSet};

use skiplagging::engines::self_transfer::{
    bridge_pairs, combine_at_hubs, connection_minutes, pick_transfer_hubs, stitch_chain,
};
use skiplagging::models::{Offer, Segment};

use common::{hs, offer_full};

fn seg(o: &str, d: &str, dep: &str, arr: &str, flight: &str, carrier: &str) -> Segment {
    let mut s = common::seg_carrier(o, d, dep, arr, flight, carrier);
    s.duration_min = 120;
    s
}

fn priced(oid: &str, price: f64, segs: Vec<Segment>, carrier: &str, currency: &str) -> Offer {
    let stops = (segs.len().saturating_sub(1)) as i32;
    let mut o = offer_full(
        oid,
        Some(price),
        segs,
        stops,
        currency,
        "duffel",
        Some(true),
        None,
    );
    o.carrier = carrier.into();
    o
}

fn codes(hs: &[&str]) -> Vec<String> {
    hs.iter().map(|s| (*s).to_string()).collect()
}

#[test]
fn pick_hubs_ranks_overlap_first() {
    let hubs = pick_transfer_hubs(
        "BEY",
        "CBR",
        &hs(&["KUL", "DXB"]),
        &hs(&["KUL", "SYD", "DPS"]),
        4,
        &[],
        &HashMap::new(),
    );
    assert_eq!(hubs[0], "KUL");
    assert!(!hubs.iter().any(|h| h == "BEY" || h == "CBR"));
}

#[test]
fn one_sided_overlap_uses_the_graph_not_a_hub_list() {
    let hubs = pick_transfer_hubs(
        "BEY",
        "CBR",
        &hs(&["DXB", "DOH"]),
        &hs(&["SYD", "MEL"]),
        4,
        &[],
        &HashMap::new(),
    );
    let allowed = hs(&["DXB", "DOH", "SYD", "MEL"]);
    assert!(hubs.iter().all(|h| allowed.contains(h)));
    assert!(!hubs.iter().any(|h| h == "BEY" || h == "CBR"));
}

#[test]
fn extra_hubs_fill_when_overlap_is_thin() {
    let extra = codes(&["SIN", "FRA", "KUL"]);
    let hubs = pick_transfer_hubs(
        "BEY",
        "CBR",
        &hs(&["DXB"]),
        &hs(&["SYD"]),
        4,
        &extra,
        &HashMap::new(),
    );
    assert!(hubs[0] == "DXB" || hubs[0] == "SYD");
    assert!(hubs.iter().any(|h| h == "SIN"));
}

#[test]
fn bridge_pairs_only_when_no_shared_hub() {
    assert_eq!(
        &bridge_pairs(
            &codes(&["KUL", "SIN", "DPS", "SYD"]),
            &hs(&["KUL", "SIN"]),
            &hs(&["DPS", "SYD"]),
            2
        )[..2],
        [("KUL".into(), "DPS".into()), ("KUL".into(), "SYD".into())]
    );
}

#[test]
fn bridge_pairs_skip_when_two_tickets_already_work() {
    assert!(bridge_pairs(
        &codes(&["KUL", "DXB"]),
        &hs(&["KUL"]),
        &hs(&["KUL", "SYD"]),
        2
    )
    .is_empty());
    assert!(bridge_pairs(
        &codes(&["ORD", "DTW", "ATL"]),
        &HashSet::new(),
        &HashSet::new(),
        2
    )
    .is_empty());
    assert_eq!(
        bridge_pairs(&codes(&["DXB", "DOH"]), &hs(&["DXB"]), &hs(&["DOH"]), 2),
        [("DXB".into(), "DOH".into())]
    );
}

#[test]
fn pick_hubs_zero_limit() {
    assert!(pick_transfer_hubs(
        "BEY",
        "CBR",
        &hs(&["KUL"]),
        &hs(&["SYD"]),
        0,
        &[],
        &HashMap::new()
    )
    .is_empty());
}

#[test]
fn stitch_two_tickets_three_stops_like_kayak() {
    let left = priced(
        "airasia",
        280.0,
        vec![
            seg(
                "BEY",
                "SHJ",
                "2026-11-19T12:15",
                "2026-11-19T16:20",
                "G9412",
                "G9",
            ),
            seg(
                "SHJ",
                "KUL",
                "2026-11-19T21:25",
                "2026-11-20T08:40",
                "D7107",
                "AK",
            ),
        ],
        "AK",
        "USD",
    );
    let right = priced(
        "virgin",
        437.0,
        vec![
            seg(
                "KUL",
                "DPS",
                "2026-11-20T15:35",
                "2026-11-20T18:30",
                "VA80",
                "VA",
            ),
            seg(
                "DPS",
                "CBR",
                "2026-11-20T21:25",
                "2026-11-21T07:00",
                "VA81",
                "VA",
            ),
        ],
        "VA",
        "USD",
    );
    let joined = stitch_chain(&[left, right], "CBR").unwrap();
    assert_eq!(joined.kind, "self-transfer");
    assert_eq!(joined.price, Some(717.0));
    assert_eq!(joined.stops, 3);
    assert_eq!(joined.self_transfer_airports, ["KUL"]);
    assert_eq!(
        joined.segments[..joined.segments.len() - 1]
            .iter()
            .map(|s| s.dest.as_str())
            .collect::<Vec<_>>(),
        ["SHJ", "KUL", "DPS"]
    );
    assert_eq!(joined.segments.last().unwrap().dest, "CBR");
    assert_eq!(joined.separate_tickets.len(), 2);
    assert!(joined
        .note
        .as_deref()
        .unwrap()
        .to_lowercase()
        .contains("not hidden-city"));
}

#[test]
fn stitch_three_tickets_two_self_transfers() {
    let a = priced(
        "t1",
        200.0,
        vec![seg(
            "BEY",
            "KUL",
            "2026-11-19T12:15",
            "2026-11-20T08:40",
            "AK1",
            "AK",
        )],
        "AK",
        "USD",
    );
    let b = priced(
        "t2",
        220.0,
        vec![seg(
            "KUL",
            "DPS",
            "2026-11-20T15:35",
            "2026-11-20T18:30",
            "AK2",
            "AK",
        )],
        "AK",
        "USD",
    );
    let c = priced(
        "t3",
        297.0,
        vec![seg(
            "DPS",
            "CBR",
            "2026-11-20T21:25",
            "2026-11-21T07:00",
            "VA81",
            "VA",
        )],
        "VA",
        "USD",
    );
    let joined = stitch_chain(&[a, b, c], "CBR").unwrap();
    assert_eq!(joined.price, Some(717.0));
    assert_eq!(joined.self_transfer_airports, ["KUL", "DPS"]);
    assert_eq!(joined.stops, 2);
}

#[test]
fn stitch_rejects_tight_self_transfer() {
    let left = priced(
        "a",
        100.0,
        vec![seg(
            "BEY",
            "KUL",
            "2026-11-19T08:00",
            "2026-11-19T20:00",
            "AK1",
            "AK",
        )],
        "AK",
        "USD",
    );
    let right = priced(
        "b",
        100.0,
        vec![seg(
            "KUL",
            "CBR",
            "2026-11-19T21:00",
            "2026-11-20T10:00",
            "VA1",
            "VA",
        )],
        "VA",
        "USD",
    );
    assert!(stitch_chain(&[left, right], "CBR").is_none());
}

#[test]
fn stitch_rejects_inbound_that_already_visits_b() {
    let left = priced(
        "via-cbr",
        400.0,
        vec![
            seg(
                "BEY",
                "CBR",
                "2026-11-19T08:00",
                "2026-11-20T10:00",
                "QF1",
                "QF",
            ),
            seg(
                "CBR",
                "SYD",
                "2026-11-20T12:00",
                "2026-11-20T13:00",
                "QF2",
                "QF",
            ),
        ],
        "QF",
        "USD",
    );
    let right = priced(
        "back",
        50.0,
        vec![seg(
            "SYD",
            "CBR",
            "2026-11-20T18:00",
            "2026-11-20T19:30",
            "VA9",
            "VA",
        )],
        "VA",
        "USD",
    );
    assert!(stitch_chain(&[left, right], "CBR").is_none());
}

#[test]
fn stitch_rejects_mixed_currency() {
    let left = priced(
        "a",
        100.0,
        vec![seg(
            "BEY",
            "KUL",
            "2026-11-19T08:00",
            "2026-11-19T20:00",
            "AK1",
            "AK",
        )],
        "AK",
        "USD",
    );
    let right = priced(
        "b",
        100.0,
        vec![seg(
            "KUL",
            "CBR",
            "2026-11-20T08:00",
            "2026-11-20T22:00",
            "VA1",
            "VA",
        )],
        "VA",
        "EUR",
    );
    assert!(stitch_chain(&[left, right], "CBR").is_none());
}

#[test]
fn stitch_rejects_cabin_mismatch() {
    let left = priced(
        "a",
        100.0,
        vec![seg(
            "BEY",
            "KUL",
            "2026-11-19T08:00",
            "2026-11-19T20:00",
            "AK1",
            "AK",
        )],
        "AK",
        "USD",
    );
    let mut right = priced(
        "b",
        100.0,
        vec![seg(
            "KUL",
            "CBR",
            "2026-11-20T08:00",
            "2026-11-20T22:00",
            "VA1",
            "VA",
        )],
        "VA",
        "USD",
    );
    right.cabin = "BUSINESS".into();
    assert!(stitch_chain(&[left, right], "CBR").is_none());
}

#[test]
fn combine_keeps_cheapest() {
    let left = priced(
        "a",
        100.0,
        vec![seg(
            "BEY",
            "KUL",
            "2026-11-19T08:00",
            "2026-11-19T20:00",
            "AK1",
            "AK",
        )],
        "AK",
        "USD",
    );
    let cheap = priced(
        "b1",
        200.0,
        vec![seg(
            "KUL",
            "CBR",
            "2026-11-20T08:00",
            "2026-11-20T22:00",
            "VA1",
            "VA",
        )],
        "VA",
        "USD",
    );
    let dear = priced(
        "b2",
        900.0,
        vec![seg(
            "KUL",
            "CBR",
            "2026-11-20T09:00",
            "2026-11-20T23:00",
            "VA2",
            "VA",
        )],
        "VA",
        "USD",
    );
    let out = combine_at_hubs(
        &HashMap::from([("KUL".into(), vec![left])]),
        &HashMap::from([("KUL".into(), vec![dear, cheap])]),
        &hs(&["CBR"]),
        None,
        8,
    );
    assert_eq!(out[0].price, Some(300.0));
}

#[test]
fn stitch_rejects_six_flights() {
    let left = priced(
        "a",
        200.0,
        vec![
            seg(
                "BEY",
                "SHJ",
                "2026-11-19T12:15",
                "2026-11-19T16:20",
                "G91",
                "AK",
            ),
            seg(
                "SHJ",
                "KUL",
                "2026-11-19T21:25",
                "2026-11-20T08:40",
                "D71",
                "AK",
            ),
            seg(
                "KUL",
                "SIN",
                "2026-11-20T11:00",
                "2026-11-20T12:30",
                "AK3",
                "AK",
            ),
        ],
        "AK",
        "USD",
    );
    let right = priced(
        "b",
        200.0,
        vec![
            seg(
                "SIN",
                "DPS",
                "2026-11-20T16:00",
                "2026-11-20T18:30",
                "VA1",
                "VA",
            ),
            seg(
                "DPS",
                "SYD",
                "2026-11-20T21:25",
                "2026-11-21T04:00",
                "VA2",
                "VA",
            ),
            seg(
                "SYD",
                "CBR",
                "2026-11-21T08:00",
                "2026-11-21T09:00",
                "VA3",
                "VA",
            ),
        ],
        "VA",
        "USD",
    );
    assert!(stitch_chain(&[left, right], "CBR").is_none());
}

#[test]
fn combine_keeps_three_ticket_only_when_cheaper() {
    let left = priced(
        "a",
        100.0,
        vec![seg(
            "BEY",
            "KUL",
            "2026-11-19T08:00",
            "2026-11-19T20:00",
            "AK1",
            "AK",
        )],
        "AK",
        "USD",
    );
    let two_right = priced(
        "b",
        200.0,
        vec![seg(
            "KUL",
            "CBR",
            "2026-11-20T08:00",
            "2026-11-20T22:00",
            "VA1",
            "VA",
        )],
        "VA",
        "USD",
    );
    let mid = priced(
        "m",
        50.0,
        vec![seg(
            "KUL",
            "DPS",
            "2026-11-20T08:00",
            "2026-11-20T12:00",
            "AK2",
            "AK",
        )],
        "AK",
        "USD",
    );
    let dear_last = priced(
        "c",
        400.0,
        vec![seg(
            "DPS",
            "CBR",
            "2026-11-20T16:00",
            "2026-11-21T06:00",
            "VA9",
            "VA",
        )],
        "VA",
        "USD",
    );
    let mids = HashMap::from([(("KUL".into(), "DPS".into()), vec![mid.clone()])]);
    let out = combine_at_hubs(
        &HashMap::from([("KUL".into(), vec![left.clone()])]),
        &HashMap::from([
            ("KUL".into(), vec![two_right.clone()]),
            ("DPS".into(), vec![dear_last]),
        ]),
        &hs(&["CBR"]),
        Some(&mids),
        8,
    );
    assert!(!out.is_empty());
    assert!(out.iter().all(|o| o.separate_tickets.len() == 2));
    let cheap_last = priced(
        "c2",
        80.0,
        vec![seg(
            "DPS",
            "CBR",
            "2026-11-20T16:00",
            "2026-11-21T06:00",
            "VA8",
            "VA",
        )],
        "VA",
        "USD",
    );
    let cheaper = combine_at_hubs(
        &HashMap::from([("KUL".into(), vec![left])]),
        &HashMap::from([
            ("KUL".into(), vec![two_right]),
            ("DPS".into(), vec![cheap_last]),
        ]),
        &hs(&["CBR"]),
        Some(&mids),
        8,
    );
    assert_eq!(cheaper[0].price, Some(230.0));
    assert_eq!(cheaper[0].separate_tickets.len(), 3);
}

#[test]
fn connection_minutes_missing_times() {
    assert!(connection_minutes("", "2026-11-20T08:00").is_none());
}
