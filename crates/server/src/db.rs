// crates/server/src/db.rs
use anyhow::Result;
use sqlx::Row;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use std::path::Path;

pub async fn init_db(path: &Path) -> Result<SqlitePool> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let opts = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .busy_timeout(std::time::Duration::from_secs(5));
    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .connect_lazy_with(opts);

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS sources (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            url TEXT NOT NULL,
            name TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL
        )",
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        )",
    )
    .execute(&pool)
    .await?;

    Ok(pool)
}

pub async fn get_setting(pool: &SqlitePool, key: &str) -> Result<Option<String>> {
    let row = sqlx::query("SELECT value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| r.get::<String, _>(0)))
}

pub async fn set_setting(pool: &SqlitePool, key: &str, value: &str) -> Result<()> {
    sqlx::query("INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value")
        .bind(key)
        .bind(value)
        .execute(pool)
        .await?;
    Ok(())
}

pub fn gen_token() -> String {
    use rand::RngCore;
    let mut buf = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut buf);
    hex::encode(buf)
}

/// settings 表是否已初始化两个 token（用于判断是否首次启动、是否需要打印 token）。
pub async fn tokens_initialized(pool: &SqlitePool) -> Result<bool> {
    let row = sqlx::query(
        "SELECT COUNT(*) AS n FROM settings WHERE key IN ('subscribe_token', 'admin_token')",
    )
    .fetch_one(pool)
    .await?;
    let n: i64 = row.get(0);
    Ok(n >= 2)
}

/// 环境变量预设的初始 token（仅首次初始化时使用）。
/// 已部署实例的 token 稳定：settings 表已有值时环境变量不生效。
pub async fn ensure_tokens(pool: &SqlitePool) -> Result<(String, String)> {
    // 环境变量 SUB_MERGE_SUBSCRIBE_TOKEN / SUB_MERGE_ADMIN_TOKEN 可预设初始 token，
    // 便于部署脚本可控管理；未设置则随机生成（32 字节 hex）。
    ensure_tokens_with(pool, |key| {
        std::env::var(format!("SUB_MERGE_{}", key.to_uppercase()))
            .ok()
            .filter(|s| !s.is_empty())
    })
    .await
}

/// 可注入 token 来源的 ensure_tokens（测试用；生产走环境变量预设或随机生成）。
pub async fn ensure_tokens_with(
    pool: &SqlitePool,
    initial: impl Fn(&str) -> Option<String>,
) -> Result<(String, String)> {
    let sub = get_setting(pool, "subscribe_token").await?;
    let sub = match sub {
        Some(s) => s,
        None => {
            let t = initial("subscribe_token").unwrap_or_else(gen_token);
            set_setting(pool, "subscribe_token", &t).await?;
            t
        }
    };
    let admin = get_setting(pool, "admin_token").await?;
    let admin = match admin {
        Some(s) => s,
        None => {
            let t = initial("admin_token").unwrap_or_else(gen_token);
            set_setting(pool, "admin_token", &t).await?;
            t
        }
    };
    Ok((sub, admin))
}
