use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::models::Offer;

#[derive(Debug, Clone)]
pub struct ProviderCall {
    pub provider: String,
    pub origin: String,
    pub dest: String,
    pub date: String,
    pub purpose: String,
    pub ok: bool,
    pub offers: i32,
    pub latency_ms: f64,
    pub cost_usd: f64,
}

#[derive(Debug)]
struct LedgerInner {
    cost_per_call: f64,
    max_paid: i32,
    reserved: i32,
    calls: Vec<ProviderCall>,
}

#[derive(Clone)]
pub struct Ledger {
    inner: Arc<Mutex<LedgerInner>>,
}

impl Default for Ledger {
    fn default() -> Self {
        Self::new(0.005, 12)
    }
}

impl Ledger {
    pub fn new(cost_per_call: f64, max_paid: i32) -> Self {
        Self {
            inner: Arc::new(Mutex::new(LedgerInner {
                cost_per_call,
                max_paid,
                reserved: 0,
                calls: vec![],
            })),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, LedgerInner> {
        crate::mutex_lock(&self.inner)
    }

    pub fn total_cost(&self) -> f64 {
        let g = self.lock();
        (g.calls.iter().map(|c| c.cost_usd).sum::<f64>() * 10000.0).round() / 10000.0
    }

    pub fn paid(&self) -> Vec<ProviderCall> {
        self.lock()
            .calls
            .iter()
            .filter(|c| c.cost_usd > 0.0)
            .cloned()
            .collect()
    }

    pub fn calls(&self) -> Vec<ProviderCall> {
        self.lock().calls.clone()
    }

    pub fn remaining(&self) -> i32 {
        let g = self.lock();
        (g.max_paid - g.calls.iter().filter(|c| c.cost_usd > 0.0).count() as i32 - g.reserved).max(0)
    }

    pub fn reserve(&self, n: i32) -> bool {
        if n <= 0 {
            return true;
        }
        let mut g = self.lock();
        let paid = g.calls.iter().filter(|c| c.cost_usd > 0.0).count() as i32;
        if g.max_paid - paid - g.reserved < n {
            return false;
        }
        g.reserved += n;
        true
    }

    fn finish(&self, paid: bool, call: ProviderCall) {
        let mut g = self.lock();
        if paid {
            g.reserved = (g.reserved - 1).max(0);
        }
        g.calls.push(call);
    }

    fn cost_per_call(&self) -> f64 {
        self.lock().cost_per_call
    }
}

tokio::task_local! {
    pub static LEDGER: Ledger;
}

pub fn start_ledger(cost_per_call: f64, max_paid: i32) -> Ledger {
    Ledger::new(cost_per_call, max_paid)
}

pub fn current_ledger() -> Option<Ledger> {
    LEDGER.try_with(|ledger| ledger.clone()).ok()
}

const FREE_PROVIDERS: &[&str] = &["mock"];

pub async fn timed_shop<Fut, E>(
    provider: &str,
    origin: &str,
    dest: &str,
    date: &str,
    purpose: &str,
    fut: Fut,
) -> Vec<Offer>
where
    Fut: Future<Output = Result<Vec<Offer>, E>>,
    E: std::fmt::Display,
{
    let ledger = current_ledger();
    let paid = !FREE_PROVIDERS.iter().any(|p| *p == provider);
    if paid {
        if let Some(ledger) = &ledger {
            if !ledger.reserve(1) {
                return vec![];
            }
        }
    }
    let t0 = Instant::now();
    let (ok, offers) = match fut.await {
        Ok(list) => (true, list),
        Err(e) => {
            tracing::warn!(provider, origin, dest, error = %e, "supplier shop failed");
            (false, vec![])
        }
    };
    if let Some(ledger) = &ledger {
        ledger.finish(
            paid,
            ProviderCall {
                provider: provider.to_string(),
                origin: origin.to_uppercase(),
                dest: dest.to_uppercase(),
                date: date.to_string(),
                purpose: purpose.to_string(),
                ok,
                offers: offers.len() as i32,
                latency_ms: (t0.elapsed().as_secs_f64() * 1000.0 * 10.0).round() / 10.0,
                cost_usd: if paid { ledger.cost_per_call() } else { 0.0 },
            },
        );
    }
    offers
}

pub fn provider_score(ok_count: i32, total: i32, avg_latency_ms: f64) -> f64 {
    if total <= 0 {
        return 0.5;
    }
    let hit = (ok_count as f64 + 0.5) / (total as f64 + 1.0);
    let latency_penalty = (avg_latency_ms.max(0.0) / 10_000.0).clamp(0.0, 0.3);
    ((hit - latency_penalty).clamp(0.0, 1.0) * 10000.0).round() / 10000.0
}
