use std::cell::Cell;
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::models::{Offer, SeparateTicket};

/// Baked-in USD-per-unit. Tests may stamp `fetched_at` and read these.
/// Production ranking never uses them: `to_usd` is None until Frankfurter
/// sets `fetched_at`, and a successful fetch **replaces** this map (no merge).
const FALLBACK: &[(&str, f64)] = &[
    ("USD", 1.0),
    ("GBP", 1.27),
    ("EUR", 1.08),
    ("AUD", 0.66),
    ("CAD", 0.73),
    ("AED", 0.272),
    ("SGD", 0.74),
    ("HKD", 0.128),
    ("JPY", 0.0067),
    ("CHF", 1.12),
    ("INR", 0.012),
    ("MYR", 0.23),
    ("THB", 0.028),
    ("NZD", 0.59),
    ("ZAR", 0.056),
    ("SAR", 0.267),
    ("QAR", 0.275),
    ("KWD", 3.27),
    ("BHD", 2.65),
    ("OMR", 2.60),
    ("CNY", 0.14),
    ("KRW", 0.00072),
    ("SEK", 0.095),
    ("NOK", 0.094),
    ("DKK", 0.145),
    ("MXN", 0.055),
    ("BRL", 0.18),
    ("TRY", 0.029),
    ("PLN", 0.25),
    ("CZK", 0.043),
    ("ILS", 0.27),
    ("EGP", 0.021),
    ("IDR", 0.000061),
    ("PHP", 0.017),
    ("TWD", 0.031),
];

const TTL: Duration = Duration::from_secs(12 * 3600);
pub const FETCH_TIMEOUT: Duration = Duration::from_secs(4);
pub const BACKGROUND_TICK: Duration = Duration::from_secs(3600);

struct RateCache {
    rates: HashMap<String, f64>,
    fetched_at: Option<Instant>,
}

fn cache() -> &'static Mutex<RateCache> {
    static CACHE: OnceLock<Mutex<RateCache>> = OnceLock::new();
    CACHE.get_or_init(|| {
        Mutex::new(RateCache {
            rates: FALLBACK
                .iter()
                .map(|(k, v)| ((*k).to_string(), *v))
                .collect(),
            fetched_at: None,
        })
    })
}

fn fetch_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

fn rates() -> MutexGuard<'static, RateCache> {
    crate::mutex_lock(cache())
}

fn needs_refresh() -> bool {
    match rates().fetched_at {
        Some(at) => Instant::now().duration_since(at) >= TTL,
        None => true,
    }
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// Parse Frankfurter `{ "rates": { "GBP": 0.79, ... } }` (`from=USD`).
/// Replaces the cache; does not merge FALLBACK. Returns whether `fetched_at` was set.
pub fn apply_frankfurter_body(body: &Value) -> bool {
    let Some(quoted) = body.get("rates").and_then(|v| v.as_object()) else {
        return false;
    };
    let mut live = HashMap::from([("USD".to_string(), 1.0)]);
    for (code, per_usd) in quoted {
        let Some(per) = per_usd.as_f64() else {
            continue;
        };
        if per <= 0.0 {
            continue;
        }
        live.insert(
            code.to_uppercase(),
            (1.0 / per * 1_000_000.0).round() / 1_000_000.0,
        );
    }
    if live.len() <= 1 {
        return false;
    }
    let mut guard = rates();
    guard.rates = live;
    guard.fetched_at = Some(Instant::now());
    true
}

/// Pull Frankfurter if the cache is empty or past TTL. Sets `fetched_at` only
/// after a successful parse so a failed attempt is retried. Last good live
/// rates are kept on failure — never revert to FALLBACK for ranking.
pub async fn refresh_rates(http: &reqwest::Client) {
    if !needs_refresh() {
        return;
    }
    let _gate = fetch_lock().lock().await;
    if !needs_refresh() {
        return;
    }
    let Ok(resp) = http
        .get("https://api.frankfurter.app/latest?from=USD")
        .timeout(FETCH_TIMEOUT)
        .send()
        .await
    else {
        return;
    };
    if !resp.status().is_success() {
        return;
    }
    let Ok(body) = resp.json::<Value>().await else {
        return;
    };
    let _ = apply_frankfurter_body(&body);
}

/// Boot fetch, then retry on a timer so search is not the only refresh.
pub fn spawn_background_refresh(http: reqwest::Client) {
    tokio::spawn(async move {
        refresh_rates(&http).await;
        loop {
            tokio::time::sleep(BACKGROUND_TICK).await;
            refresh_rates(&http).await;
        }
    });
}

thread_local! {
    static FORCE_UNTRUSTED: Cell<bool> = const { Cell::new(false) };
}

pub fn rates_are_live() -> bool {
    if FORCE_UNTRUSTED.with(|c| c.get()) {
        return false;
    }
    rates().fetched_at.is_some()
}

/// Tests (and only tests) may treat baked-in fallbacks as a successful fetch.
pub fn mark_rates_trusted() {
    let mut g = rates();
    if g.fetched_at.is_none() {
        g.fetched_at = Some(Instant::now());
    }
}

/// Integration tests share one process-wide rate cache. Force this thread to
/// behave as if Frankfurter has not succeeded yet.
pub fn with_untrusted_rates<R>(f: impl FnOnce() -> R) -> R {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            FORCE_UNTRUSTED.with(|c| c.set(false));
        }
    }
    let _reset = Reset;
    FORCE_UNTRUSTED.with(|c| c.set(true));
    f()
}

