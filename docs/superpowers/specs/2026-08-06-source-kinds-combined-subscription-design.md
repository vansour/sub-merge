# sub-merge 源类型与命名组合订阅设计

- 日期：2026-08-06
- 状态：已批准
- 前置：全项目代码精读（2026-08-06）

## 1. 背景与目标

当前所有源都被当作远程订阅 URL：添加后统一拉取网络再解析。实际使用中源有两类：

1. **单条节点**（如 `ss://...`、`vmess://...`）——本身就是节点，无需网络拉取，直接解析即可
2. **远程订阅**——需要拉取的订阅链接

同时，组合订阅（合并输出）当前挂在 `/api/subscribe?token=...` 下，订阅 token 对"把链接填进客户端"的使用场景是纯负担。

### 目标
- `sources` 表区分 `single`（单条节点 URI）/ `remote`（远程订阅），表单显式选择
- 组合订阅（最终结果）使用命名路径 `GET /subscribe/{name}?format=...`，**无鉴权**
- 彻底移除订阅 token（settings 表、配置页、轮换、README）

### 非目标（已确认）
- 多组合订阅（一个名字对应一组源的子集）——只做一个组合订阅
- 旧 `/api/subscribe` 保留兼容别名——直接替换
- 单条源支持粘贴多行/多节点——一个源一条 URI
- socks5/http username、SSR protocol/obfs、wireguard IP 等模型字段扩展（沿用 hardening 已确认的非目标）

## 2. 架构与数据流

三层结构不变。改动集中在 server 的 DB 层、service 层、路由层与 web 前端。proxy-core 零改动（`parse_line` 已覆盖单条解析）。

```
sources(kind) ──▶ fetch_and_merge（single 直解 / remote 拉取）──▶ /subscribe/{name}?format=...
```

## 3. 路由表（去 `/api` 前缀）

管理接口与订阅接口就是全部 API 面，`/api` 前缀对 SPA 回退之外的区分无实际价值，一并去掉：

| 方法 | 路径 | 鉴权 | 说明 |
|------|------|------|------|
| GET | `/subscribe/{name}?format=clash\|v2ray\|singbox` | 无 | 组合订阅输出 |
| GET | `/admin/sources` | Bearer | 源列表 |
| POST | `/admin/sources` | Bearer | 添加源 |
| PUT | `/admin/sources/{id}` | Bearer | 更新源 |
| DELETE | `/admin/sources/{id}` | Bearer | 删除源 |
| POST | `/admin/sources/{id}/refresh` | Bearer | 单源刷新 |
| GET | `/admin/preview` | Bearer | 预览 |
| GET | `/admin/config` | Bearer | 配置 |
| PUT | `/admin/config` | Bearer | 轮换 admin token / 改组合订阅名 |
| GET | `/healthz` | 无 | 健康检查 |

`static.rs` fallback 的 API 命名空间守卫由 `starts_with("api")` 改为 `admin`/`subscribe` 前缀：未知的此类路径返回 JSON 404，其余路径照旧 SPA 回退。

## 4. 数据模型与迁移

### sources 表加 `kind` 列

- 新库建表 SQL 直接带 `kind TEXT NOT NULL DEFAULT 'remote'`
- 旧库：启动时 `ALTER TABLE sources ADD COLUMN kind ...`（列已存在时报错，忽略）

### settings 表

- 新增 `combined_name`（默认 `merged`，校验 `[A-Za-z0-9-_]`，违者 400——路径段安全，无需 URL 编码）
- 不再写入 `subscribe_token`；旧库残留行无害（不再被读取）
- `tokens_initialized` / `ensure_tokens` 只处理 `admin_token`；`first_init` 判断随之简化

### PUT /admin/config 请求体

可选字段，可同时出现：

```json
{ "rotate": "admin", "combined_name": "merged" }
```

- `rotate` 仅接受 `"admin"`（订阅 token 轮换已随订阅 token 删除；其他值 400）
- `combined_name` 存在时校验并保存；校验失败 400
- 响应与 GET 一致：`{ "admin_token", "combined_name", "subscribe_url" }`（`subscribe_url` 为 `/subscribe/{name}`）

## 5. service 层：按 kind 分支

`fetch_and_merge` 对每个 enabled 源：

- `single`：`proxy_core::parser::parse_line(url)` 直接解析，不发起网络请求；解析失败 → `SourceError`（与远程源失败同构）
- `remote`：现有拉取 + `parse_subscription_text` 流程不变

`refresh_source`（`POST /admin/sources/{id}/refresh`）：single 源本地重解析一次（不拉网络），remote 源照旧拉取；响应结构（`ok`/`node_count`/`reason`）不变，前端按钮无需改动。

## 6. 组合订阅路由

`GET /subscribe/{name}?format=...`（无鉴权）：

- `{name}` 不匹配 settings 的 `combined_name` → 404 JSON（统一错误格式）
- `format` 缺省 clash；非法值 → 400
- 合并逻辑、全部源失败 502 附明细、序列化、Content-Type、`profile-update-interval` 头全部复用现有 `subscribe_handler` 实现
- 删除旧路由 `/api/subscribe` 与订阅 token 恒定时间校验

## 7. 前端（web crate）

- **sources 页**：添加表单加「类型」下拉（单条节点 / 远程订阅，默认远程订阅）；类型为单条时 URL 占位符提示改为节点 URI 示例；表格加「类型」列（徽章）；`SourceDto` 加 `kind` 字段
- **config 页**：
  - 删除订阅 token 展示行与轮换按钮（仅保留管理 token）
  - 新增「组合订阅名称」输入框 + 保存按钮（`PUT /admin/config` 带 `combined_name`），保存成功刷新链接
  - 订阅链接：`{base}/subscribe/{name}?format=clash|v2ray|singbox`（无 token）
  - `ConfigDto`：去掉 `subscribe_token`，加 `combined_name`
- **api.rs**：所有路径去掉 `/api` 前缀
- overview / preview 页：无结构性改动（仅路径随 api.rs 更新）

## 8. 错误处理与边界

- 名字不匹配 404、format 非法 400、全部源失败 502（附明细）
- single 源解析失败 → 源错误列表（preview errors + 502 明细），与远程源失败同一路径
- `combined_name` 保存校验失败 → 400
- admin token 轮换逻辑不变（DB + 内存热轮换，前端会话自动同步）

## 9. 测试

- **server 集成测试**：
  - 路径批量更新 `/api/admin/*` → `/admin/*`
  - 删除订阅 token 相关测试；新增：无 token 可访问 `/subscribe/{name}`、名字不匹配 404、非法 format 400、改名后旧名 404
  - 新增 single 源测试：创建 `kind=single` → 合并输出含节点；非法 URI → 进源错误列表；single 源零网络请求（wiremock 断言）
  - 旧库迁移测试：预置无 `kind` 列的库 → 启动后 ALTER 成功、旧源默认为 remote
- **smoke.sh**：步骤 5-8 改为新路径与无 token 订阅；DB 只查 `admin_token`
- **proxy-core**：零改动（已有 `parse_line` 测试覆盖）
- 前端：`dx build` + `make smoke` + 浏览器人工核对（无测试 harness，沿用既有验证方式）
