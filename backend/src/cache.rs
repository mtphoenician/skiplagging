use moka::sync::Cache;
use std::sync::Arc;
use std::time::Duration;

use crate::config::Settings;
use crate::models::SearchResponse;

pub type SearchCache = Cache<String, Arc<SearchResponse>>;

/// Live Duffel: POST /search memory TTL. Mock/sandbox keep CACHE_TTL_SECONDS (180).
pub const LIVE_HTTP_CACHE_TTL_SECS: u64 = 45;

/// Live Duffel: cap the HTTP search cache at 45s so a repeat is not a 3-minute-old shop.
/// Honest A→B reuse is a separate 60s rule inside the shop planner.
pub fn search_cache_ttl(settings: &Settings) -> u64 {
    if settings.duffel_live() {
        settings
            .cache_ttl_seconds
            .min(LIVE_HTTP_CACHE_TTL_SECS)
            .max(1)
    } else {
        settings.cache_ttl_seconds.max(1)
    }
}

/// `Cache-Control: no-cache` / `no-store` / `max-age=0` skip the in-memory search cache.
pub fn cache_control_bypasses(header: Option<&str>) -> bool {
    let Some(raw) = header else {
        return false;
    };
    raw.split(',').any(|directive| {
        let d = directive.trim().to_ascii_lowercase();
        d == "no-cache" || d == "no-store" || d == "max-age=0"
    })
}

pub fn stamp_http_cache_hit(planner: &str) -> String {
    let inner = planner
        .strip_prefix("http-cache(")
        .and_then(|s| s.strip_suffix(')'))
        .unwrap_or(planner);
    format!("http-cache({inner})")
}

pub fn build_cache(settings: &Settings) -> SearchCache {
    Cache::builder()
        .max_capacity(4096)
        .weigher(|key: &String, value: &Arc<SearchResponse>| {
            let offers: u32 = value.channels.iter().map(|c| c.offers.len() as u32).sum();
            let hidden = value.hidden_city.len() as u32;
            let base = 8u32 + (key.len() as u32 / 32);
            base.saturating_add(offers)
                .saturating_add(hidden.saturating_mul(2))
        })
        .time_to_live(Duration::from_secs(search_cache_ttl(settings)))
        .build()
}

pub fn search_key(
    origin: &str,
    dest: &str,
    date: &str,
    adults: i32,
    cabin: &str,
    nearby: bool,
    live: bool,
    _currency: &str,
    return_date: Option<&str>,
) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}|{}|{}|USD|v9",
        origin.to_uppercase(),
        dest.to_uppercase(),
        date,
        return_date.unwrap_or(""),
        adults,
        cabin,
        nearby,
        live
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_duffel_caps_http_cache_at_45s() {
        let mut settings = crate::config::Settings::load();
        settings.cache_ttl_seconds = 180;
        settings.duffel_token = "duffel_live_unit".into();
        assert_eq!(search_cache_ttl(&settings), 45);
        settings.duffel_token = "duffel_test_abc".into();
        assert_eq!(search_cache_ttl(&settings), 180);
        settings.duffel_token = String::new();
        settings.cache_ttl_seconds = 0;
        assert_eq!(search_cache_ttl(&settings), 1);
    }

    #[test]
    fn cache_control_no_cache_bypasses() {
        assert!(!cache_control_bypasses(None));
        assert!(!cache_control_bypasses(Some("max-age=180")));
        assert!(cache_control_bypasses(Some("no-cache")));
        assert!(cache_control_bypasses(Some("no-store")));
        assert!(cache_control_bypasses(Some("max-age=0")));
        assert!(cache_control_bypasses(Some("private, no-cache, max-age=0")));
        assert_eq!(stamp_http_cache_hit("index+live"), "http-cache(index+live)");
        assert_eq!(
            stamp_http_cache_hit("http-cache(index)"),
            "http-cache(index)"
        );
        assert_ne!(stamp_http_cache_hit("index+live"), "fresh");
    }
}
