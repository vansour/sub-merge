# sub-merge

订阅链接聚合与转换工具：聚合多个订阅源，实时并发拉取并合并为一个订阅，统一输出 Clash YAML / V2Ray base64 / Sing-box JSON 三种格式。小圈子自用，token 鉴权。

## 功能

- 聚合多个订阅源（URL），并发拉取（默认并发 8，单源超时 15s），单源失败自动跳过
- 11 种协议解析：ss、ssr、socks5、http、vmess、vless、trojan、hysteria、hysteria2、tuic、wireguard
- 3 种输出格式：Clash / V2Ray / Sing-box
- 输入支持：V2Ray base64 订阅、明文 URI 列表、Clash YAML（`proxies` 段）
- 管理界面（WASM）：订阅源 CRUD、转换预览、订阅链接复制、token 轮换

## 快速开始

```bash
# 依赖：Rust 1.97+、dx（dioxus-cli 0.8.0-alpha.1）
make run          # 构建前端并启动（首次运行自动建库并生成 token）
```

默认监听 `:8080`。管理 token 的获取方式：

```bash
# 方式 1：首次启动日志直接可见（仅首次打印一次，重启不重复）
docker compose up -d --build && docker compose logs | grep token

# 方式 2：部署时用环境变量预设初始 token（可控可管理）
SUB_MERGE_ADMIN_TOKEN=your-admin-token SUB_MERGE_SUBSCRIBE_TOKEN=your-sub-token make run

# 方式 3：查库（compose bind mount 场景）
python3 -c "import sqlite3; db=sqlite3.connect('submerge-data/submerge.db'); [print(k,'=',v) for k,v in db.execute('SELECT key,value FROM settings')]"
```

浏览器打开 `http://<host>:8080`，输入管理 token 进入管理界面。

## Docker 部署

```bash
docker build -t sub-merge .
docker run -d --name sub-merge -p 8080:8080 -v submerge-data:/app/data sub-merge
```

或使用 docker compose（`compose.yaml`）：

```bash
docker compose up -d --build
```

多阶段构建：前端 WASM 与后端在同一镜像内，SQLite 数据通过 `/app/data` 卷持久化（compose 使用 bind mount `./submerge-data`；环境变量已在镜像内设默认值，无需在 compose 重复声明）。

## 环境变量

| 变量 | 默认值 | 说明 |
|------|--------|------|
| PORT | 8080 | 监听端口 |
| DATABASE_PATH | ./submerge.db | SQLite 文件路径 |
| CONCURRENCY | 8 | 并发拉取上限 |
| TIMEOUT_SECS | 15 | 单源超时 |
| MAX_NODES | 2000 | 节点总数上限 |
| WEB_DIST | ./web/dist | 前端静态资源目录 |
| SUB_MERGE_SUBSCRIBE_TOKEN | 随机生成 | 预设初始订阅 token（仅首次初始化时生效） |
| SUB_MERGE_ADMIN_TOKEN | 随机生成 | 预设初始管理 token（仅首次初始化时生效） |

预设 token 仅在数据库首次初始化时使用；已部署实例的 token 不受影响（settings 表已有值时不覆盖）。

## API

### 订阅接口

```
GET /api/subscribe?token=<订阅token>&format=clash|v2ray|singbox
```

`format` 缺省为 `clash`。全部源失败时返回 502 并附错误明细。

### 管理接口（`Authorization: Bearer <管理token>`）

| 方法 | 路径 | 说明 |
|------|------|------|
| GET/POST | /api/admin/sources | 列表 / 添加订阅源 |
| PUT/DELETE | /api/admin/sources/{id} | 更新（url/name/enabled）/ 删除 |
| POST | /api/admin/sources/{id}/refresh | 手动刷新单源 |
| GET | /api/admin/preview | 转换结果预览（节点列表 + 源错误） |
| GET/PUT | /api/admin/config | 获取配置 / 轮换 token |

订阅 token 与管理 token 独立，可分别轮换。错误统一返回 `{"error":{"code":"...","message":"..."}}`。

### 注意：V2Ray 格式的节点覆盖

`format=v2ray` 输出仅包含 ss/ssr/vmess/vless/trojan/tuic 节点；socks5、http、hysteria、hysteria2、wireguard 节点在此格式被跳过（请使用 clash 或 singbox 格式）。

## 开发

```bash
make build-web    # 构建前端 WASM（dx build --web）
make build-server # 构建后端（release）
make smoke        # 端到端冒烟测试（构建前端 → 起服务 → curl 验证）
cargo test --workspace  # 全部单元/集成测试
```

## 架构

```
Dioxus Web (WASM) 管理界面 ──▶ axum Server（/api/subscribe、/api/admin/*）
                                  │
                        service 层（并发拉取/合并/鉴权）
                                  │
                        proxy-core（纯逻辑库：解析/序列化，无 IO）
                                  │
                        SQLite (SQLx, WAL)
```

- **proxy-core**：中间模型 `ProxyNode` 覆盖全部协议，各协议 parser/serializer 围绕模型转换，三种输出格式独立模块，可脱离服务独立测试
- **server**：axum 路由、双 token 鉴权（恒定时间比较）、并发拉取（信号量限流）、SQLite 持久化、WASM 静态资源托管
- **web**：Dioxus 0.8 (WASM) 管理界面，管理 token 存 localStorage

## 测试

- proxy-core：各协议解析/序列化单元测试、roundtrip 往返测试、Clash YAML/订阅解析测试、proptest 随机输入防 panic
- server：API 集成测试（wiremock 模拟订阅源、真实 TCP 并发计数验证并发上限）、静态托管与 SPA 回退测试
- 端到端：`make smoke` 冒烟脚本
