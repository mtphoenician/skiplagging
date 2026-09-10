use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::cache::{build_cache, search_key, SearchCache};
use crate::catalog::sources_payload;
use crate::config::Settings;
use crate::db::repo;
use crate::engines::candidates::graph_snapshot;
use crate::engines::hidden::{detect_hidden_city, refresh_keeps_hidden_city};
use crate::engines::refresh::refresh_priced_offer;
use crate::engines::shop::search_all_ways;
use crate::models::{
    OfferRefreshRequest, OfferRefreshResponse, SearchExpandRequest, SearchQuery, SearchQueryError,
};
use crate::providers::aerodatabox::AeroDataBoxProvider;
use crate::providers::opensky::OpenSkyProvider;

#[derive(Clone)]
pub struct AppState {
    pub settings: Arc<Settings>,
    pub pool: PgPool,
    pub http: reqwest::Client,
    pub cache: SearchCache,
}

pub fn router(state: AppState) -> Router {
    let origins: Vec<HeaderValue> = state
        .settings
        .origin_list()
        .into_iter()
        .filter_map(|s| s.parse().ok())
        .collect();
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers(tower_http::cors::Any);
    Router::new()
        .route("/health", get(health))
        .route("/sources", get(sources))
        .route("/defaults", get(defaults))
        .route("/continents", get(continents))
        .route("/countries", get(countries))
        .route("/countries/{iso2}", get(country))
        .route("/countries/{iso2}/regions", get(country_regions))
        .route("/airports", get(airports))
        .route("/airports/{iata}", get(airport_detail))
        .route("/track/{iata}", get(track))
        .route("/board/{iata}", get(board))
        .route("/deals", get(deals))
        .route("/search", post(search))
        .route("/search/expand", post(search_expand))
        .route("/offers/refresh", post(refresh_offer))
        .route("/debug/route-graph", get(debug_route_graph))
        .route("/debug/budget", get(debug_budget))
        .route("/debug/price-history", get(debug_price_history))
        .layer(cors)
        .with_state(state)
}

fn err(status: StatusCode, detail: impl Into<String>) -> Response {
    (status, Json(json!({ "detail": detail.into() }))).into_response()
}

async fn health(State(st): State<AppState>) -> Json<Value> {
    let s = &st.settings;
    let db = repo::counts(&st.pool).await.unwrap_or_default();
    Json(json!({
        "ok": true,
        "database": "postgresql",
        "shop": {
            "duffel": s.duffel_enabled(),
            "duffel_live": s.duffel_live(),
            "publish_deals": s.publish_deals(),
        },
        "track": {"opensky": true, "aerodatabox": s.aerodatabox_enabled()},
        "tables": db,
    }))
}

async fn sources(State(st): State<AppState>) -> Json<Value> {
    let s = &st.settings;
    let enabled = std::collections::HashMap::from([
        ("duffel".into(), s.duffel_enabled()),
        ("aerodatabox".into(), s.aerodatabox_enabled()),
    ]);
    Json(json!({
        "layers": [
            "reference — OurAirports countries/regions/airports/runways/navaids + GeoNames countryInfo",
            "historical-route-map — who used to fly B→C (OpenFlights, stale)",
            "schedule-status — FIDS board (AeroDataBox if keyed)",
            "priced-offer — NDC shop (Duffel if keyed)",
            "order-pnr — not implemented; we never create a reservation",
            "live-track — ADS-B state vectors (OpenSky)",
            "meta-search / ota / airline-direct — booker deep-links only",
        ],
        "sources": sources_payload(&enabled),
    }))
}

async fn defaults(State(st): State<AppState>) -> Response {
    match repo::default_pair(&st.pool).await {
        Ok((Some(origin), Some(dest))) => Json(json!({"origin": origin, "destination": dest})).into_response(),
        _ => err(
            StatusCode::SERVICE_UNAVAILABLE,
            "Route table is empty. Run cargo run --bin ingest",
        ),
    }
}

