use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};

use crate::engines::index::ticketed_dests_through;
use crate::models::Offer;

pub static WEIGHTS: LazyLock<HashMap<&'static str, f64>> = LazyLock::new(|| {
    HashMap::from([
        ("connection", 0.30),
        ("savings_probability", 0.25),
        ("expected_savings", 0.20),
        ("freshness", 0.10),
        ("hub", 0.10),
        ("provider", 0.05),
    ])
});

pub const DEFAULT_MIN_SCORE: f64 = 0.35;
/// Default cold-start fill in unit tests. Live search passes FAST_CANDIDATES (floor 5).
pub const COLD_START_PROBES: usize = 3;
pub const DEAD_AFTER_CHECKS: i32 = 5;
pub const FRESHNESS_HALF_LIFE_DAYS: f64 = 7.0;
/// Frozen OpenFlights (~2014–2017) is a structural prior, not a timetable.
pub const OPENFLIGHTS_HUB: f64 = 0.35;
/// B→C seen inside a priced ticket, or a hidden-city stat row.
pub const LEARNED_HUB: f64 = 1.0;

pub fn source_priority(source: &str) -> i32 {
    match source {
        "stats" => 4,
        "index" => 3,
        "edges" => 2,
        "openflights" | "hub" => 0,
        _ => 1,
    }
}

pub fn is_learned_source(source: &str) -> bool {
    matches!(source, "stats" | "index" | "edges")
}

/// Keep the stronger planner source. OpenFlights never overwrites learned rows.
pub fn assign_source(pool: &mut HashMap<String, String>, code: String, source: &str) {
    let incoming = source_priority(source);
    let current = pool.get(&code).map(|s| source_priority(s)).unwrap_or(-1);
    if incoming > current {
        pool.insert(code, source.to_string());
    }
}

fn is_learned_candidate(c: &Candidate) -> bool {
    c.observations > 0 || is_learned_source(&c.source)
}

#[derive(Debug, Clone)]
pub struct RouteStat {
    pub origin: String,
    pub intended: String,
    pub ticketed: String,
    pub observations: i32,
    pub successful_connections: i32,
    pub cheaper_than_direct_count: i32,
    pub median_saving: f64,
    pub average_saving_percent: f64,
    pub last_success_at: Option<DateTime<Utc>>,
    pub last_cheaper_at: Option<DateTime<Utc>>,
}

