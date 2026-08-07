# sub-merge

订阅链接聚合与转换工具：聚合多个订阅源，实时并发拉取并合并为一个订阅，统一输出 Clash YAML / V2Ray base64 / Sing-box JSON 三种格式。小圈子自用，用户名+密码鉴权。

## 功能

- 聚合多个订阅源（URL），并发拉取（默认并发 8，单源超时 15s），单源失败自动跳过
- 两种源类型：单条节点（URI 直接解析，不拉网络）与远程订阅（订阅链接，拉取后解析）；组合订阅使用命名路径输出（无 token 鉴权）
- 11 种协议解析：ss、ssr、socks5、http、vmess、vless、trojan、hysteria、hysteria2、tuic、wireguard
- 3 种输出格式：Clash / V2Ray / Sing-box
- 输入支持：V2Ray base64 订阅、明文 URI 列表、Clash YAML（`proxies` 段）
- 管理界面（WASM）：订阅源 CRUD、转换预览、订阅链接复制、账号管理（修改密码）
- 多个组合订阅：每组合从源中勾选成员（多对多），独立命名订阅链接（/subscribe/{name}），组合订阅管理在侧边栏「组合订阅」页

## 快速开始

```bash
# 依赖：Rust 1.97+、dx（dioxus-cli 0.8.0-alpha.1）
make run          # 构建前端并启动（首次运行登录页引导创建管理员）
```

默认监听 `:8080`。

首次访问浏览器打开 `http://<host>:8080`，登录页会引导创建管理员（用户名+密码）。
创建完成后即可登录进入管理界面；之后重启不再出现创建表单。

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

## API

### 订阅接口

```
GET /subscribe/{name}?format=clash|v2ray|singbox
```

`{name}` 为组合订阅名（在 `/admin/combineds` 中定义）；`format` 缺省为 `clash`，无鉴权。名字不匹配返回 404；组合无成员时输出空配置（200）；全部成员源失败时返回 502 并附错误明细。

### 管理接口（`Authorization: Bearer <会话 token>`，登录后获得）

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | /admin/setup-status | 初始化状态（`needs_setup`） |
| POST | /admin/setup | 首次创建管理员（仅未初始化时可用） |
| POST | /admin/login | 登录，返回会话 token |
| POST | /admin/logout | 注销当前会话（幂等，204） |
| GET/POST | /admin/sources | 列表 / 添加订阅源（`kind`: `single` 单条节点 \| `remote` 远程订阅，缺省 remote） |
| PUT/DELETE | /admin/sources/{id} | 更新（url/name/kind/enabled）/ 删除 |
| POST | /admin/sources/{id}/refresh | 手动刷新单源 |
| GET | /admin/preview | 转换结果预览（节点列表 + 源错误；`?combined=<name>` 按组合成员过滤） |
| GET/POST | /admin/combineds | 组合订阅列表 / 创建（`source_ids` 成员源数组） |
| PUT/DELETE | /admin/combineds/{id} | 更新（名字/成员全量替换）/ 删除 |
| GET/PUT | /admin/config | 获取配置（用户名）/ 修改密码 |

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
Dioxus Web (WASM) 管理界面 ──▶ axum Server（/subscribe/{name}、/admin/*）
                                  │
                        service 层（并发拉取/合并/鉴权）
                                  │
                        proxy-core（纯逻辑库：解析/序列化，无 IO）
                                  │
                        SQLite (SQLx, WAL)
```

- **proxy-core**：中间模型 `ProxyNode` 覆盖全部协议，各协议 parser/serializer 围绕模型转换，三种输出格式独立模块，可脱离服务独立测试
- **server**：axum 路由、用户名+密码登录与会话 token 鉴权（argon2 哈希、sha256 会话查表）、并发拉取（信号量限流）、SQLite 持久化、WASM 静态资源托管
- **web**：Dioxus 0.8 (WASM) 管理界面，会话 token 存 localStorage

## 测试

- proxy-core：各协议解析/序列化单元测试、roundtrip 往返测试、Clash YAML/订阅解析测试、proptest 随机输入防 panic
- server：API 集成测试（wiremock 模拟订阅源、真实 TCP 并发计数验证并发上限）、静态托管与 SPA 回退测试
- 端到端：`make smoke` 冒烟脚本
