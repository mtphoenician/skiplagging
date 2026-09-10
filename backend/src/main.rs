#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "skiplagging=info,tower_http=info".into()),
        )
        .init();
    if let Err(e) = skiplagging::http::serve().await {
        eprintln!("{e:#}");
        std::process::exit(1);
    }
}
