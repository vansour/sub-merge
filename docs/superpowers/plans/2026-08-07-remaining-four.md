# 剩余 4 项边界问题修复计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复最终评审记录的 4 条仅记录项：秒精度字典序边界（彻底统一）、迁移撕裂窗口、i64 病态 panic、401 三态文案测试缺失。

**Architecture:** 单任务后端修复：① sessions 写入统一秒精度（create_session/validate_session 的 `with_nanosecond(0)` + init_db 规范化既有纳秒行）使字典序比较完全对齐；② 迁移回填恢复无条件执行（幂等，消除 migrated 分支的撕裂窗口）；③ i64 超限语义改为「永不过期」而非 panic；④ parse_bearer 三态纯函数测试。

**Tech Stack:** axum/sqlx/chrono（后端）。

## Global Constraints

- 每次代码修改后必须按序通过：`cargo upgrade -i` → `cargo fmt --all` → `cargo clippy --workspace` → `cargo test --workspace`（CLAUDE.md 强制）
- Rust edition 2024，rust-version 1.97
- 本任务纯后端（db.rs/api_test.rs），无需 dx build
- 基线 140 测试；行为变更点：sessions 存储格式（秒精度）、超大 TTL 语义（永不过期）

---

### Task 1: 后端 4 项边界修复

**Files:**
- Modify: `crates/server/src/db.rs`
- Test: `crates/server/tests/api_test.rs`

**Interfaces:**
- Consumes: 现状 `create_session`/`validate_session`/`delete_expired_sessions`（db.rs）、`parse_bearer`（auth.rs）
- Produces: 行为变更——sessions 写入统一秒精度（既有纳秒行迁移时规范化）；回填无条件执行（幂等）；`ttl_days` 超 i64::MAX 视为永不过期（不 panic 不误删）；`parse_bearer` 三态有测试

- [ ] **Step 1: 写失败/新增测试（api_test.rs）**

```rust
#[test]
fn parse_bearer_three_states() {
    use axum::http::HeaderMap;
    use axum::http::header::{AUTHORIZATION, HeaderValue};

    let mut no_header = HeaderMap::new();
    assert_eq!(
        server::auth::parse_bearer(&no_header),
        Err("missing authorization header")
    );

    let mut wrong_scheme = HeaderMap::new();
    wrong_scheme.insert(AUTHORIZATION, HeaderValue::from_static("Basic abc"));
    assert_eq!(
        server::auth::parse_bearer(&wrong_scheme),
        Err("expected Bearer token")
    );

    let mut ok = HeaderMap::new();
    ok.insert(AUTHORIZATION, HeaderValue::from_static("Bearer tok123"));
    assert_eq!(server::auth::parse_bearer(&ok), Ok("tok123"));
}
```

`user_and_session_db_functions` 追加（TTL 超限 = 永不过期 + 写入秒精度）：

```rust
// 超大 TTL（u64 超 i64::MAX）：不 panic、不过期（永不过期语义）
let t6 = server::db::create_session(&pool).await.unwrap();
sqlx::query("UPDATE sessions SET last_used_at = datetime('now', '-100 days') WHERE token_hash = ?")
    .bind(sha256_hex_manual(&t6))
    .execute(&pool).await.unwrap();
assert!(
    server::db::validate_session(&pool, &t6, u64::MAX).await.unwrap(),
    "ttl 超 i64::MAX 视为永不过期"
);
assert!(server::db::delete_expired_sessions(&pool, u64::MAX).await.is_ok(), "超限 ttl 清理 no-op 不 panic");

// 写入秒精度（无纳秒小数）：新会话 last_used_at 为 'YYYY-MM-DDTHH:MM:SSZ' 形态
let t7 = server::db::create_session(&pool).await.unwrap();
let stored: String = sqlx::query_scalar("SELECT last_used_at FROM sessions WHERE token_hash = ?")
    .bind(sha256_hex_manual(&t7))
    .fetch_one(&pool).await.unwrap();
assert!(
    !stored.contains('.'),
    "session last_used_at must be second precision: {stored}"
);
```