impl RouteStat {
    pub fn new(origin: &str, intended: &str, ticketed: &str) -> Self {
        Self {
            origin: origin.to_string(),
            intended: intended.to_string(),
            ticketed: ticketed.to_string(),
            observations: 0,
            successful_connections: 0,
            cheaper_than_direct_count: 0,
            median_saving: 0.0,
            average_saving_percent: 0.0,
            last_success_at: None,
            last_cheaper_at: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Candidate {
    pub code: String,
    pub score: f64,
    pub source: String,
    pub parts: HashMap<String, f64>,
    pub observations: i32,
    pub successful_connections: i32,
    pub cheaper_than_direct_count: i32,
    pub median_saving: f64,
    pub selected: bool,
}

impl Candidate {
    pub fn new(code: &str, score: f64, source: &str) -> Self {
        Self {
            code: code.to_string(),
            score,
            source: source.to_string(),
            parts: HashMap::new(),
            observations: 0,
            successful_connections: 0,
            cheaper_than_direct_count: 0,
            median_saving: 0.0,
            selected: false,
        }
    }
}

fn smooth(successes: i32, trials: i32) -> f64 {
    if trials <= 0 {
        0.0
    } else {
        (successes as f64 + 0.5) / (trials as f64 + 1.0)
    }
}

pub fn freshness(last: Option<DateTime<Utc>>, now: DateTime<Utc>) -> f64 {
    let last = match last {
        Some(v) => v,
        None => return 0.0,
    };
    let age_days = (now - last).num_seconds().max(0) as f64 / 86400.0;
    0.5_f64.powf(age_days / FRESHNESS_HALF_LIFE_DAYS)
}

pub fn score_candidate(
    stat: Option<&RouteStat>,
    hub_probability: f64,
    provider_rate: f64,
    now: DateTime<Utc>,
) -> (f64, HashMap<String, f64>) {
    let parts = if let Some(stat) = stat {
        HashMap::from([
            (
                "connection".to_string(),
                smooth(stat.successful_connections, stat.observations),
            ),
            (
                "savings_probability".to_string(),
                smooth(stat.cheaper_than_direct_count, stat.observations),
            ),
            (
                "expected_savings".to_string(),
                (stat.average_saving_percent / 100.0).clamp(0.0, 1.0),
            ),
            (
                "freshness".to_string(),
                freshness(stat.last_cheaper_at.or(stat.last_success_at), now),
            ),
            ("hub".to_string(), hub_probability),
            ("provider".to_string(), provider_rate),
        ])
    } else {
        HashMap::from([
            ("connection".to_string(), 0.0),
            ("savings_probability".to_string(), 0.0),
            ("expected_savings".to_string(), 0.0),
            ("freshness".to_string(), 0.0),
            ("hub".to_string(), hub_probability),
            ("provider".to_string(), provider_rate),
        ])
    };
    let score: f64 = WEIGHTS
        .iter()
        .map(|(k, w)| w * parts.get(*k).copied().unwrap_or(0.0))
        .sum();
    let rounded: HashMap<String, f64> = parts
        .into_iter()
        .map(|(k, v)| (k, (v * 10000.0).round() / 10000.0))
        .collect();
    ((score * 10000.0).round() / 10000.0, rounded)
}

pub fn is_dead(stat: Option<&RouteStat>) -> bool {
    stat.map(|s| s.observations >= DEAD_AFTER_CHECKS && s.successful_connections == 0)
        .unwrap_or(false)
}

pub fn hub_probabilities(
    intended: &HashSet<String>,
    openflights: &HashMap<String, HashSet<String>>,
    learned: &HashSet<String>,
) -> HashMap<String, f64> {
    let mut out: HashMap<String, f64> = HashMap::new();
    for b in intended {
        if let Some(cs) = openflights.get(&b.to_uppercase()) {
            for c in cs {
                out.insert(c.to_uppercase(), OPENFLIGHTS_HUB);
            }
        }
    }
    for c in learned {
        let c = c.to_uppercase();
        if c.len() == 3 {
            out.insert(c, LEARNED_HUB);
        }
    }
    out
}

pub fn rank_candidates(
    origin: &str,
    intended: &HashSet<String>,
    stats: &HashMap<String, RouteStat>,
    pool: &HashMap<String, String>,
    hub_prob: &HashMap<String, f64>,
    provider_rate: &HashMap<String, f64>,
    now: Option<DateTime<Utc>>,
) -> Vec<Candidate> {
    let now = now.unwrap_or_else(Utc::now);
    let origin_u = origin.to_uppercase();
    let mut excluded: HashSet<String> = intended.iter().map(|b| b.to_uppercase()).collect();
    excluded.insert(origin_u);
    let mut codes: HashMap<String, String> = HashMap::new();
    for (code, source) in pool {
        let c = code.to_uppercase();
        if c.len() == 3 && !excluded.contains(&c) {
            assign_source(&mut codes, c, source);
        }
    }
    for c in stats.keys() {
        let c = c.to_uppercase();
        if c.len() == 3 && !excluded.contains(&c) {
            codes.insert(c, "stats".into());
        }
    }
    let mut out = Vec::new();
    for (code, source) in codes {
        let stat = stats.get(&code);
        if is_dead(stat) {
            continue;
        }
        let (score, parts) = score_candidate(
            stat,
            hub_prob.get(&code).copied().unwrap_or(0.0),
            provider_rate.get(&code).copied().unwrap_or(0.5),
            now,
        );
        out.push(Candidate {
            code: code.clone(),
            score,
            source: if stat.map(|s| s.observations > 0).unwrap_or(false) {
                "stats".into()
            } else {
                source
            },
            parts,
            observations: stat.map(|s| s.observations).unwrap_or(0),
            successful_connections: stat.map(|s| s.successful_connections).unwrap_or(0),
            cheaper_than_direct_count: stat.map(|s| s.cheaper_than_direct_count).unwrap_or(0),
            median_saving: stat.map(|s| s.median_saving).unwrap_or(0.0),
            selected: false,
        });
    }
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| source_priority(&b.source).cmp(&source_priority(&a.source)))
            .then_with(|| b.observations.cmp(&a.observations))
            .then_with(|| a.code.cmp(&b.code))
    });
    out
}

pub fn select_candidates(
    ranked: &mut [Candidate],
    budget: usize,
    min_score: f64,
    cold_start: usize,
    exclude: &HashSet<String>,
) -> Vec<Candidate> {
    if budget == 0 {
        return vec![];
    }
    let skip: HashSet<String> = exclude.iter().map(|c| c.to_uppercase()).collect();
    let usable: Vec<usize> = ranked
        .iter()
        .enumerate()
        .filter(|(_, c)| !skip.contains(&c.code))
        .map(|(i, _)| i)
        .collect();
    let mut chosen_idx: Vec<usize> = usable
        .iter()
        .copied()
        .filter(|i| ranked[*i].score >= min_score)
        .take(budget)
        .collect();
    let has_learned = usable.iter().any(|i| is_learned_candidate(&ranked[*i]));
    let fill_to = cold_start.min(budget);
    if chosen_idx.len() < fill_to {
        let mut picked: HashSet<String> =
            chosen_idx.iter().map(|i| ranked[*i].code.clone()).collect();
        for i in &usable {
            if chosen_idx.len() >= fill_to {
                break;
            }
            if picked.contains(&ranked[*i].code) {
                continue;
            }
            if has_learned && !is_learned_candidate(&ranked[*i]) {
                continue;
            }
            picked.insert(ranked[*i].code.clone());
            chosen_idx.push(*i);
        }
    }
    for c in ranked.iter_mut() {
        c.selected = false;
    }
    for i in &chosen_idx {
        ranked[*i].selected = true;
    }
    chosen_idx.into_iter().map(|i| ranked[i].clone()).collect()
}