async fn continents(State(st): State<AppState>) -> Json<Value> {
    Json(json!(repo::list_continents(&st.pool).await.unwrap_or_default()))
}

#[derive(Deserialize)]
struct Q {
    #[serde(default)]
    q: String,
}

async fn countries(State(st): State<AppState>, Query(q): Query<Q>) -> Json<Value> {
    Json(json!(
        repo::search_countries(&st.pool, &q.q, 300).await.unwrap_or_default()
    ))
}

async fn country(State(st): State<AppState>, Path(iso2): Path<String>) -> Response {
    match repo::get_country(&st.pool, &iso2).await {
        Ok(Some(row)) => Json(row).into_response(),
        _ => err(
            StatusCode::NOT_FOUND,
            "Unknown ISO 3166-1 alpha-2 in OurAirports/GeoNames tables",
        ),
    }
}

async fn country_regions(State(st): State<AppState>, Path(iso2): Path<String>) -> Json<Value> {
    Json(json!(
        repo::regions_for(&st.pool, &iso2).await.unwrap_or_default()
    ))
}

async fn airports(State(st): State<AppState>, Query(q): Query<Q>) -> Response {
    match repo::search_airports(&st.pool, &q.q, 12).await {
        Ok(hits) => {
            if hits.is_empty() && q.q.is_empty() {
                return err(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Airport table is empty. From backend/: cargo run --bin ingest",
                );
            }
            Json(hits).into_response()
        }
        Err(_) => err(StatusCode::INTERNAL_SERVER_ERROR, "airport search failed"),
    }
}

async fn airport_detail(State(st): State<AppState>, Path(iata): Path<String>) -> Response {
    match repo::get_place(&st.pool, &iata, true).await {
        Ok(Some(ap)) => Json(ap).into_response(),
        _ => err(StatusCode::NOT_FOUND, "Unknown IATA in OurAirports table"),
    }
}

async fn track(State(st): State<AppState>, Path(iata): Path<String>) -> Response {
    let Ok(Some(ap)) = repo::get_airport(&st.pool, &iata, false).await else {
        return err(StatusCode::NOT_FOUND, "Unknown IATA in OurAirports table");
    };
    let traffic = OpenSkyProvider::new(&st.settings, st.http.clone())
        .traffic_near(&ap)
        .await
        .unwrap_or_else(|_| LiveTrafficPlaceholder::from(&ap.iata));
    if !traffic.aircraft.is_empty() {
        let states: Vec<Value> = traffic
            .aircraft
            .iter()
            .filter_map(|a| serde_json::to_value(a).ok())
            .collect();
        let _ = repo::persist_tracks(
            &st.pool,
            &ap.iata,
            &states,
            traffic.api_time.map(|t| t as i32),
        )
        .await;
    }
    Json(traffic).into_response()
}

struct LiveTrafficPlaceholder;
impl LiveTrafficPlaceholder {
    fn from(iata: &str) -> crate::models::LiveTraffic {
        crate::models::LiveTraffic {
            airport: iata.to_string(),
            source: "opensky".into(),
            layer: "live-track".into(),
            api_time: None,
            note: "OpenSky unavailable.".into(),
            aircraft: vec![],
            trackers: crate::providers::opensky::tracker_links(iata, iata),
        }
    }
}

async fn board(State(st): State<AppState>, Path(iata): Path<String>) -> Json<Value> {
    let s = &st.settings;
    if !s.aerodatabox_enabled() {
        return Json(json!({
            "airport": iata.to_uppercase(),
            "source": Value::Null,
            "flights": [],
            "gap": "RAPIDAPI_KEY not set. AeroDataBox FIDS is a schedule/status board, not a fare shop.",
        }));
    }
    let flights = AeroDataBoxProvider::new(s, st.http.clone())
        .board(&iata.to_uppercase(), "Departure")
        .await
        .unwrap_or_default();
    Json(json!({
        "airport": iata.to_uppercase(),
        "source": "aerodatabox",
        "layer": "schedule-status",
        "flights": flights,
    }))
}

