use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, NaiveDateTime, Utc};
use futures::future::join_all;
use serde_json::Value;
use sqlx::PgPool;
use tokio::sync::Semaphore;

use crate::budget::{start_ledger, timed_shop, ProviderCall, LEDGER};
use crate::config::Settings;
use crate::db::repo;
use crate::engines::candidates::{
    assign_source, hub_probabilities, rank_candidates, record_offer, select_candidates, Candidate,
};
use crate::engines::expiry::{offer_unexpired, take_unexpired};
use crate::engines::hidden::{
    detect_hidden_city, hidden_city_savings, is_standard_to, itinerary_fingerprint,
    meaningful_saving, ticketed_destination,
};
use crate::engines::window::{attach_windows, sort_matches};
use crate::engines::index::{classify_for_search, ticketed_dests_through};
use crate::engines::learn::build_batch;
use crate::engines::risk::{assess, attach_risk};
use crate::engines::self_transfer::{
    bridge_pairs, combine_at_hubs, next_day, pick_transfer_hubs, MAX_LEG_FLIGHTS,
};
use crate::fx::{offer_in_usd, offers_in_usd, refresh_rates, take_offers_in_usd};
use crate::models::{
    Airport, BoardFlight, CandidateTrace, ChannelGroup, ConnectionHint, HiddenCityMatch,
    HonestPick, LiveTraffic, Offer, SearchDebug, SearchQuery, SearchResponse, ShopRequest,
    TrackedAircraft,
};
use crate::providers::aerodatabox::AeroDataBoxProvider;
use crate::providers::bookers::booker_links;
use crate::providers::duffel::DuffelProvider;
use crate::providers::mock::MockProvider;
use crate::providers::opensky::{tracker_links, OpenSkyProvider};
use crate::providers::sandbox::{is_live_fare, is_sandbox_offer, is_test_carrier};

pub const SIDE_TIMEOUT: Duration = Duration::from_millis(1200);
/// Reuse indexed honest A→B only when it is a live Duffel fare this fresh.
/// Offer TTL (900s) still applies to hidden-city through tickets. HTTP cache is
/// separately capped at 45s in live mode. Recheck / Cache-Control: no-cache skips it.
/// Freshness only — never skip honest A→B to free ledger slots for extra Cs.
pub const HONEST_LIVE_MAX_AGE_SECS: i64 = 60;

/// Ticketed-C shops this pass. Fast uses FAST_CANDIDATES. Deep shops the rest
/// up to MAX_HIDDEN_CANDIDATES (`already_probed` = fast `expanded_destinations`,
/// not covered through-dests we already have for free).
pub fn hidden_probe_budget(mode: &str, already_probed: usize, fast: usize, hidden: usize) -> usize {
    if mode == "fast" {
        fast
    } else {
        hidden.saturating_sub(already_probed)
    }
}

/// Self-transfer hubs this pass after leaving `cs_reserve` paid slots for Cs.
pub fn _self_transfer_hub_cap(mode: &str, remaining: i32, cs_reserve: i32) -> i32 {
    let want: i32 = if mode == "fast" { 2 } else { 3 };
    let per_hub: i32 = if mode == "fast" { 2 } else { 3 };
    if per_hub <= 0 {
        return 0;
    }
    let left = (remaining - cs_reserve).max(0);
    want.min(left / per_hub)
}

fn parse_retrieved_at(raw: &str) -> Option<DateTime<Utc>> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(raw) {
        return Some(dt.with_timezone(&Utc));
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(&format!("{raw}Z")) {
        return Some(dt.with_timezone(&Utc));
    }
    let compact = raw.trim_end_matches(" UTC").trim().replace(' ', "T");
    DateTime::parse_from_rfc3339(&format!("{compact}Z"))
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

fn offer_age_secs(offer: &Offer) -> Option<i64> {
    let parsed = parse_retrieved_at(offer.retrieved_at.as_deref()?.trim())?;
    Some((Utc::now() - parsed).num_seconds())
}

fn is_fresh_live_honest(offer: &Offer) -> bool {
    is_live_fare(offer)
        && offer_age_secs(offer)
            .map(|age| age <= HONEST_LIVE_MAX_AGE_SECS)
            .unwrap_or(false)
}

pub fn _reuse_indexed_honest(offers: &[Offer], mock_enabled: bool, duffel_live: bool) -> bool {
    if mock_enabled || !duffel_live {
        return false;
    }
    offers.iter().any(is_fresh_live_honest)
}

fn reuse_indexed_honest_from(
    offers: &[Offer],
    origin_u: &HashSet<String>,
    dests: &HashSet<String>,
    mock_enabled: bool,
    duffel_live: bool,
) -> bool {
    if mock_enabled || !duffel_live {
        return false;
    }
    offers.iter().any(|o| {
        o.price.is_some()
            && is_standard_to(o, dests)
            && !o.segments.is_empty()
            && origin_u.contains(&o.segments[0].origin.to_uppercase())
            && is_fresh_live_honest(o)
    })
}

struct SearchPersist {
    pool: PgPool,
    learned: Vec<Offer>,
    date: String,
    adults: i32,
    cabin: String,
    origins: HashSet<String>,
    dests: HashSet<String>,
    honest_price: Option<f64>,
    honest_ccy: Option<String>,
    expanded: Vec<String>,
    probe_origin: Option<String>,
    probe_dest: Option<String>,
    calls: Vec<ProviderCall>,
    hidden: Option<(Vec<HiddenCityMatch>, String, String, String, String)>,
    tracks: Option<(String, Vec<TrackedAircraft>, Option<i32>)>,
    search_id: Option<i32>,
    persist_offers: Vec<Offer>,
}

fn spawn_search_persist(job: SearchPersist) {
    tokio::spawn(async move {
        let _ = repo::remember_offers(
            &job.pool,
            &job.learned,
            &job.date,
            job.adults,
            &job.cabin,
        )
        .await;
        let batch = build_batch(
            &job.learned,
            &job.origins,
            &job.dests,
            job.honest_price,
            job.honest_ccy.as_deref(),
            &job.expanded,
            &job.date,
            job.adults,
            &job.cabin,
            job.probe_origin.as_deref(),
            job.probe_dest.as_deref(),
            None,
        );
        let _ = repo::apply_learning(&job.pool, &batch).await;
        let _ = repo::persist_provider_calls(&job.pool, &job.calls).await;
        if let Some((matches, origin, origin_city, dest, dest_city)) = job.hidden {
            let _ = repo::persist_hidden_deals(
                &job.pool,
                &matches,
                &origin,
                &origin_city,
                &dest,
                &dest_city,
                &job.date,
            )
            .await;
        }
        if let Some((iata, aircraft, api_time)) = job.tracks {
            let states: Vec<Value> = aircraft
                .iter()
                .filter_map(|a| serde_json::to_value(a).ok())
                .collect();
            let _ = repo::persist_tracks(&job.pool, &iata, &states, api_time).await;
        }
        if let Some(search_id) = job.search_id {
            let payload: Vec<Value> = job
                .persist_offers
                .iter()
                .filter_map(|o| serde_json::to_value(o).ok())
                .collect();
            let _ = repo::persist_search_offers(&job.pool, search_id, &payload).await;
        }
    });
}

fn members_of(place: &Airport) -> Vec<String> {
    if place.members.is_empty() {
        vec![place.iata.clone()]
    } else {
        place.members.clone()
    }
}

fn airport_kind(place: &Airport) -> &str {
    place.type_.as_str()
}

fn place_key(a: &Airport) -> &str {
    if a.place_id.is_empty() {
        &a.iata
    } else {
        &a.place_id
    }
}

fn clone_shop(req: &ShopRequest) -> ShopRequest {
    ShopRequest {
        origin: req.origin.clone(),
        dest: req.dest.clone(),
        date: req.date.clone(),
        adults: req.adults,
        cabin: req.cabin.clone(),
        currency: req.currency.clone(),
        nonstop: req.nonstop,
        max_offers: req.max_offers,
        return_date: req.return_date.clone(),
    }
}

fn shop_at(req: &ShopRequest, origin: &str, dest: &str) -> ShopRequest {
    let mut r = clone_shop(req);
    r.origin = origin.to_string();
    r.dest = dest.to_string();
    r
}

fn upper_set<I, S>(codes: I) -> HashSet<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    codes
        .into_iter()
        .map(|s| s.as_ref().to_uppercase())
        .collect()
}

pub async fn search_all_ways(
    query: SearchQuery,
    settings: &Settings,
    client: &reqwest::Client,
    pool: &PgPool,
    mode: &str,
    exclude: Option<&[String]>,
) -> anyhow::Result<SearchResponse> {
    let ledger = start_ledger(settings.search_cost_usd, settings.search_call_cap());
    LEDGER
        .scope(
            ledger,
            search_all_ways_inner(query, settings, client, pool, mode, exclude),
        )
        .await
}

