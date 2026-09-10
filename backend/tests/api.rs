mod common;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use skiplagging::db::repo;
use skiplagging::models::OfferRefreshResponse;
use skiplagging::providers::mock::jfk_ord_den;
use tower::ServiceExt;

use common::{app, pool};

async fn get(app: axum::Router, uri: &str) -> (StatusCode, Value) {
    let response = app
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body = if bytes.is_empty() {
        json!(null)
    } else {
        serde_json::from_slice(&bytes).unwrap_or(json!(null))
    };
    (status, body)
}

async fn post(app: axum::Router, uri: &str, body: Value) -> (StatusCode, Value) {
    post_with(app, uri, body, &[]).await
}

async fn post_with(
    app: axum::Router,
    uri: &str,
    body: Value,
    extra: &[(&str, &str)],
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json");
    for (k, v) in extra {
        builder = builder.header(*k, *v);
    }
    let response = app
        .oneshot(
            builder
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let parsed = serde_json::from_slice(&bytes).unwrap_or(json!(null));
    (status, parsed)
}

#[tokio::test]
async fn reference_tables_are_populated() {
    let pool = pool().await;
    let counts = repo::counts(&pool).await.unwrap();
    assert!(counts["countries"] >= 200);
    assert!(counts["regions"] >= 1000);
    assert!(counts["airports"] >= 3000);
    assert!(counts["runways"] >= 1000);
    assert!(counts["continents"] >= 5);
    assert!(counts["routes"] >= 10000);
}

#[tokio::test]
async fn continents_and_countries_come_from_tables() {
    let pool = pool().await;
    let names = repo::continent_names(&pool).await.unwrap();
    let us = repo::get_country(&pool, "US").await.unwrap().unwrap();
    let fr = repo::get_country(&pool, "FR").await.unwrap().unwrap();
    let regions = repo::regions_for(&pool, "US").await.unwrap();
    assert!(!names.is_empty());
    assert!(
        !us.name.is_empty()
            && us.iso3.as_deref() == Some("USA")
            && !us.capital.is_empty()
            && !us.currency_code.is_empty()
    );
    assert_eq!(
        us.continent_name,
        names.get(&us.continent).cloned().unwrap_or_default()
    );
    assert!(!fr.capital.is_empty());
    assert!(regions.len() >= 50);
}

#[tokio::test]
async fn airport_search_ranks_cities_not_substrings() {
    let pool = pool().await;
    let paris = repo::search_airports(&pool, "paris", 12).await.unwrap();
    let nyc = repo::search_airports(&pool, "nyc", 12).await.unwrap();
    let tokyo = repo::search_airports(&pool, "tokyo", 12).await.unwrap();
    let sao = repo::search_airports(&pool, "sao paulo", 12).await.unwrap();
    let rome = repo::search_airports(&pool, "rome", 12).await.unwrap();
    let nice = repo::search_airports(&pool, "nice", 12).await.unwrap();
    let london = repo::search_airports(&pool, "london", 12).await.unwrap();
    let ny = repo::search_airports(&pool, "new york", 12).await.unwrap();
    let stub = repo::search_airports(&pool, "par", 12).await.unwrap();
    assert_eq!(paris[0].type_, "city");
    assert_eq!(paris[0].iata, "PAR");
    assert!(["CDG", "ORY"]
        .iter()
        .all(|c| paris[1..4].iter().any(|a| a.iata == *c)));
    assert!(paris.iter().all(|a| {
        !a.region_name.to_lowercase().contains("parish")
            || a.city.to_lowercase().starts_with("paris")
    }));
    assert_eq!(stub[0].type_, "city");
    assert_eq!(stub[0].iata, "PAR");
    assert_eq!(nyc[0].type_, "city");
    assert_eq!(nyc[0].iata, "NYC");
    assert!(["JFK", "LGA", "EWR"]
        .iter()
        .all(|c| nyc[1..4].iter().any(|a| a.iata == *c)));
    assert_eq!(tokyo[0].type_, "city");
    assert_eq!(tokyo[0].iata, "TYO");
    assert!(["HND", "NRT"]
        .iter()
        .all(|c| tokyo[1..3].iter().any(|a| a.iata == *c)));
    assert_eq!(sao[0].type_, "city");
    assert_eq!(sao[0].iata, "SAO");
    assert!(["GRU", "CGH"]
        .iter()
        .all(|c| sao[1..3].iter().any(|a| a.iata == *c)));
    assert_eq!(rome[0].type_, "city");
    assert_eq!(rome[0].iata, "ROM");
    assert_eq!(nice[0].iata, "NCE");
    assert_ne!(nice[0].type_, "city");
    assert_eq!(london[0].type_, "city");
    assert_eq!(london[0].iata, "LON");
    assert!(london[0].members.iter().any(|m| m == "LHR"));
    assert_eq!(ny[0].type_, "city");
    assert_eq!(ny[0].iata, "NYC");
    let pa = repo::search_airports(&pool, "pa", 12).await.unwrap();
    let pa_codes: Vec<_> = pa
        .iter()
        .map(|a| format!("{}:{}:{}", a.iata, a.city, a.country))
        .collect();
    assert!(
        pa.iter()
            .any(|a| a.iata == "PAR" || a.city.to_lowercase().starts_with("paris")),
        "two-letter ISO PA must not hide Paris behind Panama, got {pa_codes:?}"
    );
}

#[tokio::test]
async fn country_lookup_is_exact_for_codes() {
    let pool = pool().await;
    let us = repo::search_countries(&pool, "US", 300).await.unwrap();
    let usa = repo::search_countries(&pool, "USA", 300).await.unwrap();
    let fr = repo::search_countries(&pool, "FR", 300).await.unwrap();
    let france = repo::search_countries(&pool, "france", 300).await.unwrap();
    let cote = repo::search_countries(&pool, "cote", 300).await.unwrap();
    let tokyo = repo::search_countries(&pool, "tokyo", 300).await.unwrap();
    let united = repo::search_countries(&pool, "united", 300).await.unwrap();
    let de = repo::search_countries(&pool, "deutschland", 300)
        .await
        .unwrap();
    let ivory = repo::search_countries(&pool, "ivory coast", 300)
        .await
        .unwrap();
    let uk = repo::search_countries(&pool, "UK", 300).await.unwrap();
    assert_eq!(us.len(), 1);
    assert_eq!(us[0].iso2, "US");
    assert_eq!(usa.len(), 1);
    assert_eq!(usa[0].iso3.as_deref(), Some("USA"));
    assert_eq!(fr.len(), 1);
    assert_eq!(fr[0].iso2, "FR");
    assert_eq!(
        france.iter().map(|c| c.iso2.as_str()).collect::<Vec<_>>(),
        ["FR"]
    );
    assert_eq!(cote[0].iso2, "CI");
    assert_eq!(tokyo[0].iso2, "JP");
    let united_codes: std::collections::HashSet<_> =
        united.iter().map(|c| c.iso2.as_str()).collect();
    assert!(["US", "GB", "AE"].iter().all(|c| united_codes.contains(c)));
    assert_eq!(de.len(), 1);
    assert_eq!(de[0].iso2, "DE");
    assert_eq!(ivory.len(), 1);
    assert_eq!(ivory[0].iso2, "CI");
    assert_eq!(uk.len(), 1);
    assert_eq!(uk[0].iso2, "GB");
}

#[tokio::test]
async fn airport_search_by_country_and_region_names() {
    let pool = pool().await;
    let by_name = repo::search_airports(&pool, "united states", 12)
        .await
        .unwrap();
    let by_region = repo::search_airports(&pool, "illinois", 12).await.unwrap();
    let france = repo::search_airports(&pool, "france", 12).await.unwrap();
    let (origin, dest) = repo::default_pair(&pool).await.unwrap();
    assert!(!by_name.is_empty());
    assert!(by_name.iter().all(|a| !a.country_name.is_empty()));
    assert!(by_name.iter().all(|a| a.country == "US"));
    assert!(!france.is_empty());
    assert!(france.iter().all(|a| a.country == "FR"));
    assert!(france.iter().any(|a| a.iata == "CDG" || a.iata == "ORY"));
    assert!(!by_region.is_empty());
    assert!(by_region
        .iter()
        .any(|a| a.region_name.to_lowercase() == "illinois"));
    let origin = origin.unwrap();
    let dest = dest.unwrap();
    assert_ne!(origin.iata, dest.iata);
    assert!(!origin.country_name.is_empty() && !dest.city.is_empty());
}

#[tokio::test]
async fn airport_detail_has_real_runways() {
    let pool = pool().await;
    let (origin, _) = repo::default_pair(&pool).await.unwrap();
    let origin = origin.unwrap();
    let detail = repo::get_airport(&pool, &origin.iata, true)
        .await
        .unwrap()
        .unwrap();
    assert!(!detail.runways.is_empty());
    let longest = detail
        .runways
        .iter()
        .filter_map(|r| r.length_ft)
        .max()
        .unwrap_or(0);
    assert!(longest > 3000);
}

#[tokio::test]
async fn hidden_city_candidates_are_route_rows() {
    let pool = pool().await;
    let (_, dest) = repo::default_pair(&pool).await.unwrap();
    let dest = dest.unwrap();
    let spokes = repo::destinations_from(&pool, &dest.iata, 12)
        .await
        .unwrap();
    assert!(!spokes.is_empty());
    assert!(spokes
        .iter()
        .all(|(code, airline, _)| code.len() == 3 && !airline.is_empty()));
}

#[tokio::test]
async fn health_and_defaults_and_search() {
    let app = app().await;
    let (st, health) = get(app.clone(), "/health").await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(health["ok"], true);
    assert!(health["tables"]["countries"].as_i64().unwrap() >= 200);
    assert_eq!(health["database"], "postgresql");
    let duffel_mode = health["shop"]["duffel"].as_str().unwrap();
    assert!(matches!(duffel_mode, "live" | "sandbox" | "off"));
    let opensky_mode = health["track"]["opensky"].as_str().unwrap();
    assert!(matches!(opensky_mode, "oauth" | "anonymous"));

    let (st, defaults) = get(app.clone(), "/defaults").await;
    assert_eq!(st, StatusCode::OK);
    let o = defaults["origin"]["iata"].as_str().unwrap();
    let d = defaults["destination"]["iata"].as_str().unwrap();
    assert_ne!(o, d);
    assert!(!defaults["origin"]["country_name"]
        .as_str()
        .unwrap()
        .is_empty());
    let (st, lite) = get(app.clone(), &format!("/airports/{o}?lite=1")).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(lite["iata"], o);
    assert!(lite["runways"]
        .as_array()
        .map(|a| a.is_empty())
        .unwrap_or(true));
    let (st, full) = get(app.clone(), &format!("/airports/{o}")).await;
    assert_eq!(st, StatusCode::OK);
    assert!(!full["runways"].as_array().unwrap().is_empty());

    let q = defaults["origin"]["country_name"]
        .as_str()
        .unwrap()
        .chars()
        .take(6)
        .collect::<String>();
    let (st, countries) = get(app.clone(), &format!("/countries?q={q}")).await;
    assert_eq!(st, StatusCode::OK);
    assert!(countries.as_array().unwrap().len() > 0);
    let (st, us) = get(app.clone(), "/countries?q=US").await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(
        us.as_array()
            .unwrap()
            .iter()
            .map(|c| c["iso2"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["US"]
    );
    let (st, all_countries) = get(app.clone(), "/countries").await;
    assert_eq!(st, StatusCode::OK);
    assert!(all_countries.as_array().unwrap().len() >= 200);

    let (st, data) = post(
        app.clone(),
        "/search",
        json!({"origin": o, "destination": d, "date": "2026-11-02", "include_nearby": false}),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(data["origin"]["iata"], o);
    assert!(!data["destination"]["country_name"]
        .as_str()
        .unwrap()
        .is_empty());
    assert!(data["bookers"].as_array().unwrap().len() > 0);
    assert!(data["bookers"]
        .as_array()
        .unwrap()
        .iter()
        .all(|b| !b["url"].as_str().unwrap().contains("aa.com")));
    assert!(data["connection_hints"].as_array().unwrap().len() > 0);
    assert!(data["connection_hints"]
        .as_array()
        .unwrap()
        .iter()
        .all(|h| h["source"] == "openflights-routes"));
    assert!(data["search_id"].as_i64().is_some());

    let (st, deals) = get(app.clone(), "/deals").await;
    assert_eq!(st, StatusCode::OK);
    if !skiplagging::config::Settings::load().publish_deals() {
        assert_eq!(deals, json!([]));
    }

    let (st, _) = post(
        app.clone(),
        "/search",
        json!({"origin": "JFK", "destination": "ORD", "date": "2026-10-17", "return_date": "2026-10-10"}),
    )
    .await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY);

    let (st, cabin) = post(
        app.clone(),
        "/search",
        json!({"origin": "JFK", "destination": "ORD", "date": "2026-10-17", "cabin": "COUCH"}),
    )
    .await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(cabin["detail"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("cabin"));

    let (st, date) = post(
        app.clone(),
        "/search",
        json!({"origin": "JFK", "destination": "ORD", "date": "2026-13-01"}),
    )
    .await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(date["detail"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("date"));

    let (st, _) = post(
        app,
        "/search",
        json!({"origin": "ZZZ", "destination": "ORD", "date": "2026-10-17"}),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn mock_jfk_ord_search_shows_hidden_city() {
    let app = app().await;
    let (st, data) = post(
        app,
        "/search",
        json!({"origin": "JFK", "destination": "ORD", "date": "2026-10-10", "include_nearby": false}),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(data["search_debug"]["reused_honest"], false);
    assert_ne!(data["search_debug"]["cache"], "fresh");
    assert_eq!(data["honest_pick"]["offer"]["price"], 240.0);
    let hidden = data["hidden_if_cheaper"]["through_offer"]["id"]
        .as_str()
        .unwrap();
    assert!(hidden.contains("den") || hidden.contains("sea"));
    let ids: Vec<_> = data["hidden_city"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["through_offer"]["id"].as_str().unwrap())
        .collect();
    assert!(ids.iter().any(|id| id.contains("den")));
    let den = data["hidden_city"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["through_offer"]["id"].as_str().unwrap().contains("den"))
        .unwrap();
    assert!(den["hidden_city_name"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("denver"));
    assert!(!den["risk"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|i| i["id"] == "documents"));
    let gaps: Vec<_> = data["data_gaps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g.as_str().unwrap().to_lowercase())
        .collect();
    assert!(!gaps.iter().any(|g| g.contains("no priced inversion")));
}

#[tokio::test]
async fn sources_mark_duffel_live_sandbox_or_off() {
    let app = app().await;
    let (st, body) = get(app, "/sources").await;
    assert_eq!(st, StatusCode::OK);
    let duffel = body["sources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == "duffel")
        .unwrap();
    let status = duffel["status"].as_str().unwrap();
    let label = duffel["status_label"].as_str().unwrap();
    assert_ne!(status, "on");
    assert_ne!(label, "On");
    let settings = skiplagging::config::Settings::load();
    if settings.duffel_live() {
        assert_eq!(status, "live");
        assert_eq!(label, "Live");
        assert_eq!(duffel["configured"], true);
    } else if settings.duffel_sandbox() {
        assert_eq!(status, "sandbox");
        assert_eq!(label, "Sandbox — offers dropped");
        assert_eq!(duffel["configured"], false);
    } else {
        assert_eq!(status, "off");
        assert_eq!(label, "Needs a key");
        assert_eq!(duffel["configured"], false);
    }

    let opensky = body["sources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == "opensky")
        .unwrap();
    let os_status = opensky["status"].as_str().unwrap();
    assert_ne!(os_status, "on");
    if settings.opensky_oauth() {
        assert_eq!(os_status, "oauth");
        assert_eq!(opensky["status_label"], "OAuth");
    } else {
        assert_eq!(os_status, "anonymous");
        assert_eq!(opensky["status_label"], "Anonymous");
    }
    assert_eq!(opensky["configured"], true);
}

#[tokio::test]
async fn mock_round_trip_is_honest_only_http() {
    let app = app().await;
    let (st, data) = post(
        app,
        "/search",
        json!({
            "origin": "JFK",
            "destination": "ORD",
            "date": "2026-10-10",
            "return_date": "2026-10-17",
            "include_nearby": false
        }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(data["honest_pick"]["offer"]["price"], 430.0);
    assert!(data["hidden_if_cheaper"].is_null());
    let gaps: Vec<_> = data["data_gaps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g.as_str().unwrap().to_lowercase())
        .collect();
    assert!(gaps.iter().any(|g| g.contains("one-way")));
    assert!(!gaps.iter().any(|g| g.contains("no priced inversion")));
}

#[tokio::test]
async fn track_uses_opensky_or_explains_gap() {
    let app = app().await;
    let (_, defaults) = get(app.clone(), "/defaults").await;
    let iata = defaults["origin"]["iata"].as_str().unwrap();
    let (st, traffic) = get(app, &format!("/track/{iata}")).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(traffic["source"], "opensky");
    assert!(traffic["trackers"].as_array().unwrap().len() > 0);
    assert_eq!(traffic["layer"], "live-track");
    assert!(
        traffic["aircraft"]
            .as_array()
            .map(|a| !a.is_empty())
            .unwrap_or(false)
            || traffic["note"]
                .as_str()
                .map(|n| !n.is_empty())
                .unwrap_or(false)
    );
}

#[tokio::test]
async fn refresh_endpoint_matches_mock_contract() {
    let app = app().await;
    let den = jfk_ord_den("2026-10-10");
    let (st, ok) = post(
        app.clone(),
        "/offers/refresh",
        json!({"offer": den, "intended_destination": "ORD"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let ok: OfferRefreshResponse = serde_json::from_value(ok).unwrap();
    assert!(ok.valid);
    assert_eq!(ok.offer.unwrap().price, Some(175.0));

    let mut mutated = den.clone();
    mutated.id = "mock-jfk-ord-den-mutated".into();
    let (st, dead) = post(
        app.clone(),
        "/offers/refresh",
        json!({"offer": mutated, "intended_destination": "ORD"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let dead: OfferRefreshResponse = serde_json::from_value(dead).unwrap();
    assert!(!dead.valid);
    assert!(dead.reason.contains("intended city"));

    let mut other = den;
    other.id = "duffel-x".into();
    other.source = "duffel".into();
    let (st, other_body) = post(
        app.clone(),
        "/offers/refresh",
        json!({"offer": other, "intended_destination": "ORD"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let other: OfferRefreshResponse = serde_json::from_value(other_body).unwrap();
    assert!(!other.valid);
    assert!(other.offer.is_none());

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/offers/refresh")
                .header("content-type", "application/json")
                .body(Body::from("{"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let parsed: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(parsed["detail"].as_str().unwrap().len() > 0);
}

fn jfk_ord_body() -> Value {
    json!({"origin": "JFK", "destination": "ORD", "date": "2026-10-10", "include_nearby": false})
}

#[tokio::test]
async fn repeat_search_stamps_http_cache_not_fresh() {
    let app = app().await;
    let body = jfk_ord_body();
    let (st, first) = post(app.clone(), "/search", body.clone()).await;
    assert_eq!(st, StatusCode::OK);
    let c1 = first["search_debug"]["cache"].as_str().unwrap();
    assert_ne!(c1, "fresh");
    assert!(!c1.starts_with("http-cache"));
    assert!(c1 == "miss" || c1 == "index" || c1 == "index+live", "{c1}");

    let (st, second) = post(app.clone(), "/search", body).await;
    assert_eq!(st, StatusCode::OK);
    let c2 = second["search_debug"]["cache"].as_str().unwrap();
    assert!(c2.starts_with("http-cache("), "{c2}");
    assert!(!c2.contains("fresh"));
    assert_eq!(
        second["honest_pick"]["offer"]["price"],
        first["honest_pick"]["offer"]["price"]
    );
}

#[tokio::test]
async fn recheck_refresh_flag_bypasses_http_cache() {
    let app = app().await;
    let body = jfk_ord_body();
    let _ = post(app.clone(), "/search", body.clone()).await;
    let mut refresh = body;
    refresh["refresh"] = json!(true);
    let (st, data) = post(app, "/search", refresh).await;
    assert_eq!(st, StatusCode::OK);
    let cache = data["search_debug"]["cache"].as_str().unwrap();
    assert!(!cache.starts_with("http-cache"), "{cache}");
    assert_ne!(cache, "fresh");
}

#[tokio::test]
async fn recheck_cache_control_bypasses_http_cache() {
    let app = app().await;
    let body = jfk_ord_body();
    let _ = post(app.clone(), "/search", body.clone()).await;
    let (st, data) = post_with(app, "/search", body, &[("cache-control", "no-cache")]).await;
    assert_eq!(st, StatusCode::OK);
    let cache = data["search_debug"]["cache"].as_str().unwrap();
    assert!(!cache.starts_with("http-cache"), "{cache}");
    assert_ne!(cache, "fresh");
}
