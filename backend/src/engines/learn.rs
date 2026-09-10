use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet};

use crate::engines::hidden::{detect_hidden_city, itinerary_fingerprint, ticketed_destination};
use crate::fx::{offers_in_usd, to_usd};
use crate::models::Offer;

pub const MAX_SAVINGS_KEPT: usize = 200;

#[derive(Debug, Clone)]
pub struct FareObservation {
    pub offer_uid: String,
    pub fingerprint: String,
    pub origin: String,
    pub ticketed: String,
    pub connections: Vec<String>,
    pub stops: i32,
    pub carrier: String,
    pub provider: String,
    pub price: f64,
    pub currency: String,
    pub date: String,
    pub adults: i32,
    pub cabin: String,
    pub fare_brand: Option<String>,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct EdgeObservation {
    pub origin: String,
    pub dest: String,
    pub carrier: String,
    pub flight_number: String,
    pub count: i32,
    pub travel_dates: HashSet<String>,
}

#[derive(Debug, Clone)]
pub struct StatDelta {
    pub origin: String,
    pub intended: String,
    pub ticketed: String,
    pub observations: i32,
    pub successful_connections: i32,
    pub cheaper_than_direct_count: i32,
    pub savings: Vec<f64>,
    pub saving_pcts: Vec<f64>,
    pub currency: String,
    pub best_through_price: Option<f64>,
    pub success: bool,
    pub cheaper: bool,
}

#[derive(Debug, Clone)]
pub struct LearnBatch {
    pub observed_at: DateTime<Utc>,
    pub fares: Vec<FareObservation>,
    pub edges: HashMap<(String, String, String, String), EdgeObservation>,
    pub stats: HashMap<(String, String, String), StatDelta>,
}

impl LearnBatch {
    pub fn stat(&mut self, origin: &str, intended: &str, ticketed: &str) -> &mut StatDelta {
        let key = (
            origin.to_uppercase(),
            intended.to_uppercase(),
            ticketed.to_uppercase(),
        );
        self.stats.entry(key.clone()).or_insert_with(|| StatDelta {
            origin: key.0,
            intended: key.1,
            ticketed: key.2,
            observations: 0,
            successful_connections: 0,
            cheaper_than_direct_count: 0,
            savings: vec![],
            saving_pcts: vec![],
            currency: String::new(),
            best_through_price: None,
            success: false,
            cheaper: false,
        })
    }
}

pub fn build_batch(
    offers: &[Offer],
    origins: &HashSet<String>,
    intended: &HashSet<String>,
    honest_price: Option<f64>,
    honest_currency: Option<&str>,
    probed: &[String],
    date: &str,
    adults: i32,
    cabin: &str,
    probe_origin: Option<&str>,
    probe_intended: Option<&str>,
    now: Option<DateTime<Utc>>,
) -> LearnBatch {
    let now = now.unwrap_or_else(Utc::now);
    let mut batch = LearnBatch {
        observed_at: now,
        fares: vec![],
        edges: HashMap::new(),
        stats: HashMap::new(),
    };
    let origin_set: HashSet<String> = origins
        .iter()
        .map(|o| o.to_uppercase().chars().take(3).collect())
        .collect();
    let intended_set: HashSet<String> = intended.iter().map(|b| b.to_uppercase()).collect();
    let probe_a = probe_origin
        .map(|s| s.to_uppercase().chars().take(3).collect::<String>())
        .or_else(|| origin_set.iter().cloned().min())
        .unwrap_or_default();
    let probe_b = probe_intended
        .map(|s| s.to_uppercase().chars().take(3).collect::<String>())
        .or_else(|| intended_set.iter().cloned().min())
        .unwrap_or_default();
    let probed_u: HashSet<String> = probed.iter().map(|p| p.to_uppercase()).collect();

    let mut honest_price = honest_price;
    let mut honest_currency = honest_currency.map(|s| s.to_string());
    if let Some(hp) = honest_price {
        honest_price = to_usd(Some(hp), honest_currency.as_deref());
        honest_currency = if honest_price.is_some() {
            Some("USD".into())
        } else {
            None
        };
    }

    let priced: Vec<Offer> = offers_in_usd(offers)
        .into_iter()
        .filter(|o| o.price.is_some() && !o.segments.is_empty())
        .collect();
    for offer in &priced {
        let first = &offer.segments[0];
        if !origin_set.contains(&first.origin.to_uppercase()) {
            continue;
        }
        let mut idx = offer
            .outbound_end
            .unwrap_or((offer.segments.len() - 1) as i32);
        idx = idx.max(0).min((offer.segments.len() - 1) as i32);
        let ticketed = ticketed_destination(offer);
        let outbound = &offer.segments[..=idx as usize];
        batch.fares.push(FareObservation {
            offer_uid: offer.id.clone(),
            fingerprint: itinerary_fingerprint(offer),
            origin: first.origin.to_uppercase(),
            ticketed,
            connections: offer.segments[..idx as usize]
                .iter()
                .map(|s| s.dest.to_uppercase())
                .collect(),
            stops: offer.stops,
            carrier: offer.carrier.to_uppercase(),
            provider: offer.source.clone(),
            price: offer.price.unwrap_or(0.0),
            currency: offer.currency.clone(),
            date: date.to_string(),
            adults,
            cabin: cabin.to_string(),
            fare_brand: if offer.fare_basis.is_empty() {
                None
            } else {
                Some(offer.fare_basis.clone())
            },
            expires_at: offer.expires_at.clone(),
        });
        for seg in outbound {
            let key = (
                seg.origin.to_uppercase(),
                seg.dest.to_uppercase(),
                seg.carrier.to_uppercase(),
                seg.flight_number.clone(),
            );
            let edge = batch.edges.entry(key.clone()).or_insert_with(|| EdgeObservation {
                origin: key.0,
                dest: key.1,
                carrier: key.2,
                flight_number: key.3,
                count: 0,
                travel_dates: HashSet::new(),
            });
            edge.count += 1;
            edge.travel_dates.insert(date.to_string());
        }
    }

    let mut by_ticketed: HashMap<String, Vec<Offer>> = HashMap::new();
    for offer in &priced {
        if !origin_set.contains(&offer.segments[0].origin.to_uppercase()) {
            continue;
        }
        let c = ticketed_destination(offer);
        if !c.is_empty() && !intended_set.contains(&c) {
            by_ticketed.entry(c).or_default().push(offer.clone());
        }
    }

    let mut probe_rows: HashSet<(String, String, String)> = HashSet::new();
    if !probe_a.is_empty() && !probe_b.is_empty() {
        for c in &probed_u {
            if intended_set.contains(c) || origin_set.contains(c) {
                continue;
            }
            batch.stat(&probe_a, &probe_b, c).observations += 1;
            probe_rows.insert((probe_a.clone(), probe_b.clone(), c.clone()));
        }
    }

    for (c, group) in by_ticketed {
        let mut via_b: HashMap<(String, String), Vec<Offer>> = HashMap::new();
        let mut other_connections: HashSet<(String, String)> = HashSet::new();
        for offer in &group {
            let a = offer.segments[0].origin.to_uppercase();
            if let Some(hit) = detect_hidden_city(offer, &intended_set) {
                via_b
                    .entry((a.clone(), hit.exit_airport))
                    .or_default()
                    .push(offer.clone());
            }
            let mut idx = offer
                .outbound_end
                .unwrap_or((offer.segments.len() - 1) as i32);
            idx = idx.max(0).min((offer.segments.len() - 1) as i32);
            for s in &offer.segments[..idx as usize] {
                let x = s.dest.to_uppercase();
                if !intended_set.contains(&x) && x != a {
                    other_connections.insert((a.clone(), x));
                }
            }
        }
        for ((a, b), hits) in via_b {
            let in_probe = probe_rows.contains(&(a.clone(), b.clone(), c.clone()));
            let row = batch.stat(&a, &b, &c);
            if !in_probe {
                row.observations += 1;
            }
            row.successful_connections += 1;
            row.success = true;
            let cheapest = hits
                .iter()
                .min_by(|a, b| {
                    a.price
                        .unwrap_or(1e12)
                        .partial_cmp(&b.price.unwrap_or(1e12))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .cloned();
            if let Some(cheapest) = cheapest {
                if let Some(p) = cheapest.price {
                    if row.best_through_price.map(|b| p < b).unwrap_or(true) {
                        row.best_through_price = Some(p);
                    }
                }
                if let (Some(hp), Some(hc)) = (honest_price, honest_currency.as_deref()) {
                    if cheapest.price.is_some()
                        && cheapest.currency == hc
                        && cheapest.price.unwrap() < hp
                    {
                        let saving = ((hp - cheapest.price.unwrap()) * 100.0).round() / 100.0;
                        row.cheaper_than_direct_count += 1;
                        row.cheaper = true;
                        row.currency = hc.to_string();
                        row.savings.push(saving);
                        row.saving_pcts
                            .push(((100.0 * saving / hp) * 100.0).round() / 100.0);
                    }
                }
            }
        }
        for (a, x) in other_connections {
            let row = batch.stat(&a, &x, &c);
            row.observations += 1;
            row.successful_connections += 1;
            row.success = true;
        }
    }
    for row in batch.stats.values_mut() {
        row.observations = row.observations.max(row.successful_connections);
        row.successful_connections = row
            .successful_connections
            .max(row.cheaper_than_direct_count);
    }
    batch
}

pub fn merge_savings(existing: &[f64], new: &[f64]) -> Vec<f64> {
    let mut merged = existing.to_vec();
    merged.extend_from_slice(new);
    if merged.len() > MAX_SAVINGS_KEPT {
        merged[merged.len() - MAX_SAVINGS_KEPT..].to_vec()
    } else {
        merged
    }
}

pub fn summarize_savings(savings: &[f64]) -> (f64, f64, f64) {
    if savings.is_empty() {
        return (0.0, 0.0, 0.0);
    }
    let mut sorted = savings.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let avg = ((sorted.iter().sum::<f64>() / sorted.len() as f64) * 100.0).round() / 100.0;
    let med = median(&sorted);
    let mx = ((sorted.last().copied().unwrap_or(0.0)) * 100.0).round() / 100.0;
    (avg, med, mx)
}

fn median(sorted: &[f64]) -> f64 {
    let n = sorted.len();
    if n == 0 {
        return 0.0;
    }
    let v = if n % 2 == 1 {
        sorted[n / 2]
    } else {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    };
    (v * 100.0).round() / 100.0
}