async fn search_all_ways_inner(
    mut query: SearchQuery,
    settings: &Settings,
    client: &reqwest::Client,
    pool: &PgPool,
    mode: &str,
    exclude: Option<&[String]>,
) -> anyhow::Result<SearchResponse> {
    let t0 = Instant::now();
    let mode = if mode == "deep" { "deep" } else { "fast" };
    query.currency = "USD".into();
    refresh_rates(client).await;
    let fx_live = crate::fx::rates_are_live();
    let origin = query.origin.clone();
    let dest = query.destination.clone();

    let (o_ap, d_ap) = tokio::join!(
        repo::get_place(pool, &origin, false),
        repo::get_place(pool, &dest, false),
    );
    let o_ap = o_ap?;
    let d_ap = d_ap?;
    let (Some(o_ap), Some(d_ap)) = (o_ap, d_ap) else {
        anyhow::bail!(
            "Unknown IATA in the OurAirports table. Ingest has not been run, or the code is not a scheduled airport."
        );
    };
    if same_place(&o_ap, &d_ap) {
        anyhow::bail!("Origin and destination must differ.");
    }

    let duffel = if settings.duffel_enabled() {
        Some(Arc::new(DuffelProvider::new(settings, client.clone())))
    } else {
        None
    };
    let mock = if settings.mock_enabled {
        Some(Arc::new(MockProvider::new()))
    } else {
        None
    };
    let opensky = OpenSkyProvider::new(settings, client.clone());
    let box_ = if settings.aerodatabox_enabled() {
        Some(Arc::new(AeroDataBoxProvider::new(settings, client.clone())))
    } else {
        None
    };
    let round_trip = query.return_date.is_some();

    let o_tokens = shop_tokens(&o_ap);
    let d_tokens = shop_tokens(&d_ap);
    let o_members = members_of(&o_ap);
    let d_members = members_of(&d_ap);
    let d_airports: HashSet<String> = d_members.iter().cloned().collect();
    let route_origins = {
        let mut s = HashSet::new();
        s.extend(o_members.iter().cloned());
        s.extend(o_tokens.iter().cloned());
        s
    };
    let route_dests = {
        let mut s = HashSet::new();
        s.extend(d_members.iter().cloned());
        s.extend(d_tokens.iter().cloned());
        s
    };
    let city_search = airport_kind(&o_ap) == "city" || airport_kind(&d_ap) == "city";
    let req = ShopRequest {
        origin: o_tokens
            .first()
            .cloned()
            .unwrap_or_else(|| o_ap.iata.clone()),
        dest: d_tokens
            .first()
            .cloned()
            .unwrap_or_else(|| d_ap.iata.clone()),
        date: query.date.clone(),
        adults: query.adults,
        cabin: query.cabin.clone(),
        currency: "USD".into(),
        nonstop: false,
        max_offers: 20,
        return_date: query.return_date.clone(),
    };

    let mut indexed = match repo::recall_offers(
        pool,
        &route_origins,
        &query.date,
        query.adults,
        &query.cabin,
        settings.offer_ttl_seconds,
    )
    .await
    {
        Ok(v) => v,
        Err(_) => vec![],
    };
    indexed.retain(|o| !is_self_transfer(o) && offer_unexpired(o));
    if let Some(ret) = &query.return_date {
        indexed.retain(|o| o.return_date.as_deref() == Some(ret.as_str()));
    } else {
        indexed.retain(|o| o.return_date.is_none());
    }
    let origin_u = upper_set(route_origins.iter());
    let reuse_honest = reuse_indexed_honest_from(
        &indexed,
        &origin_u,
        &route_dests,
        settings.mock_enabled,
        settings.duffel_live(),
    );

    let hints = if mode == "deep" {
        vec![]
    } else {
        let spoke_origins: Vec<String> = d_airports.iter().take(4).cloned().collect();
        let spokes = repo::destinations_from_many(pool, &spoke_origins, 18)
            .await
            .unwrap_or_default();
        let hint_codes: Vec<String> = spokes.iter().map(|(c, _, _)| c.clone()).collect();
        let hint_aps = repo::airports_by_iata(pool, &hint_codes)
            .await
            .unwrap_or_default();
        hints_from_spokes(&spokes, &hint_aps, &origin)
    };

    if !reuse_honest {
        LEDGER.try_with(|l| l.hold_direct_slot()).ok();
    }
    let shop_fut = async {
        if reuse_honest {
            shop_mock_only(&req, &o_ap, &d_ap, mock.as_deref()).await
        } else {
            shop_place(&req, &o_ap, &d_ap, duffel.as_deref(), mock.as_deref()).await
        }
    };
    let (shop_local, traffic_o_opt, traffic_d_opt, board_opt) = if mode == "deep" {
        (shop_fut.await, None, None, Some(Vec::new()))
    } else {
        let o_board = o_members
            .first()
            .cloned()
            .unwrap_or_else(|| o_ap.iata.clone());
        let (g0, g1, g2, g3) = tokio::join!(
            shop_fut,
            capped(opensky.traffic_near(&o_ap)),
            capped(opensky.traffic_near(&d_ap)),
            async {
                if let Some(b) = box_.as_ref() {
                    capped(b.board(&o_board, "Departure")).await
                } else {
                    Some(Vec::new())
                }
            },
        );
        (g0, g1, g2, g3)
    };
    let mut traffic_o = traffic_placeholder(&o_ap);
    let mut traffic_d = traffic_placeholder(&d_ap);
    let mut board: Vec<BoardFlight> = Vec::new();
    if let Some(t) = traffic_o_opt {
        traffic_o = t;
    }
    if let Some(t) = traffic_d_opt {
        traffic_d = t;
    }
    if let Some(b) = board_opt {
        board = b;
    }
    LEDGER.try_with(|l| l.release_direct_hold()).ok();

    let indexed_useful: Vec<Offer> = indexed
        .iter()
        .filter(
            |o| match classify_for_search(o, &route_origins, &route_dests) {
                Some("hidden_city") => true,
                Some("standard") => is_fresh_live_honest(o),
                _ => false,
            },
        )
        .cloned()
        .collect();
    let reused_from_index = indexed_useful.len() as i32;
    let index_cache = cache_state(&indexed_useful, &shop_local);
    let mut merged = indexed_useful;
    merged.extend(shop_local);
    let mut raw_local = take_offers_in_usd(take_drop_sandbox(take_dedupe(merged)));
    if let Some(ret) = &query.return_date {
        raw_local.retain(|o| o.return_date.as_deref() == Some(ret.as_str()));
    } else {
        raw_local.retain(|o| o.return_date.is_none());
    }
    let local_offers = take_prefer_real_carriers(take_dedupe_itineraries(
        raw_local
            .iter()
            .filter(|o| _on_route(o, &route_origins, &route_dests) && !is_self_transfer(o))
            .cloned()
            .collect(),
    ));
    let has_real_carrier = local_offers.iter().any(|o| !is_test_carrier(&o.carrier));
    let priced: Vec<Offer> = local_offers
        .into_iter()
        .filter(|o| o.price.is_some())
        .collect();

    let mut nearby_offers: Vec<Offer> = Vec::new();
    if query.include_nearby && !city_search && duffel.is_some() && priced.len() < 3 && !round_trip {
        nearby_offers = shop_nearby(
            &req,
            &query,
            pool,
            duffel.as_deref(),
            &o_members,
            &d_members,
            mock.as_deref(),
        )
        .await;
        nearby_offers = nearby_offers
            .into_iter()
            .filter(|o| !_on_route(o, &route_origins, &route_dests))
            .collect();
        nearby_offers =
            take_prefer_real_carriers(take_dedupe_itineraries(take_drop_sandbox(nearby_offers)));
        if let Some(first) = priced.first() {
            let ccy = first.currency.clone();
            nearby_offers.retain(|o| o.currency == ccy);
        }
        if has_real_carrier {
            nearby_offers.retain(|o| !is_test_carrier(&o.carrier));
        }
    }

    let nonstop: Vec<Offer> = priced.iter().filter(|o| o.stops == 0).cloned().collect();
    let connecting: Vec<Offer> = priced.iter().filter(|o| o.stops > 0).cloned().collect();
    let honest_pick = pick_honest(&nonstop, &connecting, Some("USD"));
    let best_pick = pick_best(&nonstop, &connecting, honest_pick.as_ref());

    let mut rejected: Vec<String> = Vec::new();
    let mut hidden_from_local: Vec<HiddenCityMatch> = Vec::new();
    if let Some(hp) = honest_pick.as_ref() {
        if !round_trip {
            let classified = classify_hidden(
                &raw_local,
                &route_dests,
                &hp.offer,
                Some(&o_ap),
                settings.min_hidden_saving,
            );
            hidden_from_local = classified.0;
            rejected = classified.1;
        }
    }
    let mut hidden_matches = hidden_from_local;
    let mut expanded: Vec<String> = Vec::new();
    let mut pending: Vec<String> = Vec::new();
    let mut probe_offers: Vec<Offer> = Vec::new();
    let mut trace: Vec<CandidateTrace> = Vec::new();
    let covered = ticketed_dests_through(&raw_local, &route_dests);
    let mut skipped: Vec<String> = covered.iter().cloned().collect();
    skipped.sort();
    let already_probed = exclude.map(|e| e.len()).unwrap_or(0);
    let fast_n = settings.search_fast_candidates();
    let hidden_n = settings.search_hidden_candidates();
    let c_budget = hidden_probe_budget(mode, already_probed, fast_n, hidden_n);
    let mut excluded = upper_set(exclude.unwrap_or(&[]).iter());
    excluded.extend(covered.iter().cloned());
    let can_shop = (duffel.is_some() || mock.is_some()) && !round_trip;
    let mut self_transfer_offers: Vec<Offer> = Vec::new();
    let mut transfer_legs: Vec<Offer> = Vec::new();
    let mock_covered = mock.is_some() && priced.iter().any(|o| o.source == "mock");
    if duffel.is_some() && !mock_covered && !round_trip {
        match shop_self_transfers(
            &req,
            settings,
            pool,
            duffel.as_deref(),
            &o_ap,
            &d_ap,
            &route_dests,
            mode,
            c_budget as i32,
        )
        .await
        {
            Ok((st, legs)) => {
                self_transfer_offers = take_drop_sandbox(st);
                transfer_legs = take_drop_sandbox(legs);
            }
            Err(_) => {
                self_transfer_offers = vec![];
                transfer_legs = vec![];
            }
        }
    }
    if let Some(ceiling) = honest_pick.as_ref().and_then(|p| p.offer.price) {
        self_transfer_offers.retain(|o| o.price.map(|p| p < ceiling).unwrap_or(false));
    }
    if can_shop {
        if let Some(hp) = honest_pick.as_ref() {
            let mut ranked =
                match plan_candidates(pool, settings, &route_origins, &route_dests, &d_members)
                    .await
                {
                    Ok(v) => v,
                    Err(_) => vec![],
                };
            if !(mode == "fast" && !hidden_matches.is_empty()) {
                let chosen = select_candidates(
                    &mut ranked,
                    c_budget,
                    settings.candidate_min_score,
                    fast_n,
                    &excluded,
                );
                expanded = chosen.iter().map(|c| c.code.clone()).collect();
            }
            if mode == "fast" {
                let mut preview_exclude = excluded.clone();
                preview_exclude.extend(expanded.iter().cloned());
                let preview = select_candidates(
                    &mut ranked,
                    hidden_n.saturating_sub(expanded.len()),
                    settings.candidate_min_score,
                    fast_n,
                    &preview_exclude,
                );
                pending = preview.iter().map(|c| c.code.clone()).collect();
                let chosen_codes: HashSet<String> = expanded.iter().cloned().collect();
                for c in ranked.iter_mut() {
                    c.selected = chosen_codes.contains(&c.code);
                }
            }
            if !expanded.is_empty() {
                match probe_candidates(
                    &req,
                    settings,
                    duffel.as_deref(),
                    mock.as_deref(),
                    &o_ap,
                    &route_dests,
                    &hp.offer,
                    &expanded,
                )
                .await
                {
                    Ok((extra, seen)) => {
                        hidden_matches.extend(extra);
                        probe_offers = seen;
                    }
                    Err(_) => {
                        probe_offers = vec![];
                    }
                }
            }
            trace = ranked
                .iter()
                .take(20)
                .map(|c| CandidateTrace {
                    code: c.code.clone(),
                    score: c.score,
                    source: c.source.clone(),
                    observations: c.observations,
                    successful_connections: c.successful_connections,
                    cheaper_than_direct_count: c.cheaper_than_direct_count,
                    median_saving: c.median_saving,
                    parts: c.parts.clone(),
                    selected: c.selected,
                })
                .collect();
        }
    }
    hidden_matches = clean_hidden_matches(
        hidden_matches
            .into_iter()
            .filter(|m| !_is_sandbox(&m.through_offer))
            .collect(),
    );
    let ticketed_codes: Vec<String> = hidden_matches.iter().map(ticketed_code).collect();
    let mut hist_fps = Vec::new();
    let mut hist_origins = Vec::new();
    let mut hist_ticketed = Vec::new();
    for m in &hidden_matches {
        hist_fps.push(itinerary_fingerprint(&m.through_offer));
        if let Some(s) = m.through_offer.segments.first() {
            hist_origins.push(s.origin.to_uppercase());
        }
        hist_ticketed.push(ticketed_code(m));
    }
    let (ticketed_aps, series) = tokio::join!(
        repo::airports_by_iata(pool, &ticketed_codes),
        repo::fare_series(pool, &hist_fps, &hist_origins, &hist_ticketed),
    );
    apply_ticketed_airports(
        &mut hidden_matches,
        &o_ap.country,
        &ticketed_aps.unwrap_or_default(),
    );
    attach_windows(&mut hidden_matches, &series.unwrap_or_default(), chrono::Utc::now());
    sort_matches(&mut hidden_matches);

    let mut learned_src = raw_local;
    learned_src.extend(probe_offers);
    learned_src.extend(transfer_legs);
    learned_src.extend(nearby_offers.iter().cloned());
    learned_src.extend(hidden_matches.iter().map(|m| m.through_offer.clone()));
    let mut learned_offers = take_drop_sandbox(take_dedupe(learned_src));
    learned_offers.retain(|o| !is_self_transfer(o));

    let (paid_len, total_cost, calls) = LEDGER
        .try_with(|l| (l.paid().len(), l.total_cost(), l.calls()))
        .unwrap_or((0, 0.0, vec![]));

    let hidden_if_cheaper = _hidden_if_cheaper(&hidden_matches, honest_pick.as_ref());
    let search_debug = SearchDebug {
        providers: {
            let mut p = Vec::new();
            if mock.is_some() {
                p.push("mock".into());
            }
            if duffel.is_some() {
                p.push("duffel".into());
            }
            p
        },
        standard_query: {
            let mut s = format!("{}→{}", req.origin, req.dest);
            if round_trip {
                s.push_str(&format!("→{}", req.origin));
            }
            s
        },
        mode: mode.to_string(),
        expanded_destinations: expanded.clone(),
        pending_candidates: pending,
        skipped_expansion: skipped,
        candidates: trace,
        provider_calls: paid_len as i32,
        provider_cost_usd: total_cost,
        reused_from_index,
        reused_honest: reuse_honest,
        fx_live,
        rejected: rejected.into_iter().take(24).collect(),
        cache: index_cache,
        pending_self_transfer: duffel.is_some() && !mock_covered && !round_trip && mode == "fast",
    };

    let compare_ccy = honest_pick.as_ref().map(|p| p.offer.currency.as_str());
    let currency_ok =
        |o: &Offer| o.price.is_some() && compare_ccy.map(|c| o.currency == c).unwrap_or(true);
    let cheapest_local = priced
        .iter()
        .filter(|o| currency_ok(o))
        .filter_map(|o| o.price)
        .fold(None, |acc, p| Some(acc.map(|a: f64| a.min(p)).unwrap_or(p)));
    let cheapest_any = priced
        .iter()
        .chain(nearby_offers.iter())
        .chain(self_transfer_offers.iter())
        .chain(hidden_matches.iter().map(|m| &m.through_offer))
        .filter(|o| currency_ok(o))
        .filter_map(|o| o.price)
        .fold(None, |acc, p| Some(acc.map(|a: f64| a.min(p)).unwrap_or(p)));

    let mut name_codes: HashSet<String> = HashSet::new();
    for o in &priced {
        if !o.carrier.is_empty() {
            name_codes.insert(o.carrier.clone());
        }
        if let Some(v) = &o.validating_airline {
            if !v.is_empty() {
                name_codes.insert(v.clone());
            }
        }
        for s in &o.segments {
            if !s.carrier.is_empty() {
                name_codes.insert(s.carrier.clone());
            }
        }
    }
    for m in &hidden_matches {
        if !m.through_offer.carrier.is_empty() {
            name_codes.insert(m.through_offer.carrier.clone());
        }
    }
    for o in nearby_offers.iter().chain(self_transfer_offers.iter()) {
        if !o.carrier.is_empty() {
            name_codes.insert(o.carrier.clone());
        }
        for s in &o.segments {
            if !s.carrier.is_empty() {
                name_codes.insert(s.carrier.clone());
            }
        }
    }
    let codes_vec: Vec<String> = name_codes.into_iter().collect();
    let airline_pairs = repo::airlines_by_iata(pool, &codes_vec)
        .await
        .unwrap_or_default();
    let book_ccy = "USD";
    let bookers = booker_links(
        &o_ap.iata,
        &d_ap.iata,
        &query.date,
        query.adults,
        &airline_pairs[..airline_pairs.len().min(6)],
        book_ccy,
        &query.cabin,
        query.return_date.as_deref(),
    );
    let airline_names: HashMap<String, String> = airline_pairs.into_iter().collect();
    for match_ in hidden_matches.iter_mut() {
        let code = match_.through_offer.carrier.to_uppercase();
        let thru_al: Vec<(String, String)> = if let Some(name) = airline_names.get(&code) {
            vec![(code.clone(), name.clone())]
        } else {
            vec![]
        };
        match_.bookers = booker_links(
            &o_ap.iata,
            &match_.hidden_city,
            &query.date,
            query.adults,
            &thru_al,
            book_ccy,
            &query.cabin,
            None,
        );
    }

    let persist_tracks = if !traffic_o.aircraft.is_empty() {
        Some((
            o_members
                .first()
                .cloned()
                .unwrap_or_else(|| o_ap.iata.clone()),
            traffic_o.aircraft.clone(),
            traffic_o.api_time.map(|t| t as i32),
        ))
    } else {
        None
    };
    traffic_o.aircraft.truncate(8);
    traffic_d.aircraft.truncate(8);
    board.truncate(12);

    let persist_hidden = if !hidden_matches.is_empty() && !round_trip {
        Some((
            hidden_matches.clone(),
            o_ap.iata.chars().take(3).collect::<String>(),
            o_ap.city.clone(),
            d_ap.iata.chars().take(3).collect::<String>(),
            d_ap.city.clone(),
        ))
    } else {
        None
    };

    let channels = vec![
        ChannelGroup {
            kind: "nonstop".into(),
            label: "Priced nonstop A → B".into(),
            blurb: "GDS/NDC priced offers that end at your destination with one flight. A search offer is not a ticket.".into(),
            offers: {
                let mut v = nonstop;
                v.sort_by_key(|o| honest_rank_key(o));
                v
            },
        },
        ChannelGroup {
            kind: "connecting".into(),
            label: "Priced connecting itineraries that end at B".into(),
            blurb: "Honest through products ticketed to B. Different from hidden-city A→B→C.".into(),
            offers: {
                let mut v = connecting;
                v.sort_by_key(|o| honest_rank_key(o));
                v
            },
        },
        ChannelGroup {
            kind: "nearby".into(),
            label: "Priced nearby-airport substitutes".into(),
            blurb: "Same trip intent, different IATA. Still a real A′→B′ offer, not skiplagging.".into(),
            offers: {
                let mut v: Vec<Offer> = nearby_offers.into_iter().filter(|o| o.price.is_some()).collect();
                v.sort_by_key(|o| honest_rank_key(o));
                v
            },
        },
        ChannelGroup {
            kind: "self-transfer".into(),
            label: "Self-transfer (separate tickets)".into(),
            blurb: "Two or three independently priced tickets you buy yourself. A missed connection is not protected. Not hidden-city.".into(),
            offers: self_transfer_offers,
        },
        ChannelGroup {
            kind: "hidden-city".into(),
            label: "Priced hidden-city inversions".into(),
            blurb: "Complete tickets that continue past the intended city. The priced itinerary is never rewritten.".into(),
            offers: hidden_matches.iter().map(|m| m.through_offer.clone()).collect(),
        },
    ];

    let mut sources_used = vec![
        "ourairports".into(),
        "openflights-routes".into(),
        "opensky".into(),
        "google-flights".into(),
        "kayak".into(),
        "skyscanner".into(),
        "momondo".into(),
        "cheapflights".into(),
        "wego".into(),
        "expedia".into(),
        "booking-com".into(),
        "trip-com".into(),
        "priceline".into(),
        "kiwi".into(),
        "cheapoair".into(),
        "edreams".into(),
        "traveloka".into(),
        "makemytrip".into(),
        "despegar".into(),
        "skiplagged-com".into(),
    ];
    if duffel.is_some() {
        sources_used.push("duffel".into());
    }
    if mock.is_some() {
        sources_used.push("mock".into());
    }
    if box_.is_some() {
        sources_used.push("aerodatabox".into());
    }

    let gaps = gaps(
        settings,
        duffel.as_deref(),
        box_.as_deref(),
        &priced,
        &hidden_matches,
        &traffic_o,
        &board,
        mock.as_deref(),
        round_trip,
        reuse_honest,
        reused_from_index,
        fx_live,
    );
    let notes = vec![
        "Reference / track / schedule / priced-offer / booker are different layers. A tracker is not a ticket. A metasearch link is not a PNR.".into(),
        "Ranking: this is a price comparator first. Cheapest regular ticket, then the best (fewest stops) and fastest. Self-transfer and hidden-city rows are additions shown only when they cost less than the cheapest regular ticket.".into(),
        "Hidden-city rows are complete one-way tickets that continue past the intended city. The priced itinerary is never rewritten into a fake A→B fare. Round-trip searches compare honest return tickets only.".into(),
        "Every priced row is USD before ranking. A Duffel USD amount on the offer is preferred. Frankfurter converts the rest only after a successful fetch (`fetched_at`). Baked-in rates are never used to rank.".into(),
        "Self-transfer rows are separate tickets stitched at a hub, at most five flights. The sum is not one PNR and is not hidden-city.".into(),
        "This API never calls Duffel Orders.".into(),
    ];

    let elapsed = (t0.elapsed().as_secs_f64() * 1000.0 * 10.0).round() / 10.0;
    let persist_offers: Vec<Offer> = priced
        .iter()
        .take(30)
        .cloned()
        .chain(hidden_matches.iter().take(15).map(|m| m.through_offer.clone()))
        .collect();
    let search_id = repo::persist_search(
        pool,
        &o_ap.iata.chars().take(3).collect::<String>(),
        &d_ap.iata.chars().take(3).collect::<String>(),
        &query.date,
        query.adults,
        &query.cabin,
        &query.currency,
        elapsed,
        &sources_used,
        &[],
    )
    .await
    .ok();
    spawn_search_persist(SearchPersist {
        pool: pool.clone(),
        learned: learned_offers,
        date: query.date.clone(),
        adults: query.adults,
        cabin: query.cabin.clone(),
        origins: route_origins,
        dests: route_dests,
        honest_price: honest_pick.as_ref().and_then(|p| p.offer.price),
        honest_ccy: honest_pick.as_ref().map(|p| p.offer.currency.clone()),
        expanded: expanded.clone(),
        probe_origin: Some(req.origin.clone()),
        probe_dest: Some(req.dest.clone()),
        calls,
        hidden: persist_hidden,
        tracks: persist_tracks,
        search_id,
        persist_offers,
    });

    Ok(SearchResponse {
        query,
        origin: o_ap,
        destination: d_ap,
        elapsed_ms: elapsed,
        search_id,
        sources_used,
        data_gaps: gaps,
        cheapest_local,
        cheapest_any,
        honest_pick,
        best_pick,
        hidden_if_cheaper,
        channels,
        hidden_city: hidden_matches,
        connection_hints: hints,
        bookers,
        airline_names,
        traffic_origin: Some(traffic_o),
        traffic_destination: Some(traffic_d),
        board_origin: board,
        notes,
        search_debug: Some(search_debug),
    })
}

