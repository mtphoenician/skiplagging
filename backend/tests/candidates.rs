mod common;

use std::collections::{HashMap, HashSet};

use chrono::{Duration, TimeZone, Utc};
use skiplagging::budget::{provider_score, start_ledger, timed_shop, Ledger, LEDGER};
use skiplagging::engines::candidates::{
    expected_value, freshness, hub_probabilities, is_dead, rank_candidates, score_candidate,
    select_candidates, Candidate, RouteStat, COLD_START_PROBES, DEAD_AFTER_CHECKS, WEIGHTS,
};
use skiplagging::engines::learn::{build_batch, merge_savings, summarize_savings};
use skiplagging::models::{Offer, Segment};
use skiplagging::providers::mock::jfk_ord_roundtrip;

use common::{hs, offer_full, seg_carrier};

const NOW_Y: i32 = 2026;
const NOW_M: u32 = 9;
const NOW_D: u32 = 8;

fn now() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(NOW_Y, NOW_M, NOW_D, 12, 0, 0).unwrap()
}

fn seg(o: &str, d: &str, flight: &str, carrier: &str) -> Segment {
    let mut s = seg_carrier(o, d, "2026-10-10T08:00", "2026-10-10T10:00", flight, carrier);
    s.duration_min = 120;
    s
}

fn priced(oid: &str, price: f64, segs: Vec<Segment>, currency: &str, source: &str) -> Offer {
    let mut o = offer_full(
        oid,
        Some(price),
        segs,
        0,
        currency,
        source,
        Some(true),
        None,
    );
    o.stops = (o.segments.len().saturating_sub(1)) as i32;
    o.kind = if o.stops == 0 {
        "nonstop".into()
    } else {
        "connecting".into()
    };
    o.expires_at = Some("2026-09-08T12:30:00Z".into());
    o
}

fn jfk_ord_batch(probed: &[&str]) -> skiplagging::engines::learn::LearnBatch {
    let honest = priced("aa", 240.0, vec![seg("JFK", "ORD", "AA100", "AA")], "USD", "duffel");
    let den = priced(
        "den",
        170.0,
        vec![seg("JFK", "ORD", "UA200", "UA"), seg("ORD", "DEN", "UA300", "UA")],
        "USD",
        "duffel",
    );
    let sea = priced(
        "sea",
        300.0,
        vec![seg("JFK", "ORD", "AS400", "AS"), seg("ORD", "SEA", "AS500", "AS")],
        "USD",
        "duffel",
    );
    let lax = priced(
        "lax",
        150.0,
        vec![seg("JFK", "DFW", "AA600", "AA"), seg("DFW", "LAX", "AA700", "AA")],
        "USD",
        "duffel",
    );
    let probed: Vec<String> = probed.iter().map(|s| (*s).to_string()).collect();
    build_batch(
        &[honest, den, sea, lax],
        &hs(&["JFK"]),
        &hs(&["ORD"]),
        Some(240.0),
        Some("USD"),
        &probed,
        "2026-10-10",
        1,
        "ECONOMY",
        None,
        None,
        Some(now()),
    )
}

fn stat(
    origin: &str,
    intended: &str,
    ticketed: &str,
    observations: i32,
    successful_connections: i32,
    cheaper: i32,
) -> RouteStat {
    RouteStat {
        origin: origin.into(),
        intended: intended.into(),
        ticketed: ticketed.into(),
        observations,
        successful_connections,
        cheaper_than_direct_count: cheaper,
        median_saving: 0.0,
        average_saving_percent: 0.0,
        last_success_at: None,
        last_cheaper_at: None,
    }
}

#[test]
fn weights_sum_to_one() {
    let sum: f64 = WEIGHTS.values().sum();
    assert!((sum - 1.0).abs() < 1e-9);
}