pub fn expected_value(p_useful: f64, p_purchase: f64, commission: f64) -> f64 {
    ((p_useful.max(0.0) * p_purchase.max(0.0) * commission.max(0.0)) * 10000.0).round() / 10000.0
}

#[derive(Debug, Clone)]
pub struct HiddenCityCandidateEdge {
    pub origin: String,
    pub connection: String,
    pub ticketed_destination: String,
    pub observation_count: i32,
    pub last_observed_at: String,
    pub best_observed_price: Option<f64>,
}

static GRAPH: LazyLock<Mutex<HashMap<(String, String, String), HiddenCityCandidateEdge>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn record_offer(origin: &str, connection: &str, ticketed: &str, price: Option<f64>) {
    let key = (
        origin.to_uppercase(),
        connection.to_uppercase(),
        ticketed.to_uppercase(),
    );
    let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let mut g = crate::mutex_lock(&GRAPH);
    if let Some(edge) = g.get_mut(&key) {
        edge.observation_count += 1;
        edge.last_observed_at = now;
        if let Some(p) = price {
            if edge.best_observed_price.map(|b| p < b).unwrap_or(true) {
                edge.best_observed_price = Some(p);
            }
        }
    } else {
        g.insert(
            key.clone(),
            HiddenCityCandidateEdge {
                origin: key.0,
                connection: key.1,
                ticketed_destination: key.2,
                observation_count: 1,
                last_observed_at: now,
                best_observed_price: price,
            },
        );
    }
}

pub fn graph_snapshot() -> Vec<serde_json::Value> {
    let mut edges: Vec<_> = crate::mutex_lock(&GRAPH).values().cloned().collect();
    edges.sort_by(|a, b| {
        b.observation_count
            .cmp(&a.observation_count)
            .then_with(|| a.origin.cmp(&b.origin))
    });
    edges
        .into_iter()
        .map(|e| {
            serde_json::json!({
                "origin": e.origin,
                "connection": e.connection,
                "ticketed_destination": e.ticketed_destination,
                "observation_count": e.observation_count,
                "last_observed_at": e.last_observed_at,
                "best_observed_price": e.best_observed_price,
            })
        })
        .collect()
}

pub fn candidates_beyond(
    origin: &str,
    intended: &HashSet<String>,
    limit: usize,
    extra: Option<&[String]>,
) -> Vec<String> {
    let intended_u: HashSet<String> = intended.iter().map(|c| c.to_uppercase()).collect();
    let origin_u = origin.to_uppercase();
    let mut seen = intended_u.clone();
    seen.insert(origin_u.clone());
    let mut ranked = Vec::new();
    let mut push = |codes: Vec<String>| {
        for code in codes {
            let code = code.to_uppercase();
            if code.len() == 3 && seen.insert(code.clone()) {
                ranked.push(code);
            }
        }
    };
    let mut observed: Vec<(i32, String)> = crate::mutex_lock(&GRAPH)
        .values()
        .filter(|e| e.origin == origin_u && intended_u.contains(&e.connection))
        .map(|e| (e.observation_count, e.ticketed_destination.clone()))
        .collect();
    observed.sort_by(|a, b| b.0.cmp(&a.0));
    push(observed.into_iter().map(|(_, c)| c).collect());
    if let Some(extra) = extra {
        push(extra.to_vec());
    }
    ranked.truncate(limit);
    ranked
}

pub fn plan_expansion(
    origin: &str,
    intended: &HashSet<String>,
    limit: usize,
    extra: Option<&[String]>,
    reused: &[Offer],
    already_hidden: bool,
) -> (Vec<String>, Vec<String>) {
    let dests: HashSet<String> = intended.iter().map(|c| c.to_uppercase()).collect();
    let covered = ticketed_dests_through(reused, &dests);
    let mut skipped: Vec<String> = covered.iter().cloned().collect();
    skipped.sort();
    if already_hidden || limit == 0 {
        return (vec![], skipped);
    }
    let dests: Vec<String> = candidates_beyond(origin, intended, limit + covered.len(), extra)
        .into_iter()
        .filter(|c| !covered.contains(c))
        .take(limit)
        .collect();
    (dests, skipped)
}