async fn capped<T, E>(fut: impl std::future::Future<Output = Result<T, E>>) -> Option<T> {
    match tokio::time::timeout(SIDE_TIMEOUT, fut).await {
        Ok(Ok(v)) => Some(v),
        _ => None,
    }
}

pub async fn _empty_list() -> Vec<Offer> {
    vec![]
}

pub async fn _capped<T, E>(fut: impl std::future::Future<Output = Result<T, E>>) -> Option<T> {
    capped(fut).await
}

pub fn _cache_state<T, U>(indexed: &[T], shopped: &[U]) -> String {
    cache_state_len(!indexed.is_empty(), !shopped.is_empty())
}

fn cache_state_len(indexed: bool, shopped: bool) -> String {
    if indexed && !shopped {
        "index".into()
    } else if indexed {
        "index+live".into()
    } else {
        "miss".into()
    }
}

fn cache_state(indexed: &[Offer], shopped: &[Offer]) -> String {
    _cache_state(indexed, shopped)
}

pub fn _needs_city_nonstop(
    origin_place: &Airport,
    dest_place: &Airport,
    pair_index: usize,
) -> bool {
    pair_index == 0 && (airport_kind(origin_place) == "city" || airport_kind(dest_place) == "city")
}

fn traffic_placeholder(place: &Airport) -> LiveTraffic {
    let members = members_of(place);
    let iata = members
        .first()
        .cloned()
        .unwrap_or_else(|| place.iata.clone());
    let icao = place
        .icao
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| iata.clone());
    LiveTraffic {
        airport: place.iata.clone(),
        source: "opensky".into(),
        layer: "live-track".into(),
        api_time: None,
        note: "Search does not wait on OpenSky. Use the tracker links for live aircraft.".into(),
        aircraft: vec![],
        trackers: tracker_links(&iata, &icao),
    }
}

