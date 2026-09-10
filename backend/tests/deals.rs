mod common;

use skiplagging::db::repo;
use skiplagging::fx::to_usd;
use skiplagging::models::{HiddenCityMatch, Offer, RiskAssessment, Segment};
use skiplagging::providers::sandbox::{is_live_fare, is_sandbox_offer};

use common::{offer_full, pool, seg};

fn s(origin: &str, dest: &str, flight: &str) -> Segment {
    let mut x = seg(origin, dest, "2026-10-28T08:00", "2026-10-28T11:00", flight);
    x.duration_min = 180;
    x
}

fn priced(
    oid: &str,
    price: f64,
    segments: Vec<Segment>,
    stops: i32,
    currency: &str,
    source: &str,
    live: Option<bool>,
) -> Offer {
    let mut o = offer_full(oid, Some(price), segments, stops, currency, source, live, None);
    if stops > 0 {
        o.kind = "hidden-city".into();
    }
    o
}

fn match_of(local: Offer, through: Offer, hidden: &str, saving: f64, pct: f64) -> HiddenCityMatch {
    HiddenCityMatch {
        id: "m1".into(),
        hidden_city: hidden.into(),
        hidden_city_name: "Boston (BOS)".into(),
        local_offer: local,
        through_offer: through,
        first_flight_match: true,
        gross_saving: saving,
        saving_pct: pct,
        currency: "USD".into(),
        risk: RiskAssessment {
            score: 40,
            headline: "test".into(),
            one_way_only: true,
            carry_on_only: true,
            items: vec![],
            net_saving_estimate: saving,
            expected_disruption_cost: 0.0,
            expected_enforcement_cost: 0.0,
        },
        bookers: vec![],
        ticketed_destination: String::new(),
        intended_destination: String::new(),
        exit_segment_index: 0,
        warnings: vec![],
        result_type: "hidden_city".into(),
    }
}

async fn wipe(pool: &sqlx::PgPool, origin: &str, dest: &str, hidden: &str, date: &str, flight: &str) {
    let _ = sqlx::query(
        "DELETE FROM hidden_deals WHERE origin=$1 AND destination=$2 AND hidden_city=$3 AND date=$4 AND first_flight=$5",
    )
    .bind(origin)
    .bind(dest)
    .bind(hidden)
    .bind(date)
    .bind(flight)
    .execute(pool)
    .await;
}

#[tokio::test]
async fn hidden_deals_persist_and_list() {
    let pool = pool().await;
    let local = priced("n1", 400.0, vec![s("ORD", "DCA", "BA1")], 0, "USD", "duffel", Some(true));
    let through = priced(
        "h1",
        220.0,
        vec![s("ORD", "DCA", "BA1"), s("DCA", "BOS", "BA2")],
        1,
        "USD",
        "duffel",
        Some(true),
    );
    wipe(&pool, "ORD", "DCA", "BOS", "2026-10-28", "BA1").await;
    let n = repo::persist_hidden_deals(
        &pool,
        &[match_of(local, through, "BOS", 180.0, 45.0)],
        "ORD",
        "Chicago",
        "DCA",
        "Washington",
        "2026-10-28",
    )
    .await
    .unwrap();
    assert_eq!(n, 1);
    let deals = repo::list_hidden_deals(&pool, 48, "ORD", "").await.unwrap();
    let top = deals
        .iter()
        .find(|d| d.origin == "ORD" && d.hidden_city == "BOS" && d.first_flight == "BA1")
        .unwrap();
    assert_eq!(top.through_price, 220.0);
    assert_eq!(top.honest_price, 400.0);
    assert_eq!(top.saving, 180.0);
    assert_eq!(top.currency, "USD");
    assert!(top.risk.as_ref().unwrap().items.len() > 0);
    wipe(&pool, "ORD", "DCA", "BOS", "2026-10-28", "BA1").await;
}

