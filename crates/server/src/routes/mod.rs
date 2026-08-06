// crates/server/src/routes/mod.rs
// Task 1 只建立最小 Router。subscribe/sources/preview/config 子模块
// 由 Task 2-4 创建时在 lib.rs 和本文件补声明。
pub mod config;
pub mod preview;
pub mod sources;
pub mod subscribe;

use crate::state::AppState;
use axum::Router;
use axum::routing::get;

pub async fn build_router(
    pool: sqlx::sqlite::SqlitePool,
    cfg: crate::config::AppConfig,
    admin_token: String,
) -> Router {
    let state = AppState::new(pool, cfg, admin_token);
    let api = Router::new()
        .route("/api/subscribe", get(subscribe::subscribe_handler))
        .merge(sources::router())
        .merge(preview::router())
        .merge(config::router());

    // 根路径不注册显式路由：由 fallback 返回 SPA index.html（浏览器直接打开 / 即见管理界面）。
    // 健康检查走 /healthz。
    api.route("/healthz", get(|| async { "sub-merge is running" }))
        .fallback(crate::r#static::fallback)
        .with_state(state)
}
