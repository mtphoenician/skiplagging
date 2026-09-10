use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use chrono::Local;
use serde_json::{json, Value};
use sqlx::PgPool;
use tokio::sync::Semaphore;

use crate::config::Settings;
use crate::db::repo;
use crate::engines::hidden::{meaningful_saving, ticketed_destination};
use crate::engines::risk::attach_risk;
use crate::engines::shop::{_drop_sandbox, _priced_in_currency, _shop_providers, layover_minutes};
use crate::fx::{offer_in_usd, refresh_rates};
use crate::models::{Offer, ShopRequest};
use crate::providers::bookers::booker_links;
use crate::providers::duffel::DuffelProvider;
use crate::providers::sandbox::is_live_fare;

pub fn _scan_dates() -> Vec<String> {
    let today = Local::now().date_naive();
    [21i64, 35, 49]
        .into_iter()
        .map(|offset| (today + chrono::Duration::days(offset)).format("%Y-%m-%d").to_string())
        .collect()
}

pub fn _via_stops(offer: &Offer, origin: &str) -> (String, Vec<(i32, String)>) {
    let ticketed = ticketed_destination(offer);
    let mut idx = offer
        .outbound_end
        .unwrap_or(offer.segments.len().saturating_sub(1) as i32);
    if !offer.segments.is_empty() {
        idx = idx.max(0).min((offer.segments.len() - 1) as i32);
    }
    let origin_u = origin.to_uppercase();
    let mut vias = Vec::new();
    let end = if offer.segments.is_empty() {
        0
    } else {
        idx as usize
    };
    for (i, seg) in offer.segments.iter().take(end).enumerate() {
        let dest_b = seg.dest.to_uppercase();
        if dest_b != origin_u && dest_b != ticketed {
            vias.push((i as i32, dest_b));
        }
    }
    (ticketed, vias)
}

pub fn _cheapest_to(offers: &[Offer], dest: &str) -> Option<Offer> {
    let dest_u = dest.to_uppercase();
    let priced: Vec<Offer> = offers
        .iter()
        .filter(|o| {
            o.price.is_some() && !o.segments.is_empty() && ticketed_destination(o) == dest_u
        })
        .cloned()
        .collect();
    let priced = _priced_in_currency(&priced, None);
    priced.into_iter().min_by(|a, b| {
        let pa = a.price.unwrap_or(1e12);
        let pb = b.price.unwrap_or(1e12);
        pa.partial_cmp(&pb)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| layover_minutes(a).cmp(&layover_minutes(b)))
            .then_with(|| a.duration_min.cmp(&b.duration_min))
    })
}

async fn shop(
    req: &ShopRequest,
    duffel: Option<&DuffelProvider>,
    sem: &Semaphore,
) -> Vec<Offer> {
    let _g = match sem.acquire().await {
        Ok(g) => g,
        Err(_) => return vec![],
    };
    let offers = _shop_providers(req, duffel, false, None, "discover").await;
    tokio::time::sleep(Duration::from_millis(350)).await;
    _priced_in_currency(&_drop_sandbox(&offers), None)
}

async fn dests_for(
    pool: &PgPool,
    origin: &str,
    extra: &[String],
    limit: i64,
    skip: &HashSet<String>,
) -> anyhow::Result<Vec<String>> {
    let spokes: Vec<String> = repo::destinations_from(pool, origin, limit)
        .await?
        .into_iter()
        .map(|(code, _, _)| code)
        .collect();
    let mut out = Vec::new();
    let mut seen = skip.clone();
    seen.insert(origin.to_uppercase());
    for code in extra.iter().chain(spokes.iter()) {
        let c = code.to_uppercase();
        if c.len() != 3 || seen.contains(&c) {
            continue;
        }
        seen.insert(c.clone());
        out.push(c);
    }
    Ok(out)
}

