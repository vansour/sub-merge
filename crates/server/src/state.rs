// crates/server/src/state.rs
use crate::config::AppConfig;
use sqlx::sqlite::SqlitePool;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub cfg: AppConfig,
    pub http: reqwest::Client,
    pub admin_token: String,
}

impl AppState {
    pub fn new(pool: SqlitePool, cfg: AppConfig, admin_token: String) -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(cfg.timeout_secs))
            .build()
            .expect("reqwest client");
        Self { pool, cfg, http, admin_token }
    }
}
