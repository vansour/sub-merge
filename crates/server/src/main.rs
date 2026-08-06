// crates/server/src/main.rs
use server::config::AppConfig;
use server::db::init_db;
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
    // 首次初始化时在日志打印 token（warn 级别，仅此一次；之后重启不打印，
    // 避免 token 长期留在公网服务器日志中）
    let first_init = !server::db::tokens_initialized(&pool).await?;
    let admin_token = server::db::ensure_tokens(&pool).await?;
    if first_init {
        tracing::warn!("首次初始化，请妥善保存以下 admin token（仅打印一次）：");
        tracing::warn!("admin_token: {admin_token}");
    }
    // token 是机密，不应在 info 级别输出到日志。仅 debug 级别可见。
    tracing::debug!("admin token: {}", admin_token);

    let app = build_router(pool, cfg.clone(), admin_token).await;

    let addr = format!("0.0.0.0:{}", cfg.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("listening on http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