fn hints_from_spokes(
    spokes: &[(String, String, i64)],
    airports: &HashMap<String, Airport>,
    origin: &str,
) -> Vec<ConnectionHint> {
    let mut hints = Vec::new();
    let mut seen = HashSet::new();
    let origin_u = origin.to_uppercase().chars().take(3).collect::<String>();
    for (code, airline, n) in spokes {
        if *code == origin_u || seen.contains(code) {
            continue;
        }
        seen.insert(code.clone());
        let dest_name = airports
            .get(code)
            .map(|ap| format!("{} ({code})", ap.city))
            .unwrap_or_else(|| code.clone());
        hints.push(ConnectionHint {
            dest: code.clone(),
            dest_name,
            evidence: format!("{airline} appears on {n} OpenFlights B→C record(s)"),
            source: "openflights-routes".into(),
            layer: "historical-route-map".into(),
            note: "Historical route map (~2014–2017). Not a priced fare and not proof the flight operates tomorrow.".into(),
        });
        if hints.len() >= 24 {
            break;
        }
    }
    hints
}

pub fn shop_tokens(place: &Airport) -> Vec<String> {
    let members: Vec<String> = place
        .members
        .iter()
        .filter(|m| !m.is_empty())
        .cloned()
        .collect();
    if airport_kind(place) == "city" {
        let mut tokens = Vec::new();
        if !place.iata.is_empty() {
            tokens.push(place.iata.clone());
        }
        for code in members {
            if !tokens.contains(&code) {
                tokens.push(code);
            }
        }
        tokens.truncate(4);
        if tokens.is_empty() {
            vec![place.iata.clone()]
        } else {
            tokens
        }
    } else {
        vec![place.iata.clone()]
    }
}

pub fn _route_pairs(origins: &[String], dests: &[String]) -> Vec<(String, String)> {
    route_pairs_limit(origins, dests, 8)
}

pub fn route_pairs(origins: &[String], dests: &[String]) -> Vec<(String, String)> {
    _route_pairs(origins, dests)
}

fn route_pairs_limit(origins: &[String], dests: &[String], limit: usize) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    let mut seen = HashSet::new();
    let mut add = |origin: &str, dest: &str| {
        let key = (origin.to_uppercase(), dest.to_uppercase());
        if !origin.is_empty() && !dest.is_empty() && key.0 != key.1 && seen.insert(key.clone()) {
            pairs.push((origin.to_string(), dest.to_string()));
        }
    };
    if !origins.is_empty() && !dests.is_empty() {
        add(&origins[0], &dests[0]);
    }
    let extra_o: &[String] = if origins.len() > 1 {
        &origins[1..]
    } else {
        &origins[..origins.len().min(1)]
    };
    let extra_d: &[String] = if dests.len() > 1 {
        &dests[1..]
    } else {
        &dests[..dests.len().min(1)]
    };
    for origin in extra_o {
        for dest in extra_d {
            add(origin, dest);
        }
    }
    pairs.truncate(limit);
    pairs
}

fn same_place(a: &Airport, b: &Airport) -> bool {
    if place_key(a) == place_key(b) {
        return true;
    }
    let am: HashSet<String> = members_of(a).into_iter().collect();
    let bm: HashSet<String> = members_of(b).into_iter().collect();
    if airport_kind(a) == "city" && am.contains(&b.iata) {
        return true;
    }
    if airport_kind(b) == "city" && bm.contains(&a.iata) {
        return true;
    }
    false
}

pub async fn _shop_mock_only(
    req: &ShopRequest,
    origin: &Airport,
    dest: &Airport,
    mock: Option<&MockProvider>,
) -> Vec<Offer> {
    shop_mock_only(req, origin, dest, mock).await
}

async fn shop_mock_only(
    req: &ShopRequest,
    origin: &Airport,
    dest: &Airport,
    mock: Option<&MockProvider>,
) -> Vec<Offer> {
    let Some(mock) = mock else {
        return vec![];
    };
    let pairs = _route_pairs(&shop_tokens(origin), &shop_tokens(dest));
    if pairs.is_empty() {
        return vec![];
    }
    let parts = join_all(pairs.into_iter().map(|(o, d)| {
        let r = shop_at(req, &o, &d);
        async move { mock.shop(&r).await }
    }))
    .await;
    let mut out = Vec::new();
    for part in parts {
        if let Ok(list) = part {
            out.extend(list);
        }
    }
    out
}

pub async fn _shop_place(
    req: &ShopRequest,
    origin: &Airport,
    dest: &Airport,
    duffel: Option<&DuffelProvider>,
    mock: Option<&MockProvider>,
) -> Vec<Offer> {
    shop_place(req, origin, dest, duffel, mock).await
}

