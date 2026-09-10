use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use crate::models::{Offer, SeparateTicket};

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
const FETCH_TIMEOUT: Duration = Duration::from_millis(1500);

struct RateCache {
    rates: std::collections::HashMap<String, f64>,
    fetched_at: Option<Instant>,
}

fn cache() -> &'static Mutex<RateCache> {
    static CACHE: OnceLock<Mutex<RateCache>> = OnceLock::new();
    CACHE.get_or_init(|| {
        Mutex::new(RateCache {
            rates: FALLBACK.iter().map(|(k, v)| ((*k).to_string(), *v)).collect(),
            fetched_at: None,
        })
    })
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

/// Pull Frankfurter if the cache is empty or past TTL. Sets `fetched_at` only
/// after a successful parse so a failed attempt is retried on the next search.
pub async fn refresh_rates(http: &reqwest::Client) {
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
    let Ok(body) = resp.json::<serde_json::Value>().await else {
        return;
    };
    let Some(quoted) = body.get("rates").and_then(|v| v.as_object()) else {
        return;
    };
    let mut live = std::collections::HashMap::from([("USD".to_string(), 1.0)]);
    for (code, per_usd) in quoted {
        let Some(per) = per_usd.as_f64() else {
            continue;
        };
        if per <= 0.0 {
            continue;
        }
        live.insert(code.to_uppercase(), (1.0 / per * 1_000_000.0).round() / 1_000_000.0);
    }
    if live.len() <= 1 {
        return;
    }
    let mut guard = rates();
    for (k, v) in FALLBACK {
        guard.rates.entry((*k).to_string()).or_insert(*v);
    }
    for (k, v) in live {
        guard.rates.insert(k, v);
    }
    guard.fetched_at = Some(Instant::now());
}

pub fn usd_per_unit(currency: &str) -> Option<f64> {
    let code = if currency.is_empty() {
        "USD"
    } else {
        currency
    }
    .to_uppercase();
    if code == "USD" {
        return Some(1.0);
    }
    rates().rates.get(&code).copied()
}

pub fn to_usd(amount: Option<f64>, currency: Option<&str>) -> Option<f64> {
    let amount = amount?;
    let code = currency.unwrap_or("USD").to_uppercase();
    if code == "USD" {
        return Some((amount * 100.0).round() / 100.0);
    }
    let rate = usd_per_unit(&code)?;
    Some((amount * rate * 100.0).round() / 100.0)
}

pub fn offer_in_usd(offer: &Offer) -> Option<Offer> {
    let code = if offer.currency.is_empty() {
        "USD".to_string()
    } else {
        offer.currency.to_uppercase()
    };
    let price = to_usd(offer.price, Some(&code));
    if offer.price.is_some() && price.is_none() {
        return None;
    }
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
    let already = code == "USD"
        && offer
            .separate_tickets
            .iter()
            .all(|t| t.currency.is_empty() || t.currency.eq_ignore_ascii_case("USD"));
    if already {
        return Some(offer.clone());
    }
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