#[derive(Deserialize)]
struct DealsQ {
    #[serde(default = "default_limit")]
    limit: i64,
    #[serde(default)]
    origin: String,
    #[serde(default)]
    dest: String,
}
fn default_limit() -> i64 {
    48
}

async fn deals(State(st): State<AppState>, Query(q): Query<DealsQ>) -> Json<Value> {
    if !st.settings.publish_deals() {
        return Json(json!([]));
    }
    let limit = q.limit.clamp(1, 120);
    Json(json!(
        repo::list_hidden_deals(&st.pool, limit, &q.origin, &q.dest)
            .await
            .unwrap_or_default()
    ))
}

fn cache_key(query: &SearchQuery) -> String {
    search_key(
        &query.origin,
        &query.destination,
        &query.date,
        query.adults,
        &query.cabin,
        query.include_nearby,
        false,
        &query.currency,
        query.return_date.as_deref(),
    )
}

fn validate_query(mut q: SearchQuery) -> Result<SearchQuery, Response> {
    match q.validate() {
        Ok(()) => Ok(q),
        Err(SearchQueryError::ReturnBeforeOutbound) => Err(err(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Return date must be on or after the outbound date.",
        )),
        Err(SearchQueryError::BadField(f)) => {
            let msg = match f {
                "origin" => "Invalid origin IATA or city code.",
                "destination" => "Invalid destination IATA or city code.",
                "date" => "Invalid outbound date. Use YYYY-MM-DD.",
                "return_date" => "Invalid return date. Use YYYY-MM-DD.",
                "adults" => "Adults must be between 1 and 9.",
                "cabin" => "Cabin must be ECONOMY, PREMIUM_ECONOMY, BUSINESS, or FIRST.",
                other => other,
            };
            Err(err(StatusCode::UNPROCESSABLE_ENTITY, msg))
        }
    }
}

fn search_err(e: anyhow::Error) -> Response {
    let msg = e.to_string();
    if msg.contains("Unknown IATA") || msg.contains("must differ") {
        err(StatusCode::BAD_REQUEST, msg)
    } else {
        tracing::error!(error = %msg, "search failed");
        err(StatusCode::INTERNAL_SERVER_ERROR, "Search failed")
    }
}

fn json_err(e: axum::extract::rejection::JsonRejection) -> Response {
    err(StatusCode::UNPROCESSABLE_ENTITY, e.body_text())
}

async fn search(
    State(st): State<AppState>,
    body: Result<Json<SearchQuery>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(query) = match body {
        Ok(j) => j,
        Err(e) => return json_err(e),
    };
    let query = match validate_query(query) {
        Ok(q) => q,
        Err(r) => return r,
    };
    let key = cache_key(&query);
    if let Some(mut cached) = st.cache.get(&key) {
        if let Some(debug) = cached.search_debug.as_mut() {
            debug.cache = "fresh".into();
        }
        return Json(cached).into_response();
    }
    match search_all_ways(query, &st.settings, &st.http, &st.pool, "fast", None).await {
        Ok(result) => {
            st.cache.insert(key, result.clone());
            Json(result).into_response()
        }
        Err(e) => search_err(e),
    }
}

async fn search_expand(
    State(st): State<AppState>,
    body: Result<Json<SearchExpandRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(j) => j,
        Err(e) => return json_err(e),
    };
    let query = match validate_query(body.query) {
        Ok(q) => q,
        Err(r) => return r,
    };
    let key = cache_key(&query);
    match search_all_ways(
        query,
        &st.settings,
        &st.http,
        &st.pool,
        "deep",
        Some(&body.exclude),
    )
    .await
    {
        Ok(result) => {
            st.cache.insert(key, result.clone());
            Json(result).into_response()
        }
        Err(e) => search_err(e),
    }
}

