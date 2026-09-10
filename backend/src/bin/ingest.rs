#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let settings = skiplagging::config::Settings::load();
    let pool = skiplagging::db::pool::connect_with(&settings).await?;
    skiplagging::db::schema::init_db(&pool).await?;
    let summary = skiplagging::db::ingest::ingest(&pool).await?;
    println!("{summary}");
    Ok(())
}
