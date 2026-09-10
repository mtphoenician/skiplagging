use std::env;

/// Live Duffel floors. Env may raise them; mock/sandbox keep `load()` defaults.
/// Duffel excess-search is ~$0.005 per extra `offer_request` after the included quota.
pub const LIVE_FAST_CANDIDATES: usize = 5;
pub const LIVE_MAX_HIDDEN_CANDIDATES: usize = 12;
pub const LIVE_MAX_PROVIDER_CALLS: i32 = 20;

#[derive(Clone, Debug)]
pub struct Settings {
    pub database_url: String,
    pub duffel_token: String,
    pub rapidapi_key: String,
    pub opensky_client_id: String,
    pub opensky_client_secret: String,
    pub cache_ttl_seconds: u64,
    pub offer_ttl_seconds: i64,
    pub max_hidden_candidates: usize,
    pub fast_candidates: usize,
    pub candidate_min_score: f64,
    pub search_cost_usd: f64,
    pub max_concurrency: usize,
    pub mock_enabled: bool,
    pub min_hidden_saving: f64,
    pub max_provider_calls: i32,
    pub cors_origins: String,
    pub discover_skip_dests: String,
}

use std::sync::OnceLock;

static SETTINGS: OnceLock<Settings> = OnceLock::new();

pub fn get_settings() -> &'static Settings {
    SETTINGS.get_or_init(Settings::load)
}

impl Settings {
    pub fn load() -> Self {
        let _ = dotenvy::from_filename("../.env");
        let _ = dotenvy::from_filename(".env");
        Self {
            database_url: env_or(
                "DATABASE_URL",
                "postgresql://matthieutohme@127.0.0.1:5432/skiplagging",
            ),
            duffel_token: env_or("DUFFEL_TOKEN", ""),
            rapidapi_key: env_or("RAPIDAPI_KEY", ""),
            opensky_client_id: env_or("OPENSKY_CLIENT_ID", ""),
            opensky_client_secret: env_or("OPENSKY_CLIENT_SECRET", ""),
            cache_ttl_seconds: env_parse("CACHE_TTL_SECONDS", 180),
            offer_ttl_seconds: env_parse("OFFER_TTL_SECONDS", 900),
            max_hidden_candidates: env_parse("MAX_HIDDEN_CANDIDATES", 8),
            fast_candidates: env_parse("FAST_CANDIDATES", 3),
            candidate_min_score: env_parse("CANDIDATE_MIN_SCORE", 0.35),
            search_cost_usd: env_parse("SEARCH_COST_USD", 0.005),
            max_concurrency: env_parse("MAX_CONCURRENCY", 4),
            mock_enabled: env_bool("MOCK_ENABLED", true),
            min_hidden_saving: env_parse("MIN_HIDDEN_SAVING", 20.0),
            max_provider_calls: env_parse("MAX_PROVIDER_CALLS", 12),
            cors_origins: env_or(
                "CORS_ORIGINS",
                "http://localhost:5173,http://127.0.0.1:5173",
            ),
            discover_skip_dests: env_or("DISCOVER_SKIP_DESTS", "STN"),
        }
    }

    pub fn origin_list(&self) -> Vec<String> {
        self.cors_origins
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }

    pub fn skip_dests(&self) -> std::collections::HashSet<String> {
        self.discover_skip_dests
            .split(',')
            .map(|s| s.trim().to_uppercase())
            .filter(|s| !s.is_empty())
            .collect()
    }

    pub fn duffel_enabled(&self) -> bool {
        !self.duffel_token.is_empty()
    }

    pub fn duffel_live(&self) -> bool {
        self.duffel_token.starts_with("duffel_live_")
    }

    pub fn duffel_sandbox(&self) -> bool {
        self.duffel_enabled() && !self.duffel_live()
    }

    /// Both client id and secret are required; one without the other is still anonymous.
    pub fn opensky_oauth(&self) -> bool {
        !self.opensky_client_id.trim().is_empty() && !self.opensky_client_secret.trim().is_empty()
    }

    pub fn publish_deals(&self) -> bool {
        self.duffel_live()
    }

    pub fn aerodatabox_enabled(&self) -> bool {
        !self.rapidapi_key.is_empty()
    }

    /// Fast-pass ticketed-C shops. Live never drops below 5 even if env is still 3.
    pub fn search_fast_candidates(&self) -> usize {
        if self.duffel_live() {
            self.fast_candidates.max(LIVE_FAST_CANDIDATES)
        } else {
            self.fast_candidates.max(1)
        }
    }

    /// Total ticketed-C shops across fast + expand. Live never drops below 12.
    pub fn search_hidden_candidates(&self) -> usize {
        if self.duffel_live() {
            self.max_hidden_candidates.max(LIVE_MAX_HIDDEN_CANDIDATES)
        } else {
            self.max_hidden_candidates.max(1)
        }
    }

    /// Paid Duffel calls for one `/search` or `/search/expand` ledger. Live never drops below 20.
    pub fn search_call_cap(&self) -> i32 {
        if self.duffel_live() {
            self.max_provider_calls.max(LIVE_MAX_PROVIDER_CALLS)
        } else {
            self.max_provider_calls.max(1)
        }
    }

    pub fn pg_url(&self) -> String {
        self.database_url
            .replace("postgresql+asyncpg://", "postgresql://")
            .replace("postgres+asyncpg://", "postgres://")
    }
}

fn env_or(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_string())
}

fn env_parse<T: std::str::FromStr>(key: &str, default: T) -> T {
    env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_bool(key: &str, default: bool) -> bool {
    match env::var(key) {
        Ok(v) => matches!(v.to_lowercase().as_str(), "1" | "true" | "yes" | "on"),
        Err(_) => default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(token: &str, id: &str, secret: &str) -> Settings {
        Settings {
            database_url: String::new(),
            duffel_token: token.into(),
            rapidapi_key: String::new(),
            opensky_client_id: id.into(),
            opensky_client_secret: secret.into(),
            cache_ttl_seconds: 1,
            offer_ttl_seconds: 1,
            max_hidden_candidates: 1,
            fast_candidates: 1,
            candidate_min_score: 0.0,
            search_cost_usd: 0.0,
            max_concurrency: 1,
            mock_enabled: false,
            min_hidden_saving: 0.0,
            max_provider_calls: 1,
            cors_origins: String::new(),
            discover_skip_dests: String::new(),
        }
    }

    #[test]
    fn duffel_test_token_is_sandbox_not_live() {
        let s = settings("duffel_test_abc", "", "");
        assert!(s.duffel_enabled());
        assert!(!s.duffel_live());
        assert!(s.duffel_sandbox());
        assert!(!s.publish_deals());
    }

    #[test]
    fn duffel_live_prefix_only() {
        let s = settings("duffel_live_abc", "", "");
        assert!(s.duffel_live());
        assert!(!s.duffel_sandbox());
        assert!(s.publish_deals());
    }

    #[test]
    fn opensky_oauth_needs_both_credentials() {
        assert!(!settings("", "id", "").opensky_oauth());
        assert!(!settings("", "", "secret").opensky_oauth());
        assert!(!settings("", "  ", "secret").opensky_oauth());
        assert!(settings("", "id", "secret").opensky_oauth());
    }
}
