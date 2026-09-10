use clap::Parser;

#[derive(Parser)]
struct Args {
    #[arg(long, default_value_t = 28)]
    limit: i64,
    #[arg(long, default_value_t = 22)]
    dests: i64,
    #[arg(long, default_value = "")]
    date: String,
    #[arg(long)]
    wipe: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let settings = skiplagging::config::Settings::load();
    let pool = skiplagging::db::pool::connect_with(&settings).await?;
    skiplagging::db::schema::init_db(&pool).await?;
    if args.wipe {
        let n = skiplagging::db::repo::clear_hidden_deals(&pool).await?;
        println!("wiped {n} stored deal(s)");
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(40))
        .build()?;
    let day = if args.date.is_empty() {
        None
    } else {
        Some(args.date)
    };
    let summary = skiplagging::engines::discover::discover_hidden_deals(
        &pool,
        &settings,
        &client,
        args.limit,
        args.dests,
        day,
        None,
    )
    .await?;
    println!("{summary}");
    Ok(())
}