async fn shop_place(
    req: &ShopRequest,
    origin: &Airport,
    dest: &Airport,
    duffel: Option<&DuffelProvider>,
    mock: Option<&MockProvider>,
) -> Vec<Offer> {
    let pairs = _route_pairs(&shop_tokens(origin), &shop_tokens(dest));
    if pairs.is_empty() {
        return vec![];
    }
    let sem = Arc::new(Semaphore::new(3));
    let origin_c = origin.clone();
    let dest_c = dest.clone();
    let req_c = clone_shop(req);
    let parts = join_all(pairs.into_iter().enumerate().map(|(idx, (o, d))| {
        let sem = sem.clone();
        let origin_c = origin_c.clone();
        let dest_c = dest_c.clone();
        let req_c = clone_shop(&req_c);
        async move {
            let _g = sem.acquire().await.ok()?;
            let shop = shop_at(&req_c, &o, &d);
            Some(
                _shop_providers(
                    &shop,
                    duffel,
                    _needs_city_nonstop(&origin_c, &dest_c, idx),
                    mock,
                    "direct",
                )
                .await,
            )
        }
    }))
    .await;
    let mut out = Vec::new();
    for part in parts {
        if let Some(list) = part {
            out.extend(list);
        }
    }
    out
}

pub async fn _shop_pairs(
    req: &ShopRequest,
    origins: &[String],
    dests: &[String],
    duffel: Option<&DuffelProvider>,
) -> Vec<Offer> {
    let pairs = _route_pairs(origins, dests);
    if pairs.is_empty() {
        return vec![];
    }
    let parts = join_all(pairs.into_iter().map(|(o, d)| {
        let r = shop_at(req, &o, &d);
        async move { _shop_providers(&r, duffel, false, None, "direct").await }
    }))
    .await;
    let mut out = Vec::new();
    for part in parts {
        out.extend(part);
    }
    out
}

pub async fn _shop_providers(
    req: &ShopRequest,
    duffel: Option<&DuffelProvider>,
    extra_nonstop: bool,
    mock: Option<&MockProvider>,
    purpose: &str,
) -> Vec<Offer> {
    if let Some(mock) = mock {
        let mocked = timed_shop(
            "mock",
            &req.origin,
            &req.dest,
            &req.date,
            purpose,
            mock.shop(req),
        )
        .await;
        if !mocked.is_empty() {
            return take_offers_in_usd(take_drop_sandbox(mocked));
        }
    }
    let mut parts: Vec<Vec<Offer>> = Vec::new();
    if let Some(duffel) = duffel {
        let extra = extra_nonstop && !req.nonstop;
        let parallel_extra = extra && LEDGER.try_with(|l| l.remaining() >= 2).unwrap_or(true);
        if extra && parallel_extra {
            let mut r = clone_shop(req);
            r.nonstop = true;
            r.max_offers = 12;
            let (connecting, nonstop) = tokio::join!(
                timed_shop(
                    "duffel",
                    &req.origin,
                    &req.dest,
                    &req.date,
                    purpose,
                    duffel.shop(req),
                ),
                timed_shop(
                    "duffel",
                    &r.origin,
                    &r.dest,
                    &r.date,
                    purpose,
                    duffel.shop(&r),
                ),
            );
            parts.push(connecting);
            parts.push(nonstop);
        } else {
            parts.push(
                timed_shop(
                    "duffel",
                    &req.origin,
                    &req.dest,
                    &req.date,
                    purpose,
                    duffel.shop(req),
                )
                .await,
            );
            if extra {
                let mut r = clone_shop(req);
                r.nonstop = true;
                r.max_offers = 12;
                parts.push(
                    timed_shop(
                        "duffel",
                        &r.origin,
                        &r.dest,
                        &r.date,
                        purpose,
                        duffel.shop(&r),
                    )
                    .await,
                );
            }
        }
    }
    if parts.is_empty() {
        return vec![];
    }
    let mut out = Vec::new();
    for part in parts {
        out.extend(part);
    }
    take_offers_in_usd(take_drop_sandbox(take_unexpired(out)))
}

pub async fn _shop_nearby(
    req: &ShopRequest,
    query: &SearchQuery,
    pool: &PgPool,
    duffel: Option<&DuffelProvider>,
    origin_members: &[String],
    dest_members: &[String],
    mock: Option<&MockProvider>,
) -> Vec<Offer> {
    shop_nearby(req, query, pool, duffel, origin_members, dest_members, mock).await
}

async fn shop_nearby(
    req: &ShopRequest,
    _query: &SearchQuery,
    pool: &PgPool,
    duffel: Option<&DuffelProvider>,
    origin_members: &[String],
    dest_members: &[String],
    mock: Option<&MockProvider>,
) -> Vec<Offer> {
    let skip_o: HashSet<String> = if origin_members.is_empty() {
        HashSet::from([req.origin.clone()])
    } else {
        origin_members.iter().cloned().collect()
    };
    let skip_d: HashSet<String> = if dest_members.is_empty() {
        HashSet::from([req.dest.clone()])
    } else {
        dest_members.iter().cloned().collect()
    };
    let anchor_o = origin_members
        .first()
        .cloned()
        .unwrap_or_else(|| req.origin.clone());
    let anchor_d = dest_members
        .first()
        .cloned()
        .unwrap_or_else(|| req.dest.clone());
    let near_o: Vec<Airport> = repo::nearby_airports(pool, &anchor_o, 90.0)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|a| !skip_o.contains(&a.iata))
        .collect();
    let near_d: Vec<Airport> = repo::nearby_airports(pool, &anchor_d, 90.0)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|a| !skip_d.contains(&a.iata))
        .collect();
    let mut origins = vec![req.origin.clone()];
    origins.extend(near_o.iter().take(2).map(|a| a.iata.clone()));
    let mut dests = vec![req.dest.clone()];
    dests.extend(near_d.iter().take(2).map(|a| a.iata.clone()));
    let mut pairs = Vec::new();
    for o in &origins {
        for d in &dests {
            if (o.as_str(), d.as_str()) != (req.origin.as_str(), req.dest.as_str()) && o != d {
                pairs.push((o.clone(), d.clone()));
            }
        }
    }
    pairs.truncate(2);
    if pairs.is_empty() || duffel.is_none() {
        return vec![];
    }
    let parts = join_all(pairs.into_iter().map(|(o, d)| {
        let mut r = shop_at(req, &o, &d);
        r.nonstop = true;
        r.max_offers = 5;
        async move {
            let mut offers = _shop_providers(&r, duffel, false, mock, "nearby").await;
            for offer in offers.iter_mut() {
                offer.kind = "nearby".into();
            }
            offers
        }
    }))
    .await;
    let mut out = Vec::new();
    for part in parts {
        out.extend(part);
    }
    out
}

pub async fn _shop_self_transfers(
    req: &ShopRequest,
    settings: &Settings,
    pool: &PgPool,
    duffel: Option<&DuffelProvider>,
    origin_place: &Airport,
    dest_place: &Airport,
    intended: &HashSet<String>,
    mode: &str,
    cs_reserve: i32,
) -> anyhow::Result<(Vec<Offer>, Vec<Offer>)> {
    shop_self_transfers(
        req,
        settings,
        pool,
        duffel,
        origin_place,
        dest_place,
        intended,
        mode,
        cs_reserve,
    )
    .await
}

