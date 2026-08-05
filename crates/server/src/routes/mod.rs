// crates/server/src/routes/mod.rs
// Task 1 只建立最小 Router。subscribe/sources/preview/config 子模块
// 由 Task 2-4 创建时在 lib.rs 和本文件补声明。
use crate::state::AppState;
use axum::routing::get;
use axum::Router;

pub async fn build_router(pool: sqlx::sqlite::SqlitePool, cfg: crate::config::AppConfig, admin_token: String) -> Router {
    let state = AppState::new(pool, cfg, admin_token);
    Router::new()
        .route("/", get(|| async { "sub-merge is running" }))
        .with_state(state)
}
