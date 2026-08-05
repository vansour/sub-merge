// crates/server/src/state.rs
use crate::config::AppConfig;
use sqlx::sqlite::SqlitePool;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub cfg: AppConfig,
    pub http: reqwest::Client,
    pub admin_token: Arc<RwLock<String>>,
}

impl AppState {
    pub fn new(pool: SqlitePool, cfg: AppConfig, admin_token: String) -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(cfg.timeout_secs))
            .build()
            .expect("reqwest client");
        Self {
            pool,
            cfg,
            http,
            admin_token: Arc::new(RwLock::new(admin_token)),
        }
    }

    /// 热轮换内存中的 admin token。DB 已由调用方更新。
    pub async fn rotate_admin(&self, new: String) {
        *self.admin_token.write().await = new;
    }
}