async fn shop_self_transfers(
    req: &ShopRequest,
    settings: &Settings,
    pool: &PgPool,
    duffel: Option<&DuffelProvider>,
    origin_place: &Airport,
    dest_place: &Airport,
    intended: &HashSet<String>,
    mode: &str,
    cs_reserve: i32,
) -> anyhow::Result<(Vec<Offer>, Vec<Offer>)> {
    let origin = members_of(origin_place)
        .into_iter()
        .next()
        .unwrap_or_else(|| origin_place.iata.clone());
    let dest = members_of(dest_place)
        .into_iter()
        .next()
        .unwrap_or_else(|| dest_place.iata.clone());
    let from_codes: Vec<String> = members_of(origin_place).into_iter().take(3).collect();
    let from_origin: HashSet<String> = repo::destinations_from_many(pool, &from_codes, 24)
        .await?
        .into_iter()
        .map(|(c, _, _)| c)
        .collect();
    let mut into: HashSet<String> = HashSet::new();
    for code in members_of(dest_place).into_iter().take(3) {
        into.extend(repo::origins_into(pool, &code, 24).await?);
    }
    let mut exclude = HashSet::from([origin.clone(), dest.clone()]);
    exclude.extend(members_of(origin_place));
    exclude.extend(members_of(dest_place));
    let extra = repo::busiest_airports(pool, 20, &exclude).await?;
    let mut continent_codes = vec![origin.clone(), dest.clone()];
    continent_codes.extend(from_origin.iter().cloned());
    continent_codes.extend(into.iter().cloned());
    continent_codes.extend(extra.iter().cloned());
    let continents = repo::continents_for(pool, &continent_codes).await?;
    let remaining = LEDGER
        .try_with(|l| l.remaining() as i32)
        .unwrap_or(settings.search_call_cap());
    let n_hubs = _self_transfer_hub_cap(mode, remaining, cs_reserve);
    if n_hubs < 1 {
        return Ok((vec![], vec![]));
    }
    let hubs = pick_transfer_hubs(
        &origin,
        &dest,
        &from_origin,
        &into,
        n_hubs as usize,
        extra.as_slice(),
        &continents,
    );
    if hubs.is_empty() {
        return Ok((vec![], vec![]));
    }

    let keep_legs = |offers: Vec<Offer>, dest_ok: &HashSet<String>| -> Vec<Offer> {
        let mut kept: Vec<Offer> = _prefer_real_carriers(
            offers
                .into_iter()
                .filter(|o| {
                    is_standard_to(o, dest_ok)
                        && detect_hidden_city(o, intended).is_none()
                        && !o.segments.is_empty()
                        && o.segments.len() <= MAX_LEG_FLIGHTS as usize
                })
                .collect(),
        );
        kept.sort_by(|a, b| {
            a.stops
                .cmp(&b.stops)
                .then_with(|| {
                    let pa = a.price.unwrap_or(1e12);
                    let pb = b.price.unwrap_or(1e12);
                    pa.partial_cmp(&pb).unwrap_or(std::cmp::Ordering::Equal)
                })
                .then_with(|| a.duration_min.cmp(&b.duration_min))
        });
        kept.truncate(4);
        kept
    };

    async fn leg(
        req: &ShopRequest,
        duffel: Option<&DuffelProvider>,
        o: &str,
        d: &str,
        date: &str,
    ) -> Vec<Offer> {
        let mut r = shop_at(req, o, d);
        r.date = date.to_string();
        r.nonstop = false;
        r.max_offers = 8;
        _shop_providers(&r, duffel, false, None, "interline").await
    }

    let inbound_rows = join_all(hubs.iter().map(|hub| {
        let req = clone_shop(req);
        let hub = hub.clone();
        async move {
            let offers = leg(&req, duffel, &req.origin, &hub, &req.date).await;
            (hub, offers)
        }
    }))
    .await;
    let outbound_rows = join_all(hubs.iter().map(|hub| {
        let req = clone_shop(req);
        let hub = hub.clone();
        let dates = if mode == "fast" {
            vec![req.date.clone()]
        } else {
            vec![req.date.clone(), next_day(&req.date)]
        };
        async move {
            let parts = join_all(dates.into_iter().map(|d| {
                let req = clone_shop(&req);
                let hub = hub.clone();
                async move { leg(&req, duffel, &hub, &req.dest, &d).await }
            }))
            .await;
            let mut pooled = Vec::new();
            for part in parts {
                pooled.extend(part);
            }
            (hub, pooled)
        }
    }))
    .await;

    let mut inbound: HashMap<String, Vec<Offer>> = HashMap::new();
    let mut outbound: HashMap<String, Vec<Offer>> = HashMap::new();
    for (hub, offers) in inbound_rows {
        let dest_ok = HashSet::from([hub.clone()]);
        inbound.insert(hub, keep_legs(offers, &dest_ok));
    }
    for (hub, offers) in outbound_rows {
        outbound.insert(hub, keep_legs(offers, intended));
    }

    let mut mids: HashMap<(String, String), Vec<Offer>> = HashMap::new();
    if mode == "deep" {
        for (h1, h2) in bridge_pairs(&hubs, &from_origin, &into, 2) {
            let mid = leg(req, duffel, &h1, &h2, &req.date).await;
            let dest_ok = HashSet::from([h2.clone()]);
            let kept_mid = keep_legs(mid, &dest_ok);
            if !kept_mid.is_empty() {
                mids.insert((h1, h2), kept_mid);
            }
        }
    }
    let mut legs = Vec::new();
    for group in inbound
        .values()
        .chain(outbound.values())
        .chain(mids.values())
    {
        legs.extend(group.iter().cloned());
    }
    Ok((
        combine_at_hubs(&inbound, &outbound, intended, Some(&mids), 8),
        _dedupe(&legs),
    ))
}

pub async fn _plan_candidates(
    pool: &PgPool,
    settings: &Settings,
    route_origins: &HashSet<String>,
    route_dests: &HashSet<String>,
    dest_members: &[String],
) -> anyhow::Result<Vec<Candidate>> {
    plan_candidates(pool, settings, route_origins, route_dests, dest_members).await
}

async fn plan_candidates(
    pool: &PgPool,
    settings: &Settings,
    route_origins: &HashSet<String>,
    route_dests: &HashSet<String>,
    dest_members: &[String],
) -> anyhow::Result<Vec<Candidate>> {
    let origin = route_origins.iter().cloned().min().unwrap_or_default();
    let stats = repo::load_route_stats(pool, route_origins, route_dests).await?;
    let recall = (settings.search_hidden_candidates() * 3) as i64;
    let learned_edges = repo::learned_beyond(pool, route_dests, recall)
        .await
        .unwrap_or_default();
    let mut pool_map: HashMap<String, String> = HashMap::new();
    let dest_slice: Vec<String> = dest_members.iter().take(4).cloned().collect();
    let route_map = repo::route_map_from(pool, &dest_slice, 60).await?;
    for spokes in route_map.values() {
        let mut codes: Vec<String> = spokes.iter().cloned().collect();
        codes.sort();
        for c in codes {
            assign_source(&mut pool_map, c, "openflights");
        }
    }
    for c in repo::recall_candidate_dests(
        pool,
        route_origins,
        route_dests,
        "",
        0,
        "",
        30 * 24 * 3600,
        (settings.search_hidden_candidates() * 3) as i64,
        true,
    )
    .await?
    {
        assign_source(&mut pool_map, c, "index");
    }
    for c in learned_edges.keys() {
        assign_source(&mut pool_map, c.clone(), "edges");
    }
    for c in stats.keys() {
        assign_source(&mut pool_map, c.clone(), "stats");
    }
    let mut learned_hubs: HashSet<String> = stats.keys().cloned().collect();
    learned_hubs.extend(learned_edges.keys().cloned());
    let hub_prob = hub_probabilities(route_dests, &route_map, &learned_hubs);
    let mut dest_codes: Vec<String> = pool_map.keys().cloned().collect();
    dest_codes.extend(stats.keys().cloned());
    let provider_rate = repo::provider_rates_for(pool, route_origins, &dest_codes).await?;
    Ok({
        let mut ranked = rank_candidates(
            &origin,
            route_dests,
            &stats,
            &pool_map,
            &hub_prob,
            &provider_rate,
            None,
        );
        ranked.retain(|c| {
            !route_origins
                .iter()
                .any(|o| o.eq_ignore_ascii_case(&c.code))
        });
        ranked
    })
}

pub async fn _probe_candidates(
    req: &ShopRequest,
    settings: &Settings,
    duffel: Option<&DuffelProvider>,
    mock: Option<&MockProvider>,
    origin_place: &Airport,
    intended: &HashSet<String>,
    local: &Offer,
    candidates: &[String],
) -> anyhow::Result<(Vec<HiddenCityMatch>, Vec<Offer>)> {
    probe_candidates(
        req,
        settings,
        duffel,
        mock,
        origin_place,
        intended,
        local,
        candidates,
    )
    .await
}

async fn probe_candidates(
    req: &ShopRequest,
    settings: &Settings,
    duffel: Option<&DuffelProvider>,
    mock: Option<&MockProvider>,
    origin_place: &Airport,
    intended: &HashSet<String>,
    local: &Offer,
    candidates: &[String],
) -> anyhow::Result<(Vec<HiddenCityMatch>, Vec<Offer>)> {
    if candidates.is_empty() || (duffel.is_none() && mock.is_none()) {
        return Ok((vec![], vec![]));
    }
    let sem = Arc::new(Semaphore::new(settings.max_concurrency.max(1)));
    let origin_place = origin_place.clone();
    let local = local.clone();
    let intended = intended.clone();
    let parts = join_all(candidates.iter().map(|c| {
        let sem = sem.clone();
        let mut r = clone_shop(req);
        r.dest = c.clone();
        r.nonstop = false;
        r.max_offers = 20;
        r.return_date = None;
        let origin_place = origin_place.clone();
        let local = local.clone();
        let intended = intended.clone();
        async move {
            let _g = sem.acquire().await.ok()?;
            let throughs = _shop_providers(&r, duffel, false, mock, "expand").await;
            let (matches, _) = classify_hidden(
                &throughs,
                &intended,
                &local,
                Some(&origin_place),
                settings.min_hidden_saving,
            );
            Some((matches, throughs))
        }
    }))
    .await;
    let mut matches = Vec::new();
    let mut seen = Vec::new();
    for part in parts {
        if let Some((m, t)) = part {
            matches.extend(m);
            seen.extend(t);
        }
    }
    Ok((matches, seen))
}

pub fn _classify_hidden(
    offers: &[Offer],
    intended: &HashSet<String>,
    local: &Offer,
    origin_place: Option<&Airport>,
    min_saving: f64,
) -> (Vec<HiddenCityMatch>, Vec<String>) {
    classify_hidden(offers, intended, local, origin_place, min_saving)
}

fn ticketed_code(m: &HiddenCityMatch) -> String {
    if m.ticketed_destination.is_empty() {
        m.hidden_city.clone()
    } else {
        m.ticketed_destination.clone()
    }
}

fn apply_ticketed_airports(
    matches: &mut [HiddenCityMatch],
    origin_country: &str,
    ticketed_aps: &HashMap<String, Airport>,
) {
    for m in matches {
        let ticketed = ticketed_code(m);
        let Some(ap) = ticketed_aps.get(&ticketed) else {
            continue;
        };
        if !ap.city.is_empty() {
            m.hidden_city_name = format!("{} ({})", ap.city, ticketed);
        }
        m.risk = assess(
            &m.local_offer,
            &m.through_offer,
            &m.hidden_city,
            &m.intended_destination,
            origin_country,
            &ap.country,
        );
    }
}

