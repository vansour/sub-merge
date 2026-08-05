// crates/server/src/main.rs
use server::config::AppConfig;
use server::db::{ensure_tokens, init_db};
use server::routes::build_router;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,tower_http=debug".into()),
        )
        .init();

    let cfg = AppConfig::from_env();
    let pool = init_db(&cfg.db_path).await?;
    let (sub_token, admin_token) = ensure_tokens(&pool).await?;
    tracing::info!("subscribe token: {}", sub_token);
    tracing::info!("admin token: {}", admin_token);

    let app = build_router(pool, cfg.clone(), admin_token).await;

    let addr = format!("0.0.0.0:{}", cfg.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("listening on http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
