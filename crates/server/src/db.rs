// crates/server/src/db.rs
use anyhow::Result;
use argon2::Argon2;
use argon2::password_hash::{
    PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng,
};
use chrono::Timelike;
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
            created_at TEXT NOT NULL,
            last_used_at TEXT NOT NULL
        )",
    )
    .execute(&pool)
    .await?;

    // 旧库迁移：早期版本建表无 last_used_at 列（会话 TTL 功能前）。仅当列已存在时
    // 忽略 ALTER 失败，其余错误（IO/锁等）一律传播——沿用 sources.kind 迁移模式。
    // 注意：SQLite 的 ALTER TABLE ADD COLUMN 不允许非常量 DEFAULT（datetime('now')
    // 会被拒绝），故先用空串常量默认值加列，再回填 RFC3339 格式的当前时间——
    // 保持"旧会话获得全新 last_used_at、迁移后仍有效"的意图。
    // 回填与规范化无条件执行（幂等：空表/已回填/无纳秒行时 0 行匹配，无害）：
    // 1) 空值回填无条件跑——不再用 migrated 分支按 ALTER 是否成功决定是否回填，
    //    消除「ALTER 成功但回填失败 → 重试时跳过回填」的撕裂窗口（重试路径靠
    //    无条件 UPDATE 自愈）；新库（CREATE TABLE 自带列）空表回填 0 行，正确。
    // 2) 既有纳秒行规范化到秒精度（2026-08-07T14:25:09.123Z → 2026-08-07T14:25:09Z）。
    //    用 substr 纯字符串截断而非 strftime 重排：strftime 对带时区偏移的输入会
    //    做 UTC 转换（+08:00 → 06:25:09Z），有隐式时区语义；substr 仅截取前 19 字符
    //    并补 Z，纯词法操作无时区风险。LIKE '%.%' 只命中带小数的纳秒行，偏移行
    //    （无小数）与已秒精度的行不被触碰。全库统一秒精度后字典序比较对齐
    //    （±1s 窗口彻底消除）。
    if let Err(e) =
        sqlx::query("ALTER TABLE sessions ADD COLUMN last_used_at TEXT NOT NULL DEFAULT ''")
            .execute(&pool)
            .await
        && !e.to_string().contains("duplicate column name")
    {
        return Err(e.into());
    }
    sqlx::query(
        "UPDATE sessions SET last_used_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') \
         WHERE last_used_at = ''",
    )
    .execute(&pool)
    .await?;
    sqlx::query(
        "UPDATE sessions SET last_used_at = substr(last_used_at, 1, 19) || 'Z' \
         WHERE last_used_at LIKE '%.%'",
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

/// 原子创建管理员：单语句 INSERT...WHERE NOT EXISTS 防并发双管理员。
/// 返回 Ok(true)=创建成功；Ok(false)=已存在（调用方转 409）。
pub async fn create_user_if_empty(
    pool: &SqlitePool,
    username: &str,
    password: &str,
) -> Result<bool> {
    let hash = hash_password(password)?;
    let created_at = chrono::Utc::now().to_rfc3339();
    let res = sqlx::query(
        "INSERT INTO users (username, password_hash, created_at) \
         SELECT ?, ?, ? WHERE NOT EXISTS (SELECT 1 FROM users)",
    )
    .bind(username)
    .bind(&hash)
    .bind(created_at)
    .execute(pool)
    .await?;
    Ok(res.rows_affected() > 0)
}

// 模块级常量：预计算的 argon2id dummy hash（固定盐），用户不存在时验证它，
// 使不存在与密码错误的耗时一致（时序型用户名枚举防护）。
// 由本 crate 的 hash_password("dummy-password") 生成（Argon2::default 参数），
// 保证 PasswordHash::new 可解析、verify 跑完整 argon2。
const DUMMY_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$ZzWOQI/mqH81rndoNXBf7A$kBTvZXE45uPlI/LkuR50OQhpbmi4h0IvXnSSwhogAos";

pub async fn verify_user(pool: &SqlitePool, username: &str, password: &str) -> Result<bool> {
    let row = sqlx::query("SELECT password_hash FROM users WHERE username = ?")
        .bind(username)
        .fetch_optional(pool)
        .await?;
    let stored = match row {
        Some(r) => r.get::<String, _>(0),
        // 恒定时间：用户不存在也跑一次 argon2 verify（防时序枚举）。
        // 注意不能直接返回 DUMMY_HASH 的 verify 结果——该 hash 由源码中固定的
        // 密码生成，直接返回会让"密码=固定密码"的请求对不存在用户通过登录。
        None => {
            let _verified = verify_password_hash(password, DUMMY_HASH);
            return Ok(false);
        }
    };
    Ok(verify_password_hash(password, &stored))
}

pub async fn update_password(pool: &SqlitePool, username: &str, new_password: &str) -> Result<()> {
    let hash = hash_password(new_password)?;
    let res = sqlx::query("UPDATE users SET password_hash = ? WHERE username = ?")
        .bind(hash)
        .bind(username)
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(anyhow::anyhow!("no user with username {username}"));
    }
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
    // 写入秒精度：与迁移规范化/delete_expired_sessions cutoff 对齐，字典序比较无 ±1s 窗口
    let now = chrono::Utc::now()
        .with_nanosecond(0)
        .unwrap_or_else(chrono::Utc::now)
        .to_rfc3339();
    sqlx::query("INSERT INTO sessions (token_hash, created_at, last_used_at) VALUES (?, ?, ?)")
        .bind(sha256_hex(&token))
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await?;
    Ok(token)
}

