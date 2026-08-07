// crates/server/src/config.rs
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub port: u16,
    pub db_path: PathBuf,
    pub concurrency: usize,
    pub timeout_secs: u64,
    pub max_nodes: usize,
    pub web_dist: PathBuf,
    /// 会话有效期（天）：0 禁用过期（会话永久有效，改密全失效）
    pub session_ttl_days: u64,
}

impl AppConfig {
    pub fn from_env() -> Self {
        let port = std::env::var("PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(8080);
        let db_path = std::env::var("DATABASE_PATH").unwrap_or_else(|_| "./submerge.db".into());
        let concurrency = std::env::var("CONCURRENCY")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(8);
        let timeout_secs = std::env::var("TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(15);
        let max_nodes = std::env::var("MAX_NODES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(2000);
        let web_dist = std::env::var("WEB_DIST").unwrap_or_else(|_| "./web/dist".into());
        let session_ttl_days = std::env::var("SESSION_TTL_DAYS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(30);
        Self {
            port,
            db_path: PathBuf::from(db_path),
            concurrency,
            timeout_secs,
            max_nodes,
            web_dist: PathBuf::from(web_dist),
            session_ttl_days,
        }
    }
}
