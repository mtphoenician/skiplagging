use moka::sync::Cache;
use std::time::Duration;

use crate::config::Settings;
use crate::models::SearchResponse;

pub type SearchCache = Cache<String, SearchResponse>;

pub fn build_cache(settings: &Settings) -> SearchCache {
    Cache::builder()
        .max_capacity(512)
        .time_to_live(Duration::from_secs(settings.cache_ttl_seconds.max(1)))
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
