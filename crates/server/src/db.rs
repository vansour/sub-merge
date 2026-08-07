// crates/server/src/db.rs
use anyhow::Result;
use argon2::Argon2;
use argon2::password_hash::{
    PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng,
};
use sha2::{Digest, Sha256};
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

    // 用户与会话：密码 argon2 哈希存储，会话只存 token 的 sha256
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            username TEXT NOT NULL UNIQUE,
            password_hash TEXT NOT NULL,
            created_at TEXT NOT NULL
        )",
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS sessions (
            token_hash TEXT PRIMARY KEY,
            created_at TEXT NOT NULL
        )",
    )
    .execute(&pool)
    .await?;

    Ok(pool)
}

pub fn gen_token() -> String {
    use rand::Rng;
    let mut buf = [0u8; 32];
    rand::rng().fill_bytes(&mut buf);
    const_hex::encode(buf)
}

fn sha256_hex(s: &str) -> String {
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    const_hex::encode(h.finalize())
}

pub async fn users_empty(pool: &SqlitePool) -> Result<bool> {
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await?;
    Ok(n == 0)
}

fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("argon2 hash failed: {e}"))?
        .to_string();
    Ok(hash)
}

fn verify_password_hash(password: &str, stored: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(stored) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

pub async fn create_user(pool: &SqlitePool, username: &str, password: &str) -> Result<()> {
    let hash = hash_password(password)?;
    let created_at = chrono::Utc::now().to_rfc3339();
    sqlx::query("INSERT INTO users (username, password_hash, created_at) VALUES (?, ?, ?)")
        .bind(username)
        .bind(&hash)
        .bind(created_at)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn verify_user(pool: &SqlitePool, username: &str, password: &str) -> Result<bool> {
    let row = sqlx::query("SELECT password_hash FROM users WHERE username = ?")
        .bind(username)
        .fetch_optional(pool)
        .await?;
    let Some(row) = row else {
        return Ok(false);
    };
    let stored: String = row.get(0);
    Ok(verify_password_hash(password, &stored))
}

pub async fn update_password(pool: &SqlitePool, username: &str, new_password: &str) -> Result<()> {
    let hash = hash_password(new_password)?;
    sqlx::query("UPDATE users SET password_hash = ? WHERE username = ?")
        .bind(hash)
        .bind(username)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn get_username(pool: &SqlitePool) -> Result<Option<String>> {
    let row = sqlx::query("SELECT username FROM users LIMIT 1")
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| r.get::<String, _>(0)))
}

pub async fn create_session(pool: &SqlitePool) -> Result<String> {
    let token = gen_token();
    let created_at = chrono::Utc::now().to_rfc3339();
    sqlx::query("INSERT INTO sessions (token_hash, created_at) VALUES (?, ?)")
        .bind(sha256_hex(&token))
        .bind(created_at)
        .execute(pool)
        .await?;
    Ok(token)
}

pub async fn validate_session(pool: &SqlitePool, token: &str) -> Result<bool> {
    let row = sqlx::query("SELECT 1 FROM sessions WHERE token_hash = ?")
        .bind(sha256_hex(token))
        .fetch_optional(pool)
        .await?;
    Ok(row.is_some())
}

pub async fn delete_session(pool: &SqlitePool, token: &str) -> Result<()> {
    sqlx::query("DELETE FROM sessions WHERE token_hash = ?")
        .bind(sha256_hex(token))
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_all_sessions(pool: &SqlitePool) -> Result<()> {
    sqlx::query("DELETE FROM sessions").execute(pool).await?;
    Ok(())
}