pub fn usd_per_unit(currency: &str) -> Option<f64> {
    let code = if currency.is_empty() { "USD" } else { currency }.to_uppercase();
    if code == "USD" {
        return Some(1.0);
    }
    if !rates_are_live() {
        return None;
    }
    rates().rates.get(&code).copied()
}

pub fn to_usd(amount: Option<f64>, currency: Option<&str>) -> Option<f64> {
    let amount = amount?;
    let code = currency.unwrap_or("USD").to_uppercase();
    if code == "USD" {
        return Some(round2(amount));
    }
    let rate = usd_per_unit(&code)?;
    Some(round2(amount * rate))
}

fn tickets_in_usd(offer: &Offer) -> Option<Vec<SeparateTicket>> {
    let mut tickets = Vec::new();
    for t in &offer.separate_tickets {
        let p = to_usd(Some(t.price), Some(&t.currency))?;
        tickets.push(SeparateTicket {
            origin: t.origin.clone(),
            dest: t.dest.clone(),
            date: t.date.clone(),
            price: p,
            currency: "USD".into(),
            carrier: t.carrier.clone(),
        });
    }
    Some(tickets)
}

pub fn offer_in_usd(offer: &Offer) -> Option<Offer> {
    let code = if offer.currency.is_empty() {
        "USD".to_string()
    } else {
        offer.currency.to_uppercase()
    };
    let already = code == "USD"
        && offer
            .separate_tickets
            .iter()
            .all(|t| t.currency.is_empty() || t.currency.eq_ignore_ascii_case("USD"));
    if already {
        return Some(offer.clone());
    }

    // Duffel already quoted USD on this offer. Do not wait for Frankfurter
    // and do not rescale that amount with FALLBACK.
    if let Some(usd) = offer.quoted_usd.filter(|v| *v > 0.0) {
        let tickets = if offer.separate_tickets.is_empty() {
            vec![]
        } else {
            tickets_in_usd(offer)?
        };
        let mut out = offer.clone();
        out.price = Some(round2(usd));
        out.base_price = to_usd(offer.base_price, Some(&code));
        out.taxes = to_usd(offer.taxes, Some(&code));
        out.currency = "USD".into();
        out.separate_tickets = tickets;
        return Some(out);
    }

    let price = to_usd(offer.price, Some(&code));
    if offer.price.is_some() && price.is_none() {
        return None;
    }
    let tickets = tickets_in_usd(offer)?;
    let mut out = offer.clone();
    out.price = price;
    out.base_price = to_usd(offer.base_price, Some(&code));
    out.taxes = to_usd(offer.taxes, Some(&code));
    out.currency = "USD".into();
    out.separate_tickets = tickets;
    Some(out)
}

pub fn offers_in_usd(offers: &[Offer]) -> Vec<Offer> {
    offers.iter().filter_map(offer_in_usd).collect()
}

pub fn money_in_usd(amount: Option<f64>, currency: Option<&str>) -> (Option<f64>, String) {
    match to_usd(amount, currency) {
        Some(v) => (Some(v), "USD".into()),
        None => (None, "USD".into()),
    }
}
