# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 项目

sub-merge：订阅链接聚合与转换工具。聚合多个订阅源，实时并发拉取并合并为一个订阅，统一输出 Clash YAML / V2Ray base64 / Sing-box JSON 三种格式。小圈子自用，token 鉴权。

## 架构

- `crates/proxy-core`：纯逻辑。11 种协议解析（ss/ssr/socks5/http/vmess/vless/trojan/hysteria/hysteria2/tuic/wireguard）、3 种格式序列化（clash/v2ray/singbox）。协议/格式的单元测试与 roundtrip/proptest 测试在此。
- `crates/server`：axum 服务。SQLite（sqlx）持久化 sources/settings。路由：`/subscribe/{name}`（组合订阅输出，无鉴权）、`/admin/*`（源 CRUD/预览/配置，Bearer 鉴权）；`/healthz`；SPA 静态托管 + 前端路由回退。
- `crates/server/web`（submerge-web）：**独立于 workspace**（不加入 members，避免 dx build 与 cargo build --workspace 冲突），只能由 dx 构建。dioxus 0.8.0-alpha.1（精确版本锁定）+ WASM。样式全部在 `index.html` 内联 CSS（CSS 变量双主题，`prefers-color-scheme` 自动切换）。

## 常用命令

```bash
make build-web     # 前端 WASM：cd crates/server/web && dx build --web --release
make build-server  # cargo build --release -p server
make run           # 构建前端并启动服务（默认 :8080）
make smoke         # 端到端冒烟：构建 → 临时 server → curl 验证 SPA/静态/API

cargo test --workspace                  # 后端与纯逻辑测试（web crate 不在 workspace 内）
cd crates/server/web && dx build --web --release   # 前端唯一构建方式
```

## 强制要求

1. **每次代码修改后，必须按顺序全部通过：**

   ```bash
   cargo upgrade -i
   cargo fmt --all
   cargo clippy --workspace
   cargo test --workspace
   ```

2. **代码一律使用 Rust edition 2024**（根 workspace 已设 `edition = "2024"`，crates 经 `edition.workspace = true` 继承，web crate 为字面 `edition = "2024"`）。新增 crate 保持 2024，不得引入旧 edition。

## dioxus 0.8 alpha 已验证的坑（web crate 开发必读）

- rsx 的 if/else 分支内**不能嵌套 `rsx!` 宏调用**，须直接写元素形式：`if cond { Spinner { size: 14 } } else { "文案" }`
- `Element` 是 `Result<VNode, RenderError>`，组件空渲染用 `VNode::empty()`（不是 `return None`）
- `use_effect` 无依赖数组（每次渲染都跑），需要一次性逻辑时用信号做守卫
- `EventHandler<T> = Callback<T>`；跨渲染稳定的回调用 `use_callback` 创建，不要直接在 render 体 `EventHandler::new`
- `Signal` 可直接调用取值（`signal()`）；`Signal::set/write` 使闭包变 FnMut，绑定处可能需要 `let mut`
- svg 属性在 rsx 里用 snake_case（`view_box`、`stroke_width`）；`web-sys` 的 `setTimeout` 接受 `i32`（需 `as i32`）
- 前端 UI 无测试 harness，验证 = `dx build` + `make smoke` + 浏览器人工核对

## 环境变量与 token（详见 README.md）

| 变量 | 默认值 | 说明 |
|------|--------|------|
| PORT | 8080 | 监听端口 |
| DATABASE_PATH | ./submerge.db | SQLite 路径 |
| CONCURRENCY | 8 | 并发拉取上限 |
| TIMEOUT_SECS | 15 | 单源超时 |
| MAX_NODES | 2000 | 节点总数上限 |
| WEB_DIST | ./web/dist | 前端静态资源目录（git-ignored symlink → dx 构建产物） |
| SUB_MERGE_ADMIN_TOKEN | 随机生成 | 预设初始 admin token（仅首次初始化时生效，已部署实例不受影响） |

首次启动日志打印一次随机 token（重启不重复）；部署时用上述 `SUB_MERGE_ADMIN_TOKEN` 预设。管理接口一律 `Authorization: Bearer <admin_token>`。
