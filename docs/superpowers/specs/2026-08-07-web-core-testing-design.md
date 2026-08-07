# web-core 前端逻辑测试设计

日期：2026-08-07
状态：已批准（待实施）

## 背景与目标

CLAUDE.md 的强制验证链 `cargo test --workspace` 只覆盖 `crates/proxy-core` 与 `crates/server`。前端 `crates/server/web`（submerge-web）刻意独立于 workspace（避免 dx build 与 cargo build --workspace 冲突），且无任何测试，前端逻辑只能靠 `dx build` + `make smoke` + 人工核对。

目标：让前端纯逻辑纳入自动测试，且**不改动验证链与 CI**——`cargo test --workspace` 直接覆盖前端逻辑。

约束（不可破坏）：

- web crate 保持 workspace 之外（文档化约束，dx build 与 cargo build 互不干扰）
- 前端 UI 无测试 harness；dioxus 0.8.0-alpha.1 无 `dx test` 子命令，crates.io 无 `dioxus-testing` crate——组件级测试工具不可用，本次不做
- 验证链 `cargo upgrade -i / fmt --all / clippy --workspace / test --workspace` 不新增命令

## 方案：新 crate `crates/web-core`，加入 workspace

把前端纯逻辑（DTO 结构体、映射函数、错误类型）迁移到新 crate `crates/web-core`，加入根 workspace members。`submerge-web` 通过 path 依赖引用。这样：

- `cargo test --workspace` / `clippy --workspace` / `fmt --all` 与 CI check job 自动覆盖前端逻辑，零改动
- 测试编译只需 serde + serde_json，秒级（已实测 web crate host 目标 `cargo check --tests` 20s，web-core 无 dioxus 依赖更快）
- dx build 构建 wasm 时连带编译 web-core（无 web 依赖，无冲突）；web crate 仍不在 workspace，原约束不变

已确认：web crate 在 host native 目标可编译测试（dioxus web feature 无 target 限制），核心可行性成立。

## crate 结构

```
crates/web-core/
├── Cargo.toml      # 依赖仅 serde(derive) + serde_json；edition.workspace = true
└── src/
    ├── lib.rs
    ├── dto.rs       # 与 server 的 API 契约 DTO
    ├── error.rs     # ApiError
    └── fmt.rs       # 映射/格式化纯函数
```

- edition 2024（workspace 继承，符合 CLAUDE.md 强制要求）
- 不依赖 dioxus / web-sys / wasm-bindgen / gloo-net
- 库 crate（lib.rs），web crate 以 path 依赖引用

## 迁移清单（web crate → web-core）

| 来源文件 | 迁出内容 | 去向 |
|---------|---------|------|
| `components/sources.rs` | `SourceDto`（id/url/name/kind/enabled/created_at，含 `#[allow(dead_code)] created_at` 契约保留注释） | dto.rs |
| `components/sources.rs`、`components/combineds.rs` | `kind_label(kind) -> &'static str`（**新抽**：两处重复的 `if kind == "single" { "单条" } else { "远程" }` 等价替换） | fmt.rs |
| `components/combineds.rs` | `CombinedDto`（id/name/created_at/source_ids） | dto.rs |
| `components/combineds.rs` | `subscribe_path(name, fmt) -> String`，返回 `/subscribe/{name}?format={fmt}`（从 rsx 内联 `format!` 抽出；base origin 拼装留在组件内） | fmt.rs |
| `components/combineds.rs` | `next_toast_id() -> u64`（NEXT_ID thread_local 分配逻辑；`push_toast` 与 Signal 逻辑留原地） | fmt.rs |
| `components/preview.rs` | `PreviewNode`（name/protocol/server/port）、`PreviewResp`（nodes/errors/total）、`proto_class(protocol) -> &'static str` | dto.rs / fmt.rs |
| `components/config.rs` | `ConfigDto`（username） | dto.rs |
| `api.rs` | `ApiError`（status/message + Display + From<String>）；`request()` 留原地改 import | error.rs |
| `components/toast.rs` | `ToastKind` 枚举 + icon/class 映射（Success→check/success、Error→alert/error、Info→config/info）；`push_toast`/`use_toast`/`schedule_timeout`/`ToastProvider`/`ToastCard` 留原地 | fmt.rs |