注意：`parse_bearer` 是 `pub fn`（auth.rs 模块公开）——`server::auth::parse_bearer` 可访问。

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --test api_test parse_bearer_three_states user_and_session_db_functions`
Expected: FAIL（parse_bearer 测试通过但 TTL/精度断言失败——写入仍有纳秒、u64::MAX 会 panic）

- [ ] **Step 3: db.rs 三项实现**

写入秒精度（create_session 与 validate_session 的续期写入）：

```rust
// create_session:
let now = chrono::Utc::now().with_nanosecond(0).unwrap_or_else(chrono::Utc::now).to_rfc3339();
// validate_session 续期:
let now = chrono::Utc::now().with_nanosecond(0).unwrap_or_else(chrono::Utc::now).to_rfc3339();
```

（`chrono::Timelike` 已在 use——若 not，补 `use chrono::Timelike;`）

i64 超限 = 永不过期（validate_session）：

```rust
pub async fn validate_session(pool: &SqlitePool, token: &str, ttl_days: u64) -> Result<bool> {
    let hash = sha256_hex(token);
    let row = sqlx::query("SELECT last_used_at FROM sessions WHERE token_hash = ?")
        .bind(&hash).fetch_optional(pool).await?;
    let Some(row) = row else { return Ok(false) };
    let last_used: String = row.get(0);
    // 超 i64::MAX 的病态配置视为永不过期（不 panic 不误删）；0 禁用过期
    if let Ok(ttl) = i64::try_from(ttl_days) {
        if ttl > 0 {
            let expired = match chrono::DateTime::parse_from_rfc3339(&last_used) {
                Ok(last) => last + chrono::Duration::days(ttl) < chrono::Utc::now(),
                Err(_) => true,
            };
            if expired {
                sqlx::query("DELETE FROM sessions WHERE token_hash = ?")
                    .bind(&hash).execute(pool).await?;
                return Ok(false);
            }
        }
    }
    // 滑动续期（写入秒精度）
    let now = chrono::Utc::now().with_nanosecond(0).unwrap_or_else(chrono::Utc::now).to_rfc3339();
    sqlx::query("UPDATE sessions SET last_used_at = ? WHERE token_hash = ?")
        .bind(&now).bind(&hash).execute(pool).await?;
    Ok(true)
}
```

i64 超限 no-op（delete_expired_sessions）：

```rust
pub async fn delete_expired_sessions(pool: &SqlitePool, ttl_days: u64) -> Result<()> {
    let Ok(ttl) = i64::try_from(ttl_days) else {
        return Ok(()); // 病态配置视为永不过期，无清理
    };
    if ttl == 0 { return Ok(()); }
    let cutoff = (chrono::Utc::now() - chrono::Duration::days(ttl))
        .with_nanosecond(0)
        .unwrap_or_else(chrono::Utc::now)
        .to_rfc3339();
    sqlx::query("DELETE FROM sessions WHERE last_used_at < ?")
        .bind(cutoff).execute(pool).await?;
    Ok(())
}
```

迁移：回填恢复无条件执行 + 纳秒规范化（init_db）：

```rust
// sessions 迁移：ALTER 加列（duplicate column 忽略）
if let Err(e) = sqlx::query(
    "ALTER TABLE sessions ADD COLUMN last_used_at TEXT NOT NULL DEFAULT ''",
)
.execute(&pool)
.await
    && !e.to_string().contains("duplicate column name")
{
    return Err(e.into());
}
// 回填 + 规范化无条件执行（幂等：空表/已回填时 0 行匹配）：
// 1) 空值回填（旧库首次迁移，含 ALTER 后重试路径——无条件保证撕裂窗口可自愈）
// 2) 既有纳秒行规范化到秒精度（旧格式 2026-08-07T14:25:09.123Z → 2026-08-07T14:25:09Z），
//    使全库字典序比较对齐（±1s 窗口彻底消除）
sqlx::query(
    "UPDATE sessions SET last_used_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE last_used_at = ''",
)
.execute(&pool).await?;
sqlx::query(
    "UPDATE sessions SET last_used_at = strftime('%Y-%m-%dT%H:%M:%SZ', last_used_at) WHERE last_used_at LIKE '%.%'",
)
.execute(&pool).await?;
```

注意：`strftime('%Y-%m-%dT%H:%M:%SZ', last_used_at)` 对 RFC3339 输入（含 Z/时区偏移）——SQLite strftime 能解析 ISO8601。实施时验证：对 `2026-08-07T14:25:09.123456Z` 应产出 `2026-08-07T14:25:09Z`；若含 `+08:00` 偏移格式（chrono to_rfc3339 默认 Z 或偏移取决于时区——Utc 恒 Z），确认 `LIKE '%.%'` 只命中纳秒行（偏移行无小数不命中，保持原样——偏移格式罕见，validate 的 chrono 解析可处理）。若 strftime 对带偏移输入产出异常（本地时区转换），改用 `substr` 截断方案：`UPDATE ... SET last_used_at = substr(last_used_at, 1, 19) || 'Z' WHERE last_used_at LIKE '%.%'`——**优先用 substr 截断（纯字符串操作，无时区语义风险）**，实施时二选一并在报告中说明选择。

（`use chrono::Timelike;` 若缺失补上——with_nanosecond 需要。）

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --test api_test parse_bearer_three_states user_and_session_db_functions`
Expected: 全 PASS

- [ ] **Step 5: 全量门禁 + commit**

Run: `cargo fmt --all && cargo clippy --workspace && cargo test --workspace`（基线 140 + 新增 parse_bearer 测试 = 141）

```bash
git add crates/server/src/db.rs crates/server/tests/api_test.rs
git commit -m "fix(auth): sessions 存储统一秒精度 + 迁移回填无条件自愈 + TTL 超限永不过期 + parse_bearer 三态测试"
```

---

## 自审记录

**4 项覆盖：**
| 项 | 落点 |
|----|------|
| 秒精度 ±1s 窗口 | Task 1 Step 3（写入 with_nanosecond(0) + 迁移规范化既有纳秒行，字典序全对齐） |
| 迁移撕裂窗口 | Task 1 Step 3（回填恢复无条件执行，幂等自愈——migrated 分支移除） |
| i64 病态 panic | Task 1 Step 3（超限 = 永不过期，validate 跳过检查、清理 no-op） |
| 401 三态测试 | Task 1 Step 1（parse_bearer 纯函数三态断言） |

**类型一致性：** `parse_bearer(headers) -> Result<&str, &'static str>` 已是 auth.rs 公开函数（C 组 Task 1 加入），测试引用一致；db 函数签名不变（仅内部行为）。

**实现提示：** 纳秒规范化的 SQL 二选一（strftime vs substr 截断）——实施时验证 strftime 对 RFC3339 带偏移输入的行为，若有时区风险用 substr 纯字符串截断，报告中说明选择。
