# 管理端认证改造：用户名+密码登录（替代 Bearer token）

日期：2026-08-07
状态：已批准（待实施）

## 背景与目标

现状管理端用 admin token 鉴权：首次启动随机生成/环境变量预设，前端 localStorage 存 token，每个 /admin/* 请求带 `Authorization: Bearer`。用户要求改为**用户名+密码登录**，且**首次运行未创建管理员时，登录页引导创建管理员用户**。

目标（用户确认的决策）：

1. 不用 token，改用用户名+密码登录
2. 首次运行无管理员 → 登录页引导创建管理员（用户名+密码）
3. 登录后发随机会话 token，前端沿用 Bearer 请求头（改动最小化）
4. 密码用 argon2 哈希存储（不存明文）
5. 单管理员账号（用户名可自定义、密码可修改）
6. 已部署实例破坏性迁移：旧 admin_token 作废，升级后引导重新创建管理员
7. 不引入环境变量预设（`SUB_MERGE_ADMIN_TOKEN` 删除）；引导创建接口无鉴权但一次性锁定

## 架构

### 数据模型（db.rs）

```sql
CREATE TABLE IF NOT EXISTS users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    username TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,   -- argon2 PHC 字符串（含盐）
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS sessions (
    token_hash TEXT PRIMARY KEY,   -- sha256(会话token)，DB 泄露不可直接使用
    created_at TEXT NOT NULL
);
```

- settings 表保留（表结构不动，兼容既有库）；旧 `admin_token` 残留值不清理（不再被任何代码读取）。`get_setting`/`set_setting` 若无调用方则一并删除
- 会话 token：32 字节随机 hex（复用现有 `gen_token` 逻辑），存库前 sha256
- 会话永久有效（自用 YAGNI）；**修改密码后全部会话（含当前）立即失效**

### 后端 API

| 方法/路径 | 鉴权 | 说明 |
|-----------|------|------|
| `GET /admin/setup-status` | 无 | `{"needs_setup": bool}`——登录页据此切换创建/登录表单 |
| `POST /admin/setup` | 无 | 创建管理员（username/password/password_confirm）。**仅当 users 表为空时可用**；创建成功后锁定（再次调用 409）。校验：用户名 trim 后 `[A-Za-z0-9-_]{1,64}`、密码（不 trim）≥8、两次一致 |
| `POST /admin/login` | 无 | username+password → argon2 验证 → 生成会话 token 存 sessions → `{"token": "..."}`。失败统一 401（不区分用户不存在/密码错） |
| `POST /admin/logout` | Bearer | 删除当前会话，204 |
| `GET /admin/config`（改造） | Bearer | 返回 `{"username": "..."}`（原 `admin_token` 字段删除） |
| `PUT /admin/config`（改造） | Bearer | `{"change_password": {"old", "new"}}` → 验证旧密码 → 更新 hash → **删除全部会话**（含当前）→ 前端被踢回登录页 |

无鉴权端点注册为路由、handler 内不调 `require_admin`——保持现状「handler 内显式调用」模式，不引入中间件。

### 鉴权链路改造（auth.rs / state.rs / main.rs）

- `require_admin`：`Authorization: Bearer` 取 token → `sha256(token)` → 查 sessions 表。**删除 `constant_eq`**——token 是 32 字节随机高熵值且哈希后查表，无低熵秘密可比对，时序攻击不适用
- `AppState` 删 `admin_token: Arc<RwLock<String>>` 与 `rotate_admin`；http client、fetch_semaphore 保留
- `main.rs`：删除 `ensure_tokens` / `tokens_initialized` / 首启 token 日志
- `db.rs` 新增函数族：`users_empty` / `create_user`（argon2 hash）/ `verify_user`（argon2 verify）/ `create_session` / `validate_session` / `delete_session` / `delete_all_sessions`
- 新依赖：`argon2 = "0.5"`（RustCrypto，纯 Rust 无 C 依赖；默认参数单次验证约几十 ms，仅在登录/改密时计算）

### 前端改造（web crate）

- **login.rs 双模式**：挂载时 `GET /admin/setup-status`：
  - `needs_setup: true` → 「创建管理员」表单（用户名/密码/确认密码），成功后**自动登录**直接进主界面
  - `false` → 现有登录表单
- **会话存储**：localStorage key `submerge_admin_token` → `submerge_admin_session`（旧 key 残留不影响，启动校验只读新 key）；`read/write/clear_token` 改名。请求头逻辑（api.rs `Authorization: Bearer`）不变
- **App 启动校验**不变：GET /admin/config，仅 401 清除本地会话回登录页
- **config.rs**：Token 卡片 → 「账号」卡片：显示用户名 + 修改密码表单（旧/新/确认）。改密成功 → toast「密码已修改，请重新登录」→ 清会话回登录页
- **退出登录**：先 `POST /admin/logout`（服务端删会话）→ 清 localStorage（失败也照清，本地退出兜底）
- **web-core**：`ConfigDto` 从 `{admin_token}` 改为 `{username}`；dto 测试 fixture 同步更新。`ApiError` 不变

### 错误处理

| 场景 | 状态码 | 前端表现 |
|------|--------|---------|
| 无管理员时调用 setup（正常引导） | 200 | 创建表单 |
| 已有管理员再调 setup | 409（`ApiError` 新增 `conflict()`，`code: "conflict"`） | 切回登录表单 |
| setup 校验失败（用户名格式/密码 <8 位/两次不一致） | 400 | 表单内联错误 |
| login 用户名不存在或密码错误 | 统一 401「用户名或密码错误」 | 不区分原因，防用户名枚举 |
| 会话失效（被删/改密/登出后）访问 /admin/* | 401 | 前端清 localStorage 回登录页（现有 App 启动校验路径） |

### 迁移与清理

- `SUB_MERGE_ADMIN_TOKEN` 环境变量删除（README 环境变量表、CLAUDE.md、compose.yaml 的 `SUB_MERGE_ADMIN_TOKEN: vansour` 一并移除）
- 旧 settings.admin_token 残留值不清理（不再被任何代码读取）
- smoke.sh：从「查 DB 拿 token」改为「POST /admin/setup 创建 + login 拿会话 → Bearer 调 /admin/config」
- 既有集成测试鉴权夹具（`Bearer admin`）→ 改为 setup + login 流程；`db_creates_tables_and_tokens` / `env_preset_tokens_used_only_on_first_init` / `tokens_initialized_reflects_first_init` 删除或改写
- README.md：token 获取方式（3 条）→ 首次引导创建说明；API 表更新；「登录页输入管理 token」表述更新

## 新增测试（server 集成）

1. `setup` 创建管理员成功 → 再调用返回 409（锁定）；用户名/密码校验失败 400
2. `login` 正确凭证 → 200 带 token；错误密码 / 不存在用户 → 401（不区分原因）
3. 带 session 访问 /admin/config → 200；伪造/已删 token → 401
4. `logout` 后原 token 失效
5. `change_password` 成功后所有旧会话（含当前）立即 401；新密码可登录
6. users 表 UNIQUE 约束兜底并发 setup 竞态（不产生双管理员）

## 验证方式

1. cargo 门禁（CLAUDE.md 强制）：`cargo upgrade -i` → `cargo fmt --all` → `cargo clippy --workspace` → `cargo test --workspace`
2. `make smoke`：改版后 9/9 全通过
3. 浏览器人工核对（前端无测试 harness，按 CLAUDE.md 既定流程）：
   - 全新 DB：登录页出现「创建管理员」表单 → 创建后自动登录 → 配置页显示用户名
   - 重启后（已有用户）：登录表单 → 错误密码 401 内联提示 → 正确密码进入
   - 修改密码 → 被踢回登录页 → 新密码可登录，旧密码失败
   - 退出登录 → 本地会话清除 → 再次访问需重新登录
4. 破坏性迁移人工验证：用含旧 admin_token 的库启动 → 登录页显示创建表单（旧 token 不再生效）

## 不做的事（YAGNI）

- 会话过期时间/自动清理（自用工具，永久有效 + 改密全失效已足够）
- 登录失败限流/验证码/2FA
- 多管理员用户
- 环境变量预设初始账号（`SUB_MERGE_ADMIN_TOKEN` 直接删除，不留替代）
- 前端组件渲染测试（dioxus 0.8 alpha 无 harness，维持现状验证链）
