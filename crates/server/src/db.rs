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
        .busy_timeout(std::time::Duration::from_secs(5))
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .connect_lazy_with(opts);

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS sources (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            url TEXT NOT NULL,
            name TEXT NOT NULL,
            kind TEXT NOT NULL DEFAULT 'remote',
            enabled INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL
        )",
    )
    .execute(&pool)
    .await?;

    // 旧库迁移：早期版本建表无 kind 列。仅当列已存在时忽略 ALTER 失败，
    // 其余错误（IO/锁等）一律传播。
    if let Err(e) =
        sqlx::query("ALTER TABLE sources ADD COLUMN kind TEXT NOT NULL DEFAULT 'remote'")
            .execute(&pool)
            .await
        && !e.to_string().contains("duplicate column name")
    {
        return Err(e.into());
    }

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        )",
    )
    .execute(&pool)
    .await?;

    // 组合订阅：名称唯一；成员为源的子集（多对多）
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS combined_subs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL UNIQUE,
            created_at TEXT NOT NULL
        )",
    )
    .execute(&pool)
    .await?;

    // 多对多关联：删组合/删源均级联清理（依赖 foreign_keys = ON）
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS combined_sources (
            combined_id INTEGER NOT NULL REFERENCES combined_subs(id) ON DELETE CASCADE,
            source_id INTEGER NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
            PRIMARY KEY (combined_id, source_id)
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
    use rand::Rng;
    let mut buf = [0u8; 32];
    rand::rng().fill_bytes(&mut buf);
    hex::encode(buf)
}

/// settings 表是否已初始化 admin token（用于判断是否首次启动、是否需要打印 token）。
pub async fn tokens_initialized(pool: &SqlitePool) -> Result<bool> {
    let row = sqlx::query("SELECT COUNT(*) AS n FROM settings WHERE key = 'admin_token'")
        .fetch_one(pool)
        .await?;
    let n: i64 = row.get(0);
    Ok(n >= 1)
}

/// 环境变量预设的初始 admin token（仅首次初始化时使用）。
/// 已部署实例的 token 稳定：settings 表已有值时环境变量不生效。
pub async fn ensure_tokens(pool: &SqlitePool) -> Result<String> {
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
) -> Result<String> {
    let admin = get_setting(pool, "admin_token").await?;
    match admin {
        Some(s) => Ok(s),
        None => {
            let t = initial("admin_token").unwrap_or_else(gen_token);
            set_setting(pool, "admin_token", &t).await?;
            Ok(t)
        }
    }
}
