use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use crate::config::Settings;

pub fn normalize_pg_url(url: &str) -> String {
    url.replace("postgresql+asyncpg://", "postgresql://")
        .replace("postgres+asyncpg://", "postgres://")
}

pub async fn connect() -> anyhow::Result<PgPool> {
    let settings = Settings::load();
    connect_with(&settings).await
}

pub async fn connect_with(settings: &Settings) -> anyhow::Result<PgPool> {
    let url = normalize_pg_url(&settings.pg_url());
    let pool = PgPoolOptions::new()
        .max_connections(12)
        .acquire_timeout(std::time::Duration::from_secs(30))
        .connect(&url)
        .await?;
    Ok(pool)
}
