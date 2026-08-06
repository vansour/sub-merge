# sub-merge 多组合订阅设计

- 日期：2026-08-06
- 状态：已批准
- 前置：2026-08-06 全项目代码审查与修复（含 2026-08-06-source-kinds-combined-subscription 单组合设计）

## 1. 背景与目标

当前只有一个组合订阅（settings 的 `combined_name`，默认 merged），组合订阅链接挂在配置页。实际使用需要**多个组合订阅**：每个组合从全部源中勾选成员（单个节点源与远程订阅源均可），一个源可属于多个组合（多对多）；组合订阅管理作为**侧边栏独立页面**，不再内嵌在配置页。

### 目标
- 多组合订阅：任意数量，每组合独立名字（`/subscribe/{name}` 命名路由）
- 组合成员 = 源的多选子集（多对多，关联表）
- 组合订阅页（侧边栏独立导航项）：组合 CRUD + 成员勾选 + 三种格式链接复制
- 预览页支持按组合切换

### 非目标（已确认）
- 旧组合订阅不迁移：升级后无默认组合，旧 `/subscribe/merged` 链接失效（404）
- 组合成员顺序/排序自定义（按源 id 顺序）
- 组合级过滤/去重/测速等增强
- 组合间复制成员

## 2. 数据模型与迁移

### 新表

```sql
CREATE TABLE IF NOT EXISTS combined_subs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS combined_sources (
    combined_id INTEGER NOT NULL REFERENCES combined_subs(id) ON DELETE CASCADE,
    source_id INTEGER NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
    PRIMARY KEY (combined_id, source_id)
);
```

- `init_db` 中开启 `PRAGMA foreign_keys = ON`（`SqliteConnectOptions::foreign_keys(true)`），级联删除生效
- 组合名 `[A-Za-z0-9-_]` 校验、唯一（冲突 400）；改名后旧名 404

### 不迁移

- settings 的 `combined_name` 残留无害（不再读取）；不创建默认组合
- 旧 `/subscribe/merged` 链接 404

## 3. 后端路由与逻辑

### 组合 CRUD（Bearer 鉴权，`/admin/combineds`）

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/admin/combineds` | 组合列表（含成员 source_id 数组） |
| POST | `/admin/combineds` | 创建（`{name, source_ids?}`；名字校验唯一，非法 400） |
| PUT | `/admin/combineds/{id}` | 更新（`{name?, source_ids?}`；source_ids 全量替换成员） |
| DELETE | `/admin/combineds/{id}` | 删除（级联删成员关系） |

- `source_ids` 引用不存在的源：忽略（幂等），不报错
- 删除源时 `combined_sources` 引用级联清理（外键）

### 订阅端点

`GET /subscribe/{name}?format=...`（无鉴权，不变）：名字匹配 `combined_subs`，不匹配 404；按成员源拉取合并。

### 预览端点

`GET /admin/preview?combined=<name>`：省略参数 = 全部 enabled 源（现状）；指定名字 → 按成员过滤；名字不存在 404。

### service 层

`fetch_and_merge` 增加可选源 id 过滤集参数（`Option<&[i64]>`）：
- `None` → 现状（全部 enabled 源），现有测试兼容
- `Some(ids)` → `WHERE enabled = 1 AND id IN (ids)`
- 成员为空的组合：200 空输出（不 502）；全部成员源失败 → 502 附明细（同现有语义）

## 4. 前端（web crate）

- 侧边栏新增导航项「组合订阅」（订阅源之后）：新组件 `Combineds` + 新图标
- **组合订阅页**：组合列表（名字 mono + 成员数徽章 + 三种格式链接复制按钮）；新建/编辑弹窗（名字输入 + 成员源多选复选框，显示源名 + 类型徽章 + 启用状态）；删除确认弹窗（复用 ConfirmDialog）
- **配置页**：移除「组合订阅」卡片（组合名输入框 + 订阅链接），只保留 Token 卡片
- **预览页**：页头加组合选择器（下拉：「全部源」+ 各组合名），切换后请求 `?combined=<name>`
- `ConfigDto` 移除 `combined_name`/`subscribe_url`（链接归组合页）

## 5. 错误处理与边界

- 组合名 `[A-Za-z0-9-_]` 校验 + 唯一（冲突 400）；改名后旧名 404
- `source_ids` 引用不存在源：忽略（幂等）
- 成员为空组合：订阅 200 空输出；全部成员失败：502 附明细
- 外键级联：删组合/删源自动清理成员关系

## 6. 测试

- 新表创建 + 外键级联（删组合/删源后成员关系清理）
- 组合 CRUD：创建/列表/成员全量替换/删除/名字冲突 400/非法名字 400
- 订阅端点：按组合成员输出（single + remote 混合）、成员为空 200、名字不匹配 404、全部成员失败 502
- preview `?combined=`：过滤、不存在 404、省略参数行为不变
- `fetch_and_merge` 子集过滤；现有并发上限测试（无过滤路径）不变
- smoke.sh：新增步骤（创建组合 → 勾选 fixture 源 → `/subscribe/{name}` 输出节点）；配置页断言移除 combined_name
- 前端：`dx build` + `make smoke` + 浏览器人工核对（无测试 harness）