#[test]
fn score_formula_matches_spec_weights() {
    let mut st = stat("JFK", "ORD", "DEN", 100, 71, 50);
    st.median_saving = 87.0;
    st.average_saving_percent = 30.0;
    st.last_cheaper_at = Some(now());
    let (score, parts) = score_candidate(Some(&st), 1.0, 0.9, now());
    assert!((parts["connection"] - (71.5 / 101.0)).abs() < 1e-4);
    assert!((parts["savings_probability"] - (50.5 / 101.0)).abs() < 1e-4);
    assert_eq!(parts["expected_savings"], 0.3);
    assert_eq!(parts["freshness"], 1.0);
    let expected = 0.30 * parts["connection"]
        + 0.25 * parts["savings_probability"]
        + 0.20 * 0.3
        + 0.10 * 1.0
        + 0.10 * 1.0
        + 0.05 * 0.9;
    assert!((score - expected).abs() < 1e-3);
}

#[test]
fn freshness_halves_every_week() {
    assert_eq!(freshness(None, now()), 0.0);
    assert_eq!(freshness(Some(now()), now()), 1.0);
    assert!((freshness(Some(now() - Duration::days(7)), now()) - 0.5).abs() < 1e-6);
    assert!((freshness(Some(now() - Duration::days(14)), now()) - 0.25).abs() < 1e-6);
}

#[test]
fn one_lucky_hit_is_not_certainty() {
    let one = stat("JFK", "ORD", "DEN", 1, 1, 1);
    let many = stat("JFK", "ORD", "SEA", 50, 45, 30);
    let (s1, p1) = score_candidate(Some(&one), 0.0, 0.5, now());
    let (s2, p2) = score_candidate(Some(&many), 0.0, 0.5, now());
    assert!(p1["connection"] < 0.8 && 0.8 < p2["connection"]);
    assert!(s2 > s1);
}

#[test]
fn dead_candidate_is_dropped_after_repeated_misses() {
    let bos = stat("JFK", "ORD", "BOS", DEAD_AFTER_CHECKS, 0, 0);
    assert!(is_dead(Some(&bos)));
    assert!(!is_dead(Some(&stat("JFK", "ORD", "MIA", 2, 0, 0))));
    let stats = HashMap::from([("BOS".into(), bos)]);
    let pool = HashMap::from([("BOS".into(), "hub".into()), ("DEN".into(), "hub".into())]);
    let hub_prob = HashMap::from([("BOS".into(), 1.0), ("DEN".into(), 1.0)]);
    let ranked = rank_candidates(
        "JFK",
        &hs(&["ORD"]),
        &stats,
        &pool,
        &hub_prob,
        &HashMap::new(),
        Some(now()),
    );
    assert_eq!(
        ranked.iter().map(|c| c.code.as_str()).collect::<Vec<_>>(),
        ["DEN"]
    );
}

#[test]
fn ranking_prefers_learned_history_over_hub_list() {
    let mut den = stat("JFK", "ORD", "DEN", 20, 18, 12);
    den.median_saving = 71.0;
    den.average_saving_percent = 28.0;
    den.last_cheaper_at = Some(now());
    let mut sea = stat("JFK", "ORD", "SEA", 20, 14, 7);
    sea.median_saving = 112.0;
    sea.average_saving_percent = 35.0;
    sea.last_cheaper_at = Some(now() - Duration::days(3));
    let mia = stat("JFK", "ORD", "MIA", 20, 1, 0);
    let stats = HashMap::from([
        ("DEN".into(), den),
        ("SEA".into(), sea),
        ("MIA".into(), mia),
    ]);
    let pool = HashMap::from([
        ("PHX".into(), "hub".into()),
        ("BOS".into(), "openflights".into()),
        ("DEN".into(), "hub".into()),
    ]);
    let route_map = HashMap::from([("ORD".into(), hs(&["BOS"]))]);
    let ranked = rank_candidates(
        "JFK",
        &hs(&["ORD"]),
        &stats,
        &pool,
        &hub_probabilities(&hs(&["ORD"]), &route_map),
        &HashMap::from([("DEN".into(), 0.9)]),
        Some(now()),
    );
    let codes: Vec<_> = ranked.iter().map(|c| c.code.as_str()).collect();
    assert_eq!(&codes[..2], ["DEN", "SEA"]);
    let bos = ranked.iter().find(|c| c.code == "BOS").unwrap();
    let phx = ranked.iter().find(|c| c.code == "PHX").unwrap();
    assert!(bos.score > phx.score);
    assert!(ranked.iter().all(|c| c.code != "JFK" && c.code != "ORD"));
}

