# sub-merge 设计文档

订阅链接聚合与转换工具。聚合多个订阅源，统一转换为 Clash / V2Ray / Sing-box 等格式。

- 日期：2026-08-05
- 状态：已批准
- 技术栈：后端 axum（Rust）、前端 Dioxus（WASM）、存储 SQLite（SQLx）

## 1. 目标与范围

### 目标
- 聚合多个订阅源（URL），并发拉取后合并为一个订阅
- 输出 Clash YAML、V2Ray 订阅（base64 URI）、Sing-box JSON 三种格式
- 供小圈子（少量固定成员）使用，无需注册登录，靠 token 鉴权
- 部署为 Docker 单镜像，公网 VPS 运行

### 非目标（第一版）
- 用户体系、注册/登录、多租户
- 节点健康检测 / 测速
- 节点过滤（按地区/协议/关键字）
- 节点合并去重（保留全部，不做去重）
- 缓存与定时刷新（实时拉取）

### 关键决策（已与用户确认）
1. **方案**：自研 `proxy-core` 协议解析/转换核心（不依赖不成熟的第三方转换库）
2. **输出格式**：第一版 Clash + V2Ray + Sing-box 三种
3. **协议覆盖**：11 种 —— ss、ssr、socks5、http、vmess、vless、trojan、hysteria2、hysteria1、tuic、wireguard（shadowtls 不在第一版范围）
4. **鉴权**：订阅 token 与 管理 token 分两把，独立轮换
5. **数据获取**：实时拉取，不缓存
6. **存储**：SQLite，管理界面可编辑订阅源与 token

## 2. 架构

```
┌───────────────────────────────────────────────────────┐
│                      sub-merge                         │
│                                                       │
│  ┌─────────────┐    ┌─────────────────────────────┐   │
│  │  Dioxus Web │    │        axum Server          │   │
│  │   (WASM)    │───▶│  /api/subscribe             │   │
│  │  管理界面    │HTTP│  /api/admin/*               │   │
│  └─────────────┘    └──────────┬──────────────────┘   │
│                     service 层（fetch/解析/转换/鉴权）  │
│                     ┌──────────▼──────────────────┐   │
│                     │  proxy-core（纯逻辑库）        │   │
│                     │  中间模型 / 解析 / 序列化      │   │
│                     └──────────┬──────────────────┘   │
│                     ┌──────────▼──────────────────┐   │
│                     │  SQLite (SQLx)             │   │
│                     └─────────────────────────────┘   │
└───────────────────────────────────────────────────────┘
```

**分层职责：**

| 层 | 职责 | 依赖 |
|----|------|------|
| **`proxy-core`** | 中间模型 `ProxyNode`；各协议 Parser（解析）；各格式 Serializer（序列化） | 纯 Rust，无 IO |
| **axum 服务层** | 路由、鉴权中间件、并发抓取、数据库、错误返回 | axum, reqwest, sqlx, tower |
| **Dioxus 前端** | 管理界面（WASM），调用 `/api/admin/*` | dioxus 0.8, WASM |

`proxy-core` 是独立 workspace crate，可与主服务分离编译、独立测试。服务层依赖它。

**数据流（一次订阅请求）：**

```
客户端 GET /api/subscribe?format=clash&token=xxx
  → 校验订阅 token
  → 读 SQLite 全部 enabled 订阅源
  → 并发 HTTP 拉取各源原始内容（直连，超时 15s，并发 ≤8）
  → proxy-core 解析各源 → Vec<ProxyNode>
  → 合并全部节点
  → 按 format 序列化 → 返回响应体
```

## 3. 协议与格式

### 中间模型 `ProxyNode`

单一结构体覆盖所有协议，转换只在中间模型上做：

```rust
struct ProxyNode {
    name: String,
    kind: Protocol,          // SS SSR Socks5 Http VMess VLess Trojan Hysteria2 Hysteria1 Tuic Wireguard
    server: String,
    port: u16,
    // 认证
    crypto: Option<Crypto>,        // SS/SSR 加密方式
    password: Option<String>,
    uuid: Option<String>,          // VMess/VLess
    alter_id: Option<u16>,         // VMess
    // 传输与安全
    tls: Option<TlsSettings>,      // 启用/SNI/ALPN/证书校验/指纹
    transport: Option<Transport>,  // ws/grpc/h2/httpupgrade
    // 其他协议特有字段，用 Option 保留
}
```

每个协议一个解析器 + 序列化器，围绕同一中间模型。

### 输入解析（Parser）

| 源格式 | 内容 | 方式 |
|--------|------|------|
| V2Ray URI 订阅 | base64 多行 `vmess:// vless:// trojan:// ss:// ...` | base64 解码 → 逐行解析 |
| Clash YAML | `proxies:` 内联段 | `serde_yaml`（第一版不处理 `proxy-providers`） |
| SS/SSR 明文或 base64 | `ss://` `ssr://` 链接 | 直接解析 |

解析器按协议前缀识别，**未知协议跳过并计数**，不影响整体。

### 输出序列化（Serializer）

| 目标格式 | 产出 |
|---------|------|
| Clash YAML | `proxies:` + 默认 `proxy-groups`（自动选择/负载均衡/故障转移）+ 最小 rules |
| V2Ray 订阅 | base64 编码的 URI 文本（`vmess:// vless:// trojan:// ss://`） |
| Sing-box JSON | `outbounds:` 数组 |