/// 校验会话：ttl_days > 0 时超过有效期（last_used + ttl < now）的会话删除并返回 false；
/// 有效会话滑动续期（UPDATE last_used_at）。ttl_days == 0 禁用过期。
pub async fn validate_session(pool: &SqlitePool, token: &str, ttl_days: u64) -> Result<bool> {
    let hash = sha256_hex(token);
    let row = sqlx::query("SELECT last_used_at FROM sessions WHERE token_hash = ?")
        .bind(&hash)
        .fetch_optional(pool)
        .await?;
    let Some(row) = row else { return Ok(false) };
    let last_used: String = row.get(0);
    // 病态配置（超出 chrono TimeDelta 上限，含 u64::MAX/i64::MAX 溢出带）视为永不过期：
    // 跳过过期检查直接续期，不 panic 不误删；0 禁用过期。TimeDelta::try_days 返回 Option，
    // None 即超限（Duration::days 对 >106,751,991,167 天会溢出 panic，故必须用 try_days）。
    if let Ok(ttl) = i64::try_from(ttl_days)
        && ttl > 0
        && let Some(ttl) = chrono::TimeDelta::try_days(ttl)
    {
        let expired = match chrono::DateTime::parse_from_rfc3339(&last_used) {
            Ok(last) => last + ttl < chrono::Utc::now(),
            Err(_) => true, // 无法解析视为过期（含旧格式空串）
        };
        if expired {
            sqlx::query("DELETE FROM sessions WHERE token_hash = ?")
                .bind(&hash)
                .execute(pool)
                .await?;
            return Ok(false);
        }
    }
    // 滑动续期（写入秒精度）
    let now = chrono::Utc::now()
        .with_nanosecond(0)
        .unwrap_or_else(chrono::Utc::now)
        .to_rfc3339();
    sqlx::query("UPDATE sessions SET last_used_at = ? WHERE token_hash = ?")
        .bind(&now)
        .bind(&hash)
        .execute(pool)
        .await?;
    Ok(true)
}

/// 启动清理：删除所有过期会话（ttl_days == 0 或超出 chrono TimeDelta 上限的病态配置
/// 时 no-op，后者视为永不过期，不 panic 不误删）。
pub async fn delete_expired_sessions(pool: &SqlitePool, ttl_days: u64) -> Result<()> {
    let Ok(days) = i64::try_from(ttl_days) else {
        return Ok(()); // 病态配置视为永不过期，无清理
    };
    if days == 0 {
        return Ok(());
    }
    let Some(ttl) = chrono::TimeDelta::try_days(days) else {
        return Ok(()); // 超 TimeDelta 上限（Duration::days 溢出带），视为永不过期
    };
    // cutoff 截断到秒精度，与迁移回填/存储格式对齐（RFC3339 秒级），
    // 避免纳秒级 now 与秒级 last_used_at 字典序比较产生 1 秒窗口误删
    let cutoff = (chrono::Utc::now() - ttl)
        .with_nanosecond(0) // 截断到秒精度，与迁移回填/存储格式对齐
        .unwrap_or_else(chrono::Utc::now)
        .to_rfc3339();
    sqlx::query("DELETE FROM sessions WHERE last_used_at < ?")
        .bind(cutoff)
        .execute(pool)
        .await?;
    Ok(())
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

pub async fn get_setting(pool: &SqlitePool, key: &str) -> Result<Option<String>> {
    let row = sqlx::query("SELECT value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| r.get::<String, _>(0)))
}

pub async fn set_setting(pool: &SqlitePool, key: &str, value: &str) -> Result<()> {
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await?;
    Ok(())
}