fn classify_hidden(
    offers: &[Offer],
    intended: &HashSet<String>,
    local: &Offer,
    origin_place: Option<&Airport>,
    min_saving: f64,
) -> (Vec<HiddenCityMatch>, Vec<String>) {
    let local_owned = if local.currency.eq_ignore_ascii_case("USD") && local.price.is_some() {
        None
    } else {
        Some(offer_in_usd(local).filter(|o| o.price.is_some()))
    };
    let local_usd = match &local_owned {
        None => local,
        Some(Some(o)) => o,
        Some(None) => {
            return (
                vec![],
                vec!["honest fare FX unverified (could not convert to USD)".into()],
            );
        }
    };
    let mut matches = Vec::new();
    let mut rejected = Vec::new();
    for offer in offers {
        if offer.price.is_none() {
            continue;
        }
        let converted_owned = if offer.currency.eq_ignore_ascii_case("USD") {
            None
        } else if let Some(o) = offer_in_usd(offer).filter(|o| o.price.is_some()) {
            Some(o)
        } else {
            rejected.push(format!(
                "{}: FX unverified ({})",
                offer.id,
                if offer.currency.is_empty() {
                    "unknown"
                } else {
                    offer.currency.as_str()
                }
            ));
            continue;
        };
        let converted = converted_owned.as_ref().unwrap_or(offer);
        if is_standard_to(converted, intended) {
            continue;
        }
        let Some(hit) = detect_hidden_city(converted, intended) else {
            rejected.push(format!(
                "{}: does not pass through intended destination",
                offer.id
            ));
            continue;
        };
        let saving = hidden_city_savings(local_usd.price, converted.price);
        if !meaningful_saving(saving, Some(min_saving)) {
            rejected.push(format!("{}: saving below floor", offer.id));
            continue;
        }
        let mut converted = converted.clone();
        converted.kind = "hidden-city".into();
        let ticketed = ticketed_destination(&converted);
        let match_id = format!("hc-{}-{}", converted.id, hit.exit_airport);
        let origin_country = origin_place.map(|p| p.country.clone()).unwrap_or_default();
        matches.push(attach_risk(
            &match_id,
            &ticketed,
            &ticketed,
            &local_usd,
            &converted,
            &hit.exit_airport,
            &origin_country,
            "",
            hit.exit_segment_index,
        ));
        if let Some(first) = converted.segments.first() {
            record_offer(&first.origin, &hit.exit_airport, &ticketed, converted.price);
        }
    }
    (matches, rejected)
}

pub trait AsCodes {
    fn as_codes(&self) -> HashSet<String>;
}

impl AsCodes for &str {
    fn as_codes(&self) -> HashSet<String> {
        HashSet::from([(*self).to_uppercase()])
    }
}

impl AsCodes for String {
    fn as_codes(&self) -> HashSet<String> {
        HashSet::from([self.to_uppercase()])
    }
}

impl AsCodes for HashSet<String> {
    fn as_codes(&self) -> HashSet<String> {
        self.iter().map(|s| s.to_uppercase()).collect()
    }
}

impl<'a> AsCodes for HashSet<&'a str> {
    fn as_codes(&self) -> HashSet<String> {
        self.iter().map(|s| s.to_uppercase()).collect()
    }
}

pub fn _via_b(offer: &Offer, origin: impl AsCodes, dest_b: impl AsCodes, dest_c: &str) -> bool {
    if offer.segments.is_empty() {
        return false;
    }
    let origins = origin.as_codes();
    let dests = dest_b.as_codes();
    if !origins.contains(&offer.segments[0].origin.to_uppercase()) {
        return false;
    }
    let ticketed = ticketed_destination(offer);
    let dest_c_u = dest_c.to_uppercase();
    if ticketed != dest_c_u || dests.contains(&dest_c_u) {
        return false;
    }
    detect_hidden_city(offer, &dests).is_some()
}

pub fn _is_sandbox(offer: &Offer) -> bool {
    is_sandbox_offer(offer)
}

fn is_self_transfer(offer: &Offer) -> bool {
    offer.kind == "self-transfer" || offer.source == "self-transfer"
}

pub fn _is_self_transfer(offer: &Offer) -> bool {
    is_self_transfer(offer)
}

pub fn _drop_sandbox(offers: &[Offer]) -> Vec<Offer> {
    offers.iter().filter(|o| !_is_sandbox(o)).cloned().collect()
}

fn take_drop_sandbox(offers: Vec<Offer>) -> Vec<Offer> {
    offers.into_iter().filter(|o| !_is_sandbox(o)).collect()
}

pub fn _on_route(offer: &Offer, origins: &HashSet<String>, dests: &HashSet<String>) -> bool {
    if offer.segments.is_empty() {
        return false;
    }
    let origin = offer.segments[0].origin.to_uppercase();
    let ticketed = ticketed_destination(offer);
    (origins.contains(&origin) || origins.iter().any(|o| o.eq_ignore_ascii_case(&origin)))
        && (dests.contains(&ticketed) || dests.iter().any(|d| d.eq_ignore_ascii_case(&ticketed)))
}

pub fn _keep_on_route(
    offers: &[Offer],
    origins: &HashSet<String>,
    dests: &HashSet<String>,
) -> Vec<Offer> {
    offers
        .iter()
        .filter(|o| _on_route(o, origins, dests))
        .cloned()
        .collect()
}

fn codeshare_key(offer: &Offer) -> Vec<(String, String, String, String)> {
    offer
        .segments
        .iter()
        .map(|s| {
            (
                s.origin.to_uppercase(),
                s.dest.to_uppercase(),
                s.dep.chars().take(16).collect(),
                s.arr.chars().take(16).collect(),
            )
        })
        .collect()
}

pub fn _dedupe_itineraries(offers: &[Offer]) -> Vec<Offer> {
    take_dedupe_itineraries(offers.to_vec())
}

fn take_dedupe_itineraries(mut ordered: Vec<Offer>) -> Vec<Offer> {
    ordered.sort_by(|a, b| {
        let pa = a.price.unwrap_or(1e12);
        let pb = b.price.unwrap_or(1e12);
        pa.partial_cmp(&pb)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| layover_minutes(a).cmp(&layover_minutes(b)))
            .then_with(|| a.duration_min.cmp(&b.duration_min))
    });
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for offer in ordered {
        if seen.insert(codeshare_key(&offer)) {
            out.push(offer);
        }
    }
    out
}

pub fn _prefer_real_carriers(offers: Vec<Offer>) -> Vec<Offer> {
    take_prefer_real_carriers(offers)
}

fn take_prefer_real_carriers(mut offers: Vec<Offer>) -> Vec<Offer> {
    if offers.iter().any(|o| !is_test_carrier(&o.carrier)) {
        offers.retain(|o| !is_test_carrier(&o.carrier));
    }
    offers
}

pub fn _dedupe(offers: &[Offer]) -> Vec<Offer> {
    take_dedupe(offers.to_vec())
}