async fn refresh_offer(
    State(st): State<AppState>,
    body: Result<Json<OfferRefreshRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(j) => j,
        Err(e) => return json_err(e),
    };
    let fresh = refresh_priced_offer(&body.offer, &st.settings, Some(&st.http)).await;
    let Some(mut fresh) = fresh else {
        let configured = body.offer.source != "duffel" || st.settings.duffel_enabled();
        let reason = if configured {
            "Could not re-fetch this fare from the supplier. Confirm it on a booker."
        } else {
            "Duffel is not configured. Confirm the fare on the booker link."
        };
        return Json(OfferRefreshResponse {
            offer: None,
            valid: false,
            reason: reason.into(),
        })
        .into_response();
    };
    let intended = body.intended_destination.clone();
    if detect_hidden_city(&body.offer, intended.as_str()).is_some() {
        if !refresh_keeps_hidden_city(&body.offer, &fresh, intended.as_str()) {
            return Json(OfferRefreshResponse {
                offer: Some(fresh),
                valid: false,
                reason: "Refreshed itinerary no longer passes through the intended city.".into(),
            })
            .into_response();
        }
        if body.offer.kind == "hidden-city" && fresh.kind != "hidden-city" {
            fresh.kind = "hidden-city".into();
        }
    }
    Json(OfferRefreshResponse {
        offer: Some(fresh),
        valid: true,
        reason: String::new(),
    })
    .into_response()
}

#[derive(Deserialize)]
struct GraphQ {
    #[serde(default)]
    origin: String,
    #[serde(default)]
    intended: String,
    #[serde(default = "default_graph_limit")]
    limit: i64,
}
fn default_graph_limit() -> i64 {
    100
}

async fn debug_route_graph(State(st): State<AppState>, Query(q): Query<GraphQ>) -> Json<Value> {
    let limit = q.limit.clamp(1, 500);
    let stats = repo::route_stats_snapshot(&st.pool, &q.origin, &q.intended, limit)
        .await
        .unwrap_or_default();
    let edges = repo::route_edges_snapshot(&st.pool, &q.origin, limit)
        .await
        .unwrap_or_default();
    Json(json!({
        "hidden_city_route_stats": stats,
        "route_edges": edges,
        "session_edges": graph_snapshot(),
        "note": "Learned from priced itineraries we actually saw. Not a licensed schedule, never a summed price.",
    }))
}

#[derive(Deserialize)]
struct BudgetQ {
    #[serde(default = "default_hours")]
    hours: i64,
    #[serde(default)]
    origin: String,
    #[serde(default)]
    dest: String,
}
fn default_hours() -> i64 {
    24
}

async fn debug_budget(State(st): State<AppState>, Query(q): Query<BudgetQ>) -> Json<Value> {
    let hours = q.hours.clamp(1, 24 * 30);
    let mut summary = repo::budget_summary(&st.pool, hours)
        .await
        .unwrap_or(json!({}));
    if !q.origin.is_empty() && !q.dest.is_empty() {
        if let Ok(scores) = repo::provider_route_scores(&st.pool, &q.origin, &q.dest).await {
            summary["provider_scores"] = json!(scores);
        }
    }
    summary["cost_per_call_usd"] = json!(st.settings.search_cost_usd);
    Json(summary)
}

#[derive(Deserialize)]
struct HistQ {
    fingerprint: String,
}

async fn debug_price_history(State(st): State<AppState>, Query(q): Query<HistQ>) -> Json<Value> {
    let rows = repo::price_history(&st.pool, &q.fingerprint, 200)
        .await
        .unwrap_or_default();
    Json(json!({"fingerprint": q.fingerprint, "observations": rows}))
}

pub async fn serve() -> anyhow::Result<()> {
    let settings = Arc::new(Settings::load());
    let pool = crate::db::pool::connect_with(&settings).await?;
    crate::db::schema::init_db(&pool).await?;
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(25))
        .connect_timeout(std::time::Duration::from_secs(6))
        .pool_max_idle_per_host(20)
        .build()?;
    let cache = build_cache(&settings);
    let app = router(AppState {
        settings,
        pool,
        http,
        cache,
    });
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8000").await?;
    tracing::info!("listening on http://0.0.0.0:8000");
    axum::serve(listener, app).await?;
    Ok(())
}