#[test]
fn selection_threshold_and_cold_start() {
    let mut ranked = vec![
        Candidate {
            observations: 20,
            ..Candidate::new("DEN", 0.81, "stats")
        },
        Candidate {
            observations: 20,
            ..Candidate::new("SEA", 0.52, "stats")
        },
        Candidate::new("PHX", 0.125, "hub"),
        Candidate::new("LAS", 0.125, "hub"),
        Candidate::new("SFO", 0.125, "hub"),
    ];
    let chosen = select_candidates(&mut ranked, 8, 0.35, COLD_START_PROBES, &HashSet::new());
    assert_eq!(
        chosen.iter().map(|c| c.code.as_str()).collect::<Vec<_>>(),
        ["DEN", "SEA", "PHX"]
    );
    assert!(ranked[0].selected && ranked[1].selected && ranked[2].selected && !ranked[3].selected);

    let mut cold: Vec<_> = ["DEN", "SEA", "SFO", "LAX"]
        .iter()
        .map(|c| Candidate::new(c, 0.125, "hub"))
        .collect();
    assert_eq!(
        select_candidates(&mut cold, 8, 0.35, COLD_START_PROBES, &HashSet::new())
            .iter()
            .map(|c| c.code.as_str())
            .collect::<Vec<_>>(),
        ["DEN", "SEA", "SFO"]
    );
    assert_eq!(
        select_candidates(
            &mut cold,
            8,
            0.35,
            COLD_START_PROBES,
            &hs(&["DEN", "SEA", "SFO"])
        )
        .iter()
        .map(|c| c.code.as_str())
        .collect::<Vec<_>>(),
        ["LAX"]
    );
    assert!(select_candidates(&mut cold, 0, 0.35, COLD_START_PROBES, &HashSet::new()).is_empty());
}

#[test]
fn expected_value_per_paid_request() {
    assert_eq!(expected_value(0.65, 0.025, 12.0), 0.195);
    assert_eq!(expected_value(0.01, 0.01, 10.0), 0.001);
    assert!(expected_value(0.65, 0.025, 12.0) > 0.005);
    assert!(0.005 > expected_value(0.01, 0.01, 10.0));
}

#[test]
fn learner_records_every_fare_and_flight_edge() {
    let batch = jfk_ord_batch(&["DEN", "SEA", "LAX", "PHX"]);
    let uids: HashSet<_> = batch.fares.iter().map(|f| f.offer_uid.as_str()).collect();
    assert_eq!(uids, HashSet::from(["aa", "den", "sea", "lax"]));
    let den = batch.fares.iter().find(|f| f.offer_uid == "den").unwrap();
    assert_eq!(den.origin, "JFK");
    assert_eq!(den.ticketed, "DEN");
    assert_eq!(den.connections, ["ORD"]);
    assert_eq!(den.price, 170.0);
    assert_eq!(den.expires_at.as_deref(), Some("2026-09-08T12:30:00Z"));
    assert!(batch.edges.contains_key(&("JFK".into(), "ORD".into(), "UA".into(), "UA200".into())));
    assert!(batch.edges.contains_key(&("ORD".into(), "DEN".into(), "UA".into(), "UA300".into())));
    let edge = &batch.edges[&("JFK".into(), "ORD".into(), "UA".into(), "UA200".into())];
    assert!(edge.travel_dates.contains("2026-10-10"));
}

