// crates/server/src/state.rs
use crate::config::AppConfig;
use sqlx::sqlite::SqlitePool;
use std::sync::Arc;
use tokio::sync::{RwLock, Semaphore};

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub cfg: AppConfig,
    pub http: reqwest::Client,
    pub admin_token: Arc<RwLock<String>>,
    /// 全局出站拉取信号量：跨请求共享，CONCURRENCY 约束整个进程的并发拉取数，
    /// 防止无鉴权订阅端点被并发请求放大出站流量（每请求新建信号量则放大 N×cap）。
    pub fetch_semaphore: Arc<Semaphore>,
}

impl AppState {
    pub fn new(pool: SqlitePool, cfg: AppConfig, admin_token: String) -> Self {
        let concurrency = cfg.concurrency.max(1);
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(cfg.timeout_secs))
            .build()
            .expect("reqwest client");
        Self {
            pool,
            cfg,
            http,
            admin_token: Arc::new(RwLock::new(admin_token)),
            fetch_semaphore: Arc::new(Semaphore::new(concurrency)),
        }
    }

    /// 热轮换内存中的 admin token。DB 已由调用方更新。
    pub async fn rotate_admin(&self, new: String) {
        *self.admin_token.write().await = new;
    }
}