pub async fn discover_hidden_deals(
    pool: &PgPool,
    settings: &Settings,
    client: &reqwest::Client,
    limit: i64,
    dests_per_origin: i64,
    day: Option<String>,
    dates: Option<Vec<String>>,
) -> anyhow::Result<Value> {
    let days = if let Some(day) = day.filter(|d| !d.is_empty()) {
        vec![day]
    } else {
        dates.unwrap_or_else(_scan_dates)
    };
    let skip = settings.skip_dests();
    refresh_rates(client).await;
    if !settings.duffel_enabled() {
        return Ok(json!({
            "scanned": 0,
            "found_this_run": 0,
            "stored": 0,
            "errors": ["No Duffel token"],
            "dates": days,
        }));
    }
    if !settings.publish_deals() {
        return Ok(json!({
            "scanned": 0,
            "found_this_run": 0,
            "stored": 0,
            "errors": ["Need a Duffel live token (duffel_live_…) to publish deals. Test inventory is not saved."],
            "dates": days,
        }));
    }

    let mut found = 0i64;
    let mut scanned = 0i64;
    let mut shops = 0i64;
    let mut connecting_seen = 0i64;
    let mut compared = 0i64;
    let mut empty_shops = 0i64;
    let mut closest: Vec<(f64, String)> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    let mut extras_by_origin: HashMap<String, Vec<String>> = HashMap::new();

    let hubs = repo::busiest_airports(pool, limit, &skip).await?;
    for o in &hubs {
        extras_by_origin.insert(
            o.clone(),
            repo::destinations_from(pool, o, 8)
                .await?
                .into_iter()
                .map(|(d, _, _)| d)
                .filter(|d| !skip.contains(&d.to_uppercase()))
                .collect(),
        );
    }

    let sem_n = 2.max(settings.max_concurrency.min(3));
    let sem = Arc::new(Semaphore::new(sem_n.max(1)));
    let duffel = if settings.duffel_enabled() {
        Some(DuffelProvider::new(settings, client.clone()))
    } else {
        None
    };

    for date in &days {
        println!("\n===== {date} =====");
        for origin in &hubs {
            let extra = extras_by_origin.get(origin).cloned().unwrap_or_default();
            let dests = dests_for(pool, origin, &extra, dests_per_origin, &skip).await?;
            if dests.is_empty() {
                continue;
            }
            scanned += 1;
            println!("{origin} {date}: shopping {} destinations", dests.len());
            let mut by_dest: HashMap<String, Vec<Offer>> = HashMap::new();
            let mut all_offers: Vec<Offer> = Vec::new();
            for dest in &dests {
                let req = ShopRequest {
                    origin: origin.clone(),
                    dest: dest.clone(),
                    date: date.clone(),
                    adults: 1,
                    cabin: "ECONOMY".into(),
                    currency: "USD".into(),
                    nonstop: false,
                    max_offers: 25,
                    return_date: None,
                };
                let offers = shop(&req, duffel.as_ref(), sem.as_ref()).await;
                shops += 1;
                if offers.is_empty() {
                    empty_shops += 1;
                    continue;
                }
                by_dest.insert(dest.clone(), offers.clone());
                all_offers.extend(offers);
            }

            let mut missing_b: HashSet<String> = HashSet::new();
            for offer in &all_offers {
                if offer.price.is_none() || offer.segments.len() < 2 {
                    continue;
                }
                connecting_seen += 1;
                let (_ticketed, vias) = _via_stops(offer, origin);
                for (_i, dest_b) in vias {
                    if skip.contains(&dest_b) {
                        continue;
                    }
                    if !by_dest.contains_key(&dest_b) {
                        missing_b.insert(dest_b);
                    }
                }
            }

            let mut missing: Vec<String> = missing_b.into_iter().collect();
            missing.sort();
            for dest_b in missing.into_iter().take(20) {
                let req = ShopRequest {
                    origin: origin.clone(),
                    dest: dest_b.clone(),
                    date: date.clone(),
                    adults: 1,
                    cabin: "ECONOMY".into(),
                    currency: "USD".into(),
                    nonstop: false,
                    max_offers: 25,
                    return_date: None,
                };
                let offers = shop(&req, duffel.as_ref(), sem.as_ref()).await;
                shops += 1;
                if !offers.is_empty() {
                    by_dest.insert(dest_b, offers.clone());
                    all_offers.extend(offers);
                } else {
                    empty_shops += 1;
                }
            }

            let cheapest_b: HashMap<String, Offer> = by_dest
                .iter()
                .filter_map(|(d, offers)| _cheapest_to(offers, d).map(|o| (d.clone(), o)))
                .collect();
            let mut matches = Vec::new();
            let mut seen: HashSet<(String, String, String, String)> = HashSet::new();
            let Some(o_ap) = repo::get_airport(pool, origin, false).await? else {
                errors.push(format!("{origin}: unknown"));
                continue;
            };
            for offer in &all_offers {
                if offer.price.is_none() || offer.segments.len() < 2 {
                    continue;
                }
                let (dest_c, vias) = _via_stops(offer, origin);
                if dest_c.is_empty() {
                    continue;
                }
                for (exit_i, dest_b) in vias {
                    let Some(local) = cheapest_b.get(&dest_b) else {
                        continue;
                    };
                    if local.price.is_none() {
                        continue;
                    }
                    if !is_live_fare(offer) || !is_live_fare(local) {
                        continue;
                    }
                    let Some(through_usd) = offer_in_usd(offer) else {
                        continue;
                    };
                    let Some(local_usd) = offer_in_usd(local) else {
                        continue;
                    };
                    let (Some(tp), Some(lp)) = (through_usd.price, local_usd.price) else {
                        continue;
                    };
                    compared += 1;
                    if tp >= lp {
                        let gap = if lp != 0.0 { tp / lp } else { 99.0 };
                        closest.push((
                            gap,
                            format!(
                                "{origin}->{dest_b}->{dest_c} through={tp} local={lp} USD"
                            ),
                        ));
                        continue;
                    }
                    let saving = ((lp - tp) * 100.0).round() / 100.0;
                    if !meaningful_saving(Some(saving), Some(settings.min_hidden_saving)) {
                        continue;
                    }
                    let key = (
                        origin.clone(),
                        dest_b.clone(),
                        dest_c.clone(),
                        date.clone(),
                    );
                    if !seen.insert(key) {
                        continue;
                    }
                    let Some(b_ap) = repo::get_airport(pool, &dest_b, false).await? else {
                        continue;
                    };
                    let Some(c_ap) = repo::get_airport(pool, &dest_c, false).await? else {
                        continue;
                    };
                    let mut match_ = attach_risk(
                        &format!("disc-{origin}-{dest_b}-{dest_c}-{}", offer.id),
                        &dest_c,
                        &format!("{} ({dest_c})", c_ap.city),
                        &local_usd,
                        &through_usd,
                        &dest_b,
                        &o_ap.country,
                        &c_ap.country,
                        exit_i,
                    );
                    match_.bookers = booker_links(
                        origin,
                        &dest_c,
                        date,
                        1,
                        &[],
                        "USD",
                        "ECONOMY",
                        None,
                    );
                    repo::persist_hidden_deals(
                        pool,
                        &[match_.clone()],
                        origin,
                        &o_ap.city,
                        &dest_b,
                        &b_ap.city,
                        date,
                    )
                    .await?;
                    println!(
                        "  HIT {origin}->{dest_b}->{dest_c} through={tp} USD local={lp} save={saving}"
                    );
                    matches.push(match_);
                }
            }
            found += matches.len() as i64;
            let connecting_n = all_offers.iter().filter(|o| o.segments.len() > 1).count();
            println!(
                "{origin} {date}: {} inversion(s), {} offers, {connecting_n} connecting",
                matches.len(),
                all_offers.len()
            );
        }
    }

    let deals = repo::list_hidden_deals(pool, 120, "", "").await?;
    closest.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let closest_misses: Vec<String> = closest.into_iter().take(8).map(|(_, row)| row).collect();
    let summary = json!({
        "scanned": scanned,
        "shops": shops,
        "empty_shops": empty_shops,
        "connecting_seen": connecting_seen,
        "compared_cheaper": compared,
        "found_this_run": found,
        "stored": deals.len(),
        "errors": errors,
        "dates": days,
        "closest_misses": closest_misses,
    });
    println!("{summary}");
    Ok(summary)
}