fn take_dedupe(mut ordered: Vec<Offer>) -> Vec<Offer> {
    ordered.sort_by(|a, b| {
        let pa = a.price.unwrap_or(1e12);
        let pb = b.price.unwrap_or(1e12);
        pa.partial_cmp(&pb).unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for o in ordered {
        let segs: Vec<(String, String, String)> = o
            .segments
            .iter()
            .map(|s| (s.origin.clone(), s.dest.clone(), s.flight_number.clone()))
            .collect();
        let key = (
            o.source.clone(),
            o.first_flight.clone(),
            o.price.map(|p| p.to_bits()),
            segs,
        );
        if !seen.insert(key) {
            continue;
        }
        out.push(o);
    }
    out
}

fn parse_leg_time(value: &str) -> Option<DateTime<Utc>> {
    if value.is_empty() {
        return None;
    }
    let raw = value.replace('Z', "+00:00");
    if let Ok(dt) = DateTime::parse_from_rfc3339(&raw) {
        return Some(dt.with_timezone(&Utc));
    }
    for fmt in [
        "%Y-%m-%dT%H:%M:%S%.f%:z",
        "%Y-%m-%dT%H:%M:%S%:z",
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M",
    ] {
        if let Ok(n) = NaiveDateTime::parse_from_str(&raw, fmt) {
            return Some(n.and_utc());
        }
        if let Ok(dt) = DateTime::parse_from_str(&raw, fmt) {
            return Some(dt.with_timezone(&Utc));
        }
    }
    None
}

pub fn layover_minutes(offer: &Offer) -> i32 {
    let mut segs = offer.segments.as_slice();
    if let Some(end) = offer.outbound_end {
        if !offer.segments.is_empty() {
            let idx = (end.max(0) as usize).min(offer.segments.len() - 1);
            segs = &offer.segments[..=idx];
        }
    }
    if segs.len() < 2 {
        return 0;
    }
    let mut total = 0;
    let mut parsed = 0;
    for pair in segs.windows(2) {
        let arr = parse_leg_time(&pair[0].arr);
        let dep = parse_leg_time(&pair[1].dep);
        let (Some(arr), Some(dep)) = (arr, dep) else {
            continue;
        };
        let gap = ((dep - arr).num_seconds() / 60) as i32;
        if gap > 0 {
            total += gap;
            parsed += 1;
        }
    }
    if parsed > 0 {
        return total;
    }
    let air: i32 = segs.iter().map(|s| s.duration_min).sum();
    let inferred = offer.duration_min - air;
    if inferred > 0 {
        inferred
    } else {
        24 * 60
    }
}

pub fn _priced_in_currency(offers: &[Offer], preferred: Option<&str>) -> Vec<Offer> {
    let priced: Vec<Offer> = offers_in_usd(offers)
        .into_iter()
        .filter(|o| o.price.is_some())
        .collect();
    if priced.is_empty() {
        return vec![];
    }
    let want = preferred.unwrap_or("USD").to_uppercase();
    let matched: Vec<Offer> = priced
        .iter()
        .filter(|o| o.currency.to_uppercase() == want)
        .cloned()
        .collect();
    if !matched.is_empty() {
        return matched;
    }
    let mut counts: HashMap<String, i32> = HashMap::new();
    for offer in &priced {
        *counts.entry(offer.currency.clone()).or_default() += 1;
    }
    let modal = counts
        .iter()
        .max_by_key(|(_, n)| *n)
        .map(|(c, _)| c.clone())
        .unwrap_or_default();
    priced.into_iter().filter(|o| o.currency == modal).collect()
}

pub fn _best_local_honest(offers: &[Offer], preferred: Option<&str>) -> Option<Offer> {
    let priced = _priced_in_currency(offers, preferred);
    priced.into_iter().min_by(|a, b| {
        let pa = a.price.unwrap_or(1e12);
        let pb = b.price.unwrap_or(1e12);
        pa.partial_cmp(&pb)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| layover_minutes(a).cmp(&layover_minutes(b)))
            .then_with(|| a.duration_min.cmp(&b.duration_min))
    })
}

fn as_pick(offer: Offer, reason: &str) -> HonestPick {
    let layover_min = layover_minutes(&offer);
    HonestPick {
        kind: if offer.stops == 0 {
            "nonstop".into()
        } else {
            "connecting".into()
        },
        reason: reason.into(),
        layover_min,
        offer,
    }
}

fn pick_price_key(a: &Offer, b: &Offer) -> std::cmp::Ordering {
    let pa = a.price.unwrap_or(1e12);
    let pb = b.price.unwrap_or(1e12);
    pa.partial_cmp(&pb)
        .unwrap_or(std::cmp::Ordering::Equal)
        .then_with(|| layover_minutes(a).cmp(&layover_minutes(b)))
        .then_with(|| a.duration_min.cmp(&b.duration_min))
}

pub fn pick_honest(
    nonstop: &[Offer],
    connecting: &[Offer],
    preferred: Option<&str>,
) -> Option<HonestPick> {
    let want = preferred.unwrap_or("USD");
    let mixed = nonstop
        .iter()
        .chain(connecting.iter())
        .any(|o| o.price.is_some() && !o.currency.eq_ignore_ascii_case(want));
    let offer = if mixed {
        let mut all = Vec::new();
        all.extend(nonstop.iter().cloned());
        all.extend(connecting.iter().cloned());
        _priced_in_currency(&all, preferred)
            .into_iter()
            .min_by(pick_price_key)
    } else {
        nonstop
            .iter()
            .chain(connecting.iter())
            .filter(|o| o.price.is_some())
            .min_by(|a, b| pick_price_key(a, b))
            .cloned()
    }?;
    if offer.stops == 0 {
        Some(as_pick(offer, "Best — cheapest nonstop"))
    } else {
        Some(as_pick(offer, "Cheapest — connecting"))
    }
}

fn honest_rank_key(offer: &Offer) -> (i32, u64, i32, i32) {
    (
        offer.stops,
        offer.price.unwrap_or(1e12).to_bits(),
        layover_minutes(offer),
        offer.duration_min,
    )
}

fn _honest_rank(a: &Offer, b: &Offer) -> std::cmp::Ordering {
    honest_rank_key(a).cmp(&honest_rank_key(b))
}

pub fn pick_best(
    nonstop: &[Offer],
    connecting: &[Offer],
    cheapest: Option<&HonestPick>,
) -> Option<HonestPick> {
    let cheap_id = cheapest.map(|c| c.offer.id.as_str());
    let usd = |o: &Offer| o.price.is_some() && o.currency.eq_ignore_ascii_case("USD");
    if !nonstop.iter().chain(connecting.iter()).any(usd) {
        let mut all = Vec::new();
        all.extend(nonstop.iter().cloned());
        all.extend(connecting.iter().cloned());
        return pick_best_from_owned(_priced_in_currency(&all, None), cheap_id);
    }
    if let Some(best) = nonstop
        .iter()
        .filter(|o| usd(o))
        .min_by(|a, b| _honest_rank(a, b))
    {
        if Some(best.id.as_str()) == cheap_id {
            return None;
        }
        return Some(as_pick(best.clone(), "Best — cheapest nonstop"));
    }
    let priced: Vec<&Offer> = connecting.iter().filter(|o| usd(o)).collect();
    if priced.is_empty() {
        return None;
    }
    let min_stops = priced.iter().map(|o| o.stops).min()?;
    let best = priced
        .into_iter()
        .filter(|o| o.stops == min_stops)
        .min_by(|a, b| _honest_rank(a, b))?;
    if Some(best.id.as_str()) == cheap_id {
        return None;
    }
    let label = if min_stops == 1 {
        "one stop".to_string()
    } else {
        format!("{min_stops} stops")
    };
    Some(as_pick(best.clone(), &format!("Best — cheapest {label}")))
}

fn pick_best_from_owned(priced: Vec<Offer>, cheap_id: Option<&str>) -> Option<HonestPick> {
    if priced.is_empty() {
        return None;
    }
    if let Some(best) = priced
        .iter()
        .filter(|o| o.stops == 0)
        .min_by(|a, b| _honest_rank(a, b))
    {
        if Some(best.id.as_str()) == cheap_id {
            return None;
        }
        return Some(as_pick(best.clone(), "Best — cheapest nonstop"));
    }
    let min_stops = priced.iter().map(|o| o.stops).min()?;
    let best = priced
        .iter()
        .filter(|o| o.stops == min_stops)
        .min_by(|a, b| _honest_rank(a, b))?;
    if Some(best.id.as_str()) == cheap_id {
        return None;
    }
    let label = if min_stops == 1 {
        "one stop".to_string()
    } else {
        format!("{min_stops} stops")
    };
    Some(as_pick(best.clone(), &format!("Best — cheapest {label}")))
}

pub fn _clean_hidden_matches(matches: Vec<HiddenCityMatch>) -> Vec<HiddenCityMatch> {
    clean_hidden_matches(matches)
}

fn clean_hidden_matches(matches: Vec<HiddenCityMatch>) -> Vec<HiddenCityMatch> {
    if matches.is_empty() {
        return vec![];
    }
    let throughs: Vec<Offer> = matches.iter().map(|m| m.through_offer.clone()).collect();
    let keep_ids: HashSet<String> = _prefer_real_carriers(throughs)
        .into_iter()
        .map(|o| o.id)
        .collect();
    let mut cleaned: Vec<HiddenCityMatch> = matches
        .into_iter()
        .filter(|m| keep_ids.contains(&m.through_offer.id))
        .collect();
    cleaned.sort_by(|a, b| {
        let pa = a.through_offer.price.unwrap_or(1e12);
        let pb = b.through_offer.price.unwrap_or(1e12);
        pa.partial_cmp(&pb)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                b.gross_saving
                    .partial_cmp(&a.gross_saving)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for match_ in cleaned {
        if seen.insert(codeshare_key(&match_.through_offer)) {
            out.push(match_);
        }
    }
    out
}

pub fn _hidden_if_cheaper(
    matches: &[HiddenCityMatch],
    honest: Option<&HonestPick>,
) -> Option<HiddenCityMatch> {
    let honest = honest?;
    let honest_price = honest.offer.price?;
    let _ = honest_price;
    let ceiling_offer = offer_in_usd(&honest.offer)?;
    let ceiling = ceiling_offer.price?;
    for match_ in matches {
        let Some(through) = offer_in_usd(&match_.through_offer) else {
            continue;
        };
        let Some(price) = through.price else {
            continue;
        };
        if price < ceiling {
            return Some(match_.clone());
        }
    }
    None
}

pub fn _gaps(
    settings: &Settings,
    duffel: Option<&DuffelProvider>,
    box_: Option<&AeroDataBoxProvider>,
    priced: &[Offer],
    hidden: &[HiddenCityMatch],
    traffic: &LiveTraffic,
    _board: &[BoardFlight],
    mock: Option<&MockProvider>,
    round_trip: bool,
) -> Vec<String> {
    gaps(
        settings, duffel, box_, priced, hidden, traffic, _board, mock, round_trip, false, 0, true,
    )
}

fn gaps(
    settings: &Settings,
    duffel: Option<&DuffelProvider>,
    box_: Option<&AeroDataBoxProvider>,
    priced: &[Offer],
    hidden: &[HiddenCityMatch],
    traffic: &LiveTraffic,
    _board: &[BoardFlight],
    mock: Option<&MockProvider>,
    round_trip: bool,
    reused_honest: bool,
    reused_from_index: i32,
    fx_live: bool,
) -> Vec<String> {
    let mut gaps = Vec::new();
    if settings.duffel_sandbox() {
        gaps.push(
            "Your Duffel key is sandbox (duffel_test_…). Offers with live_mode=False are dropped. \
A duffel_live_… token is required for real fares."
                .into(),
        );
    } else if duffel.is_none() && mock.is_none() {
        gaps.push(
            "No Duffel token and mock is off. Priced offers stay empty until DUFFEL_TOKEN or MOCK_ENABLED."
                .into(),
        );
    }
    let no_keys = duffel.is_none() && mock.is_none();
    if priced.is_empty() && !no_keys {
        let duffel_failed = LEDGER
            .try_with(|l| l.calls().iter().any(|c| c.provider == "duffel" && !c.ok))
            .unwrap_or(false);
        if duffel_failed {
            gaps.push(
                "Duffel shop failed. Check DUFFEL_TOKEN and Duffel-Version; this is not an empty market."
                    .into(),
            );
        } else if settings.duffel_sandbox() {
            gaps.push(
                "Sandbox shop returned no live fares (live_mode=false is dropped). This is not a live empty market."
                    .into(),
            );
        } else {
            gaps.push(
                "Duffel returned no priced offers for this city-pair/date (coverage or date)."
                    .into(),
            );
        }
    }
    if round_trip {
        gaps.push(
            "Hidden-city is one-way only. This search compares honest round-trip tickets.".into(),
        );
    } else if hidden.is_empty() {
        gaps.push(
            "No priced inversion P(A,B,C)<P(A,B) in the current shop. Connection hints are not savings."
                .into(),
        );
    }
    if reused_honest {
        gaps.push(
            "Honest A→B reused a live Duffel fare less than 60 seconds old. Hidden-city candidates still come from the index."
                .into(),
        );
    } else if reused_from_index > 0 {
        gaps.push(format!(
            "Reused {reused_from_index} priced offer(s) from the index. Those through tickets were not shopped again this request."
        ));
    }
    if !fx_live {
        gaps.push(
            "FX unverified: Frankfurter has not returned rates yet. Non-USD offers were omitted from ranking (baked-in rates are not used).".into(),
        );
    }
    if traffic.aircraft.is_empty() {
        gaps.push(traffic.note.clone());
    }
    if box_.is_none() {
        gaps.push(
            "No RAPIDAPI_KEY: AeroDataBox FIDS (scheduled/estimated/actual board) is off. Use FR24/FlightAware links for commercial boards."
                .into(),
        );
    }
    gaps
}