每种格式是独立 serializer 模块，后续扩展加文件即可。

## 4. 数据模型与 API

### SQLite 表

```sql
CREATE TABLE sources (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  url TEXT NOT NULL,
  name TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL
);

CREATE TABLE settings (
  key TEXT PRIMARY KEY,        -- 'subscribe_token' | 'admin_token'
  value TEXT NOT NULL
);
```

启动时若 settings 无 token 则自动生成（随机 32 字节 hex）。

### API 路由

| 方法 | 路径 | 鉴权 | 说明 |
|------|------|------|------|
| GET | `/api/subscribe` | 订阅 token `?token=` | 输出聚合订阅，`?format=clash\|v2ray\|singbox`（默认 `clash`） |
| GET | `/api/admin/sources` | 管理 token (Bearer) | 列出订阅源 |
| POST | `/api/admin/sources` | 管理 token (Bearer) | 添加订阅源 |
| DELETE | `/api/admin/sources/:id` | 管理 token (Bearer) | 删除订阅源 |
| PUT | `/api/admin/sources/:id` | 管理 token (Bearer) | 启用/禁用、改 URL/名字 |
| POST | `/api/admin/sources/:id/refresh` | 管理 token (Bearer) | 手动触发刷新（重新抓取） |
| GET | `/api/admin/preview` | 管理 token (Bearer) | 转换结果预览（节点列表） |
| GET | `/api/admin/config` | 管理 token (Bearer) | 获取订阅链接 / 两个 token |
| PUT | `/api/admin/config` | 管理 token (Bearer) | 轮换任一 token |

**鉴权：**
- 两把 token 独立，可分别轮换
- 订阅接口只读，仅有输出订阅内容能力
- 管理接口走 Bearer token

## 5. 错误处理、并发与安全

### 并发拉取
- `tokio` 并发抓取，并发上限默认 8
- 单源超时（默认 15s）或失败 → 跳过该源，不拖垮整体；响应标记错误源
- 全部源失败 → 502，附错误明细

### 输入安全（解析器对不可信输入防御式处理）
- 未知协议跳过
- 非法 base64 跳过
- 恶意超大行截断（>1MB）
- 节点总数上限（默认 2000），防订阅爆炸
- 序列化前校验节点字段（端口范围、server 非空）

### 错误返回格式
统一 JSON：`{ "error": { "code": "...", "message": "..." } }`

## 6. 测试策略

| 层 | 测试内容 | 方式 |
|----|---------|------|
| proxy-core 解析 | 各协议 URI 解析正确性 | 单元测试：内置真实样本 → 断言字段 |
| proxy-core 序列化 | 三种输出结构 | 单元测试：中间模型 → 断言关键字段 |
| 往返测试 | 解析→序列化→再解析，字段不丢 | 单元测试 |
| service 层 | 拉取、合并、鉴权逻辑 | 集成测试：wiremock 模拟订阅源 |
| API 层 | 路由、鉴权、错误码 | axum 测试路由（tower::ServiceExt） |

- 测试全部走 mock，不依赖真实网络
- 可选增强：解析器 proptest 随机输入不 panic

## 7. 部署

- Docker 单镜像：axum 服务 + 前端 WASM 静态资源（由后端服务托管）
- SQLite 数据文件挂载卷持久化
- 环境变量：`PORT`（默认 8080）、`DATABASE_PATH`、`CONCURRENCY`、`TIMEOUT_SECS`、`MAX_NODES`
- 前端为纯静态 WASM，无需独立静态服务器

## 8. 目录结构（规划）

```
sub-merge/
├── Cargo.toml            # workspace
├── crates/
│   ├── proxy-core/       # 协议解析/序列化库
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── model.rs          # ProxyNode / Protocol / 配置结构
│   │   │   ├── parser/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── v2ray_uri.rs  # base64 订阅解析
│   │   │   │   ├── clash_yaml.rs
│   │   │   │   └── protocol/     # 各协议解析器
│   │   │   └── serializer/
│   │   │       ├── mod.rs
│   │   │       ├── clash.rs
│   │   │       ├── v2ray.rs
│   │   │       └── singbox.rs
│   │   └── tests/         # 解析/序列化/往返测试
│   └── server/            # axum 服务 + Dioxus 前端
│       ├── src/
│       │   ├── main.rs
│       │   ├── routes/           # API 路由
│       │   ├── service.rs        # fetch/合并/鉴权
│       │   ├── db.rs             # SQLx 访问
│       │   └── error.rs
│       └── web/                  # Dioxus 前端
└── Dockerfile
```

## 9. 已确认决策清单

| 项 | 决策 |
|----|------|
| 方案 | 自研 proxy-core，不依赖第三方转换库 |
| 使用场景 | 小圈子共用 |
| 部署形态 | 公网 VPS，Docker 镜像 |
| 节点处理 | 只要转换，不做测速/过滤/去重 |
| 存储 | SQLite（SQLx） |
| 输出格式 | Clash + V2Ray + Sing-box（第一版） |
| 协议范围 | 11 种（ss ssr socks5 http vmess vless trojan hysteria2 hysteria1 tuic wireguard，shadowtls 不在第一版范围） |
| 鉴权 | 订阅/管理 token 分两把，独立轮换 |
| 拉取策略 | 实时拉取，不缓存 |
| Dioxus | Web (WASM) 模式 |
| 管理界面 | 订阅源管理、转换结果预览、复制订阅链接 |
| 抓取 | 直连，无需代理 |