#[test]
fn learner_scores_probes_success_and_savings() {
    let batch = jfk_ord_batch(&["DEN", "SEA", "LAX", "PHX"]);
    let den = &batch.stats[&("JFK".into(), "ORD".into(), "DEN".into())];
    assert_eq!(den.observations, 1);
    assert_eq!(den.successful_connections, 1);
    assert_eq!(den.cheaper_than_direct_count, 1);
    assert_eq!(den.savings, [70.0]);
    assert_eq!(den.saving_pcts, [((100.0 * 70.0 / 240.0) * 100.0_f64).round() / 100.0]);
    assert_eq!(den.best_through_price, Some(170.0));
    let sea = &batch.stats[&("JFK".into(), "ORD".into(), "SEA".into())];
    assert_eq!(sea.observations, 1);
    assert_eq!(sea.successful_connections, 1);
    assert_eq!(sea.cheaper_than_direct_count, 0);
    let lax = &batch.stats[&("JFK".into(), "ORD".into(), "LAX".into())];
    assert_eq!(lax.observations, 1);
    assert_eq!(lax.successful_connections, 0);
    assert_eq!(
        batch.stats[&("JFK".into(), "DFW".into(), "LAX".into())].successful_connections,
        1
    );
    let phx = &batch.stats[&("JFK".into(), "ORD".into(), "PHX".into())];
    assert_eq!(phx.observations, 1);
    assert_eq!(phx.successful_connections, 0);
    assert!(batch.stats.keys().all(|k| k.2 != "ORD"));
}

#[test]
fn learner_counts_index_hits_without_double_counting_probes() {
    let batch = jfk_ord_batch(&[]);
    let den = &batch.stats[&("JFK".into(), "ORD".into(), "DEN".into())];
    assert_eq!(den.observations, 1);
    assert_eq!(den.successful_connections, 1);
}

#[test]
fn learner_converts_gbp_and_ignores_offers_from_other_origins() {
    let honest = priced("aa", 240.0, vec![seg("JFK", "ORD", "AA100", "AA")], "USD", "duffel");
    let gbp = priced(
        "gbp",
        80.0,
        vec![seg("JFK", "ORD", "BA1", "BA"), seg("ORD", "DEN", "BA2", "BA")],
        "GBP",
        "duffel",
    );
    let other = priced(
        "ewr",
        90.0,
        vec![seg("EWR", "ORD", "UA9", "UA"), seg("ORD", "DEN", "UA10", "UA")],
        "USD",
        "duffel",
    );
    let batch = build_batch(
        &[honest, gbp, other],
        &hs(&["JFK"]),
        &hs(&["ORD"]),
        Some(240.0),
        Some("USD"),
        &["DEN".into()],
        "2026-10-10",
        1,
        "ECONOMY",
        None,
        None,
        Some(now()),
    );
    let den = &batch.stats[&("JFK".into(), "ORD".into(), "DEN".into())];
    assert_eq!(den.successful_connections, 1);
    assert_eq!(den.cheaper_than_direct_count, 1);
    assert!(batch.stats.contains_key(&("JFK".into(), "ORD".into(), "DEN".into())));
    assert!(!batch.stats.contains_key(&("EWR".into(), "ORD".into(), "DEN".into())));
    let uids: HashSet<_> = batch.fares.iter().map(|f| f.offer_uid.as_str()).collect();
    assert_eq!(uids, HashSet::from(["aa", "gbp"]));
    assert!(batch.fares.iter().all(|f| f.currency == "USD"));
}

#[test]
fn savings_summary_keeps_history_bounded() {
    let kept = merge_savings(&(0..300).map(|i| i as f64).collect::<Vec<_>>(), &[1000.0]);
    assert_eq!(kept.len(), 200);
    assert_eq!(*kept.last().unwrap(), 1000.0);
    assert_eq!(summarize_savings(&[70.0, 90.0, 110.0]), (90.0, 90.0, 110.0));
    assert_eq!(summarize_savings(&[]), (0.0, 0.0, 0.0));
}

#[test]
fn learner_round_trip_ticketed_is_outbound_dest() {
    let rt = jfk_ord_roundtrip("2026-10-10", "2026-10-17");
    let batch = build_batch(
        &[rt],
        &hs(&["JFK"]),
        &hs(&["ORD"]),
        Some(430.0),
        Some("USD"),
        &[],
        "2026-10-10",
        1,
        "ECONOMY",
        None,
        None,
        Some(now()),
    );
    let fare = &batch.fares[0];
    assert_eq!(fare.ticketed, "ORD");
    assert!(fare.connections.is_empty());
    assert!(!batch.edges.contains_key(&("ORD".into(), "JFK".into(), "AA".into(), "AA101".into())));
    assert!(batch.stats.is_empty());
}