组件内 fetch、Signal、localStorage、setTimeout、web_sys::window 等全部留原地。

迁移原则：**纯迁移，UI 行为零变化**。kind_label 是唯一"新增"函数，只是把两处已重复的表达式等价替换；其余均原样搬移并改引用。DTO 从私有变 pub。

## 测试内容（web-core 各模块 `#[cfg(test)]`）

### fmt.rs 映射测试

- `proto_class`：ss/ssr→proto-0，vmess/vless→proto-1，trojan→proto-2，hysteria/hysteria2→proto-3，tuic→proto-4，未知协议→proto-5（fallback）
- `kind_label`：single→单条，remote→远程，未知→远程（与现有表达式一致）
- `subscribe_path`：name+三种格式（clash/v2ray/singbox）的路径拼装；含需要 URL 编码的字符（如空格、中文）时原样拼接（与现状一致，不做编码——现状就是不编码）
- ToastKind 映射：三枚举值的 icon 与 class
- `next_toast_id`：连续调用单调递增

### error.rs 测试

- Display 输出 message
- From<String> 转换
- status 区分：Some(401) 与其他 status/None（分类字段直接可用，无额外逻辑；测试锁定字段语义防回归）

### dto.rs 契约测试（fixture 取自 server routes 源码实际输出形状）

- `/admin/sources` → `[{"id":1,"url":"https://example.com/sub","name":"机场A","kind":"remote","enabled":true,"created_at":"2026-08-07 12:00:00"}]`：反序列化成功、字段完整
- `/admin/combineds` → `[{"id":1,"name":"home","created_at":"2026-08-07 12:00:00","source_ids":[1,2]}]`：反序列化成功、source_ids 顺序与内容
- `/admin/preview` → `{"nodes":[{"name":"节点1","protocol":"vmess","server":"1.2.3.4","port":443}],"errors":["源A: 超时"],"total":1}`：nodes/errors/total 完整
- `/admin/config` → `{"username":"admin"}`：反序列化成功（config_dto_parses 实测 fixture）
- 未知字段忽略：在 fixtures 中混入未知键（如 `"extra":"x"`），断言反序列化不受影响（serde 默认行为，锁定契约宽松性）

## 集成

1. 根 `Cargo.toml`：`members = ["crates/proxy-core", "crates/server", "crates/web-core"]`
2. `crates/server/web/Cargo.toml`：添加 `submerge-web-core = { path = "../web-core" }`
3. web crate 各组件改为 `use submerge_web_core::...`
4. 验证链零改动：`cargo test --workspace` / `cargo clippy --workspace` / `cargo fmt --all` 自动覆盖 web-core；CI check job 自动覆盖
5. CLAUDE.md 架构节补一句 web-core 说明（可选，验证链不变）

## 风险与取舍

- **跨 crate 引用**：DTO 定义在 web-core，组件 use 引入，引用关系简单
- **fixture 一致性**：契约测试 fixture 手工从 server routes 源码取样；server 字段变更时测试失败即提醒同步（这正是契约测试的目的）
- **dx build 行为**：web crate 构建 wasm 时连带编译 web-core，无 web 依赖不引入冲突；web crate 仍在 workspace 外，原约束不变
- **不做**：组件渲染测试（dioxus 0.8 alpha 无官方 harness，crates.io 无 dioxus-testing）、浏览器 E2E（本次用户明确排除，留待后续）

## 完成判定

- [ ] `cargo test --workspace` 通过且包含 web-core 测试（`cargo test -p web-core` 可见测试清单）
- [ ] `cargo clippy --workspace`、`cargo fmt --all` 通过
- [ ] `dx build --web --release` 成功（web crate 依赖路径正确）
- [ ] `make smoke` 通过（前端行为无回归）
- [ ] CLAUDE.md 验证链未增删命令
