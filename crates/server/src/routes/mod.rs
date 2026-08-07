// crates/server/src/routes/mod.rs
pub mod auth;
pub mod combineds;
pub mod config;
pub mod preview;
pub mod sources;
pub mod subscribe;

use crate::state::AppState;
use axum::Router;
use axum::routing::get;

/// SQLite UNIQUE 约束冲突（错误码 2067 / 消息含 UNIQUE constraint failed）
pub(crate) fn is_unique_violation(e: &sqlx::Error) -> bool {
    e.as_database_error()
        .map(|d| d.message().contains("UNIQUE"))
        .unwrap_or(false)
}

/// 组合订阅名：路径段安全（无 URL 编码），限定 [A-Za-z0-9-_]
pub(crate) fn valid_combined_name(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}

pub async fn build_router(pool: sqlx::sqlite::SqlitePool, cfg: crate::config::AppConfig) -> Router {
    let state = AppState::new(pool, cfg);
    let api = Router::new()
        .route("/subscribe/{name}", get(subscribe::subscribe_handler))
        .merge(auth::router())
        .merge(combineds::router())
        .merge(sources::router())
        .merge(preview::router())
        .merge(config::router());

    // 根路径不注册显式路由：由 fallback 返回 SPA index.html（浏览器直接打开 / 即见管理界面）。
    // 健康检查走 /healthz。
    api.route("/healthz", get(|| async { "sub-merge is running" }))
        .fallback(crate::r#static::fallback)
        .with_state(state)
}