#[tokio::test]
async fn ledger_records_paid_calls_and_free_mock() {
    let ledger = start_ledger(0.005, 12);
    let sample = priced("x", 1.0, vec![seg("JFK", "ORD", "AA1", "AA")], "USD", "duffel");
    LEDGER
        .scope(ledger.clone(), async {
            let got = timed_shop(
                "duffel",
                "JFK",
                "DEN",
                "2026-10-10",
                "expand",
                async { Ok::<_, String>(vec![sample.clone()]) },
            )
            .await;
            assert_eq!(got.len(), 1);
            let boom = timed_shop(
                "duffel",
                "JFK",
                "SEA",
                "2026-10-10",
                "expand",
                async { Err::<Vec<Offer>, String>("supplier down".into()) },
            )
            .await;
            assert!(boom.is_empty());
            let mock = timed_shop(
                "mock",
                "JFK",
                "ORD",
                "2026-10-10",
                "direct",
                async { Ok::<_, String>(vec![sample]) },
            )
            .await;
            assert!(!mock.is_empty());
        })
        .await;
    assert_eq!(ledger.calls().len(), 3);
    assert_eq!(ledger.paid().len(), 2);
    assert_eq!(ledger.total_cost(), 0.01);
    let failed = ledger.calls().into_iter().find(|c| c.dest == "SEA").unwrap();
    assert!(!failed.ok && failed.offers == 0);
}

#[tokio::test]
async fn ledger_caps_paid_calls() {
    let ledger = start_ledger(0.005, 1);
    let sample = priced("x", 1.0, vec![seg("JFK", "ORD", "AA1", "AA")], "USD", "duffel");
    LEDGER
        .scope(ledger.clone(), async {
            let first = timed_shop(
                "duffel",
                "JFK",
                "DEN",
                "2026-10-10",
                "expand",
                async { Ok::<_, String>(vec![sample.clone()]) },
            )
            .await;
            let second = timed_shop(
                "duffel",
                "JFK",
                "SEA",
                "2026-10-10",
                "expand",
                async { Ok::<_, String>(vec![sample]) },
            )
            .await;
            assert_eq!(first.len(), 1);
            assert!(second.is_empty());
        })
        .await;
    assert_eq!(ledger.paid().len(), 1);
}

#[tokio::test]
async fn ledger_caps_concurrent_paid_calls() {
    let ledger = start_ledger(0.005, 2);
    let sample = priced("x", 1.0, vec![seg("JFK", "ORD", "AA1", "AA")], "USD", "duffel");
    LEDGER
        .scope(ledger.clone(), async {
            let ok = || {
                let sample = sample.clone();
                async move {
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                    Ok::<_, String>(vec![sample])
                }
            };
            let parts = tokio::join!(
                timed_shop("duffel", "JFK", "AAA", "2026-10-10", "direct", ok()),
                timed_shop("duffel", "JFK", "BBB", "2026-10-10", "direct", ok()),
                timed_shop("duffel", "JFK", "CCC", "2026-10-10", "direct", ok()),
            );
            let n = [parts.0, parts.1, parts.2].iter().filter(|p| !p.is_empty()).count();
            assert_eq!(n, 2);
        })
        .await;
    assert_eq!(ledger.paid().len(), 2);
}

#[test]
fn provider_score_rewards_hits_and_penalises_latency() {
    assert_eq!(provider_score(0, 0, 0.0), 0.5);
    assert!(provider_score(9, 10, 800.0) > provider_score(5, 10, 800.0));
    assert!(provider_score(9, 10, 800.0) > provider_score(9, 10, 2500.0));
    assert_eq!(Ledger::default().total_cost(), 0.0);
}