#[tokio::test]
async fn expired_through_is_not_persisted_or_listed() {
    let pool = pool().await;
    let local = priced("n-exp", 400.0, vec![s("MIA", "ATL", "DL9")], 0, "USD", "duffel", Some(true));
    let mut through = priced(
        "h-exp",
        200.0,
        vec![s("MIA", "ATL", "DL9"), s("ATL", "BOS", "DL8")],
        1,
        "USD",
        "duffel",
        Some(true),
    );
    through.expires_at = Some("2020-01-01T00:00:00Z".into());
    wipe(&pool, "MIA", "ATL", "BOS", "2026-10-28", "DL9").await;
    let n = repo::persist_hidden_deals(
        &pool,
        &[match_of(local.clone(), through.clone(), "BOS", 200.0, 50.0)],
        "MIA",
        "Miami",
        "ATL",
        "Atlanta",
        "2026-10-28",
    )
    .await
    .unwrap();
    assert_eq!(n, 0);
    sqlx::query(
        "INSERT INTO hidden_deals (origin, destination, hidden_city, origin_city, dest_city, hidden_city_name, date, honest_price, through_price, currency, saving, saving_pct, first_flight, source, local_payload, through_payload) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)",
    )
    .bind("MIA")
    .bind("ATL")
    .bind("BOS")
    .bind("Miami")
    .bind("Atlanta")
    .bind("Boston (BOS)")
    .bind("2026-10-28")
    .bind(400.0)
    .bind(200.0)
    .bind("USD")
    .bind(200.0)
    .bind(50.0)
    .bind("DL9")
    .bind("duffel")
    .bind(serde_json::to_value(&local).unwrap())
    .bind(serde_json::to_value(&through).unwrap())
    .execute(&pool)
    .await
    .unwrap();
    let deals = repo::list_hidden_deals(&pool, 48, "MIA", "ATL").await.unwrap();
    assert!(!deals.iter().any(|d| d.first_flight == "DL9"));
    wipe(&pool, "MIA", "ATL", "BOS", "2026-10-28", "DL9").await;
}

#[tokio::test]
async fn persist_converts_to_usd_and_skips_sandbox() {
    let pool = pool().await;
    let gbp_local = priced("n-gbp", 400.0, vec![s("LHR", "DUB", "BA9")], 0, "GBP", "duffel", Some(true));
    let gbp_through = priced(
        "h-gbp",
        220.0,
        vec![s("LHR", "DUB", "BA9"), s("DUB", "BOS", "BA8")],
        1,
        "GBP",
        "duffel",
        Some(true),
    );
    let fake_local = priced("n-fake", 400.0, vec![s("JFK", "ORD", "BA3")], 0, "USD", "duffel", Some(false));
    let fake_through = priced(
        "h-fake",
        100.0,
        vec![s("JFK", "ORD", "BA3"), s("ORD", "DEN", "BA4")],
        1,
        "USD",
        "duffel",
        Some(false),
    );
    let mock_local = priced("n-mock", 400.0, vec![s("JFK", "BOS", "AA1")], 0, "USD", "mock", None);
    let mock_through = priced(
        "h-mock",
        200.0,
        vec![s("JFK", "BOS", "AA1"), s("BOS", "MIA", "AA2")],
        1,
        "USD",
        "mock",
        Some(true),
    );
    wipe(&pool, "LHR", "DUB", "BOS", "2026-11-02", "BA9").await;
    wipe(&pool, "JFK", "ORD", "DEN", "2026-11-02", "BA3").await;
    wipe(&pool, "JFK", "BOS", "MIA", "2026-11-02", "AA1").await;
    let n_live = repo::persist_hidden_deals(
        &pool,
        &[match_of(gbp_local, gbp_through, "BOS", 180.0, 45.0)],
        "LHR",
        "London",
        "DUB",
        "Dublin",
        "2026-11-02",
    )
    .await
    .unwrap();
    let n_fake = repo::persist_hidden_deals(
        &pool,
        &[match_of(fake_local, fake_through, "DEN", 300.0, 75.0)],
        "JFK",
        "New York",
        "ORD",
        "Chicago",
        "2026-11-02",
    )
    .await
    .unwrap();
    let n_mock = repo::persist_hidden_deals(
        &pool,
        &[match_of(mock_local, mock_through, "MIA", 200.0, 50.0)],
        "JFK",
        "New York",
        "BOS",
        "Boston",
        "2026-11-02",
    )
    .await
    .unwrap();
    assert_eq!(n_live, 1);
    assert_eq!(n_fake, 0);
    assert_eq!(n_mock, 0);
    let deals = repo::list_hidden_deals(&pool, 48, "LHR", "DUB").await.unwrap();
    assert_eq!(deals.len(), 1);
    assert_eq!(deals[0].currency, "USD");
    assert_eq!(deals[0].honest_price, to_usd(Some(400.0), Some("GBP")).unwrap());
    assert_eq!(deals[0].through_price, to_usd(Some(220.0), Some("GBP")).unwrap());
    assert_eq!(
        deals[0].saving,
        ((deals[0].honest_price - deals[0].through_price) * 100.0).round() / 100.0
    );
    wipe(&pool, "LHR", "DUB", "BOS", "2026-11-02", "BA9").await;
}

#[tokio::test]
async fn list_ranks_usd_and_hides_sandbox_rows() {
    let pool = pool().await;
    let krw_local = priced("n-krw", 80000.0, vec![s("ICN", "NRT", "KE1")], 0, "KRW", "duffel", Some(true));
    let krw_through = priced(
        "h-krw",
        10000.0,
        vec![s("ICN", "NRT", "KE1"), s("NRT", "HNL", "KE2")],
        1,
        "KRW",
        "duffel",
        Some(true),
    );
    let usd_local = priced("n-usd", 400.0, vec![s("ICN", "NRT", "DL1")], 0, "USD", "duffel", Some(true));
    let usd_through = priced(
        "h-usd",
        150.0,
        vec![s("ICN", "NRT", "DL1"), s("NRT", "LAX", "DL2")],
        1,
        "USD",
        "duffel",
        Some(true),
    );
    let fake = priced(
        "h-sand",
        50.0,
        vec![s("ICN", "NRT", "ZZ1"), s("NRT", "SEA", "ZZ2")],
        1,
        "USD",
        "duffel",
        Some(false),
    );
    sqlx::query("DELETE FROM hidden_deals WHERE origin='ICN' AND destination='NRT' AND date='2026-11-03'")
        .execute(&pool)
        .await
        .unwrap();
    for (hidden, name, honest, through_p, ccy, saving, pct, flight, local, through) in [
        (
            "HNL",
            "Honolulu (HNL)",
            80000.0,
            10000.0,
            "KRW",
            70000.0,
            87.0,
            "KE1",
            serde_json::to_value(&krw_local).unwrap(),
            serde_json::to_value(&krw_through).unwrap(),
        ),
        (
            "LAX",
            "Los Angeles (LAX)",
            400.0,
            150.0,
            "USD",
            250.0,
            62.0,
            "DL1",
            serde_json::to_value(&usd_local).unwrap(),
            serde_json::to_value(&usd_through).unwrap(),
        ),
        (
            "SEA",
            "Seattle (SEA)",
            400.0,
            50.0,
            "USD",
            350.0,
            87.0,
            "ZZ1",
            serde_json::to_value(&usd_local).unwrap(),
            serde_json::to_value(&fake).unwrap(),
        ),
    ] {
        sqlx::query(
            "INSERT INTO hidden_deals (origin, destination, hidden_city, origin_city, dest_city, hidden_city_name, date, honest_price, through_price, currency, saving, saving_pct, first_flight, source, bookers, local_payload, through_payload) \
             VALUES ('ICN','NRT',$1,'Seoul','Tokyo',$2,'2026-11-03',$3,$4,$5,$6,$7,$8,'duffel','[]'::jsonb,$9,$10)",
        )
        .bind(hidden)
        .bind(name)
        .bind(honest)
        .bind(through_p)
        .bind(ccy)
        .bind(saving)
        .bind(pct)
        .bind(flight)
        .bind(local)
        .bind(through)
        .execute(&pool)
        .await
        .unwrap();
    }
    let deals = repo::list_hidden_deals(&pool, 48, "ICN", "NRT").await.unwrap();
    let cities: Vec<_> = deals.iter().map(|d| d.hidden_city.as_str()).collect();
    assert!(!cities.contains(&"SEA"));
    assert_eq!(cities[0], "LAX");
    assert_eq!(deals[0].saving, 250.0);
    assert_eq!(deals[0].currency, "USD");
    sqlx::query("DELETE FROM hidden_deals WHERE origin='ICN' AND destination='NRT' AND date='2026-11-03'")
        .execute(&pool)
        .await
        .unwrap();
}

#[test]
fn live_fare_rejects_test_inventory() {
    let live = priced("ok", 200.0, vec![s("JFK", "LHR", "BA1")], 0, "USD", "duffel", Some(true));
    let mut fake = live.clone();
    fake.live = Some(false);
    fake.note = Some("Duffel offer. live_mode=False.".into());
    let mut mock = live.clone();
    mock.source = "mock".into();
    mock.live = None;
    let mut zz = live.clone();
    zz.carrier = "ZZ".into();
    zz.segments[0].carrier = "ZZ".into();
    assert!(is_live_fare(&live));
    assert!(is_sandbox_offer(&fake));
    assert!(!is_live_fare(&fake));
    assert!(!is_live_fare(&mock));
    assert!(!is_live_fare(&zz));
}
