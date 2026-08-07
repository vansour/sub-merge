# 前端导航重构设计：层级菜单 + 预览拆分

日期：2026-08-07
状态：已批准（待实施）

## 背景与目标

现状侧边栏是 5 个平铺一级菜单（概览/订阅源/组合订阅/预览/配置）。用户要求重构：

1. 删除概览页面
2. 一级菜单更名为「订阅管理」，下设二级菜单「单条订阅」与「组合订阅」
3. 「单条订阅」下设三级菜单「本地订阅」与「远程订阅」
4. 预览功能拆分：远程订阅、本地订阅、组合订阅三页各自内嵌预览区
5. 配置页保留为一级菜单
6. 分组（「订阅管理」「单条订阅」）可折叠，选中叶子时祖先分组自动展开

## 导航结构（目标态）

```
订阅管理 ▾          （一级分组，可折叠）
├── 单条订阅 ▾      （二级分组，可折叠）
│   ├── 本地订阅     （叶子：kind=single 源 CRUD + 预览区）
│   └── 远程订阅     （叶子：kind=remote 源 CRUD + 预览区）
└── 组合订阅         （叶子：组合 CRUD + 预览区）
配置                 （一级叶子，保留现状）
```

- 叶子页内部索引：`0=本地订阅 1=远程订阅 2=组合订阅 3=配置`（MainShell 的 usize tab 模式保留，切换/预载编排不重写）
- 分组展开状态：`use_signal(HashSet<&'static str>)` 记录展开的分组名，默认展开「订阅管理」+「单条订阅」

## 组件与交互

### NavItem 拆分（main.rs）

- `NavGroup(name, label, icon, open, on_toggle, children)`：分组项，折叠箭头（open 时旋转 90°），点击切换展开；children 渲染在其下（缩进）
- `NavLeaf(...)`：现有 NavItem 改名（active/loading/onnav 不变）

### 交互规则

- 点击分组标题 → 切换展开/收起
- 点击叶子 → 走现有「先载内容再切页」流程（pending 转圈、旧页保持、缓存秒开均不变）
- 选中叶子时其所有祖先分组强制展开（切页时把祖先分组加入展开集合）
- 窄屏（≤768px）顶栏模式：分组标题作为按钮展开/收起子菜单（纵向堆叠），保持隐藏文字只显图标

### CSS（index.html）

新增 `.nav-group`（分组标题行，hover 态同 nav-item）、`.nav-group-children`（缩进 12px 容器）、`.nav-chevron`（折叠箭头）、叶子缩进层级（二级叶子缩进 1 层、三级叶子缩进 2 层）。沿用现有 CSS 变量双主题。

## 后端：/admin/preview 支持按 kind 过滤

`preview.rs` 的 `PreviewQuery` 加可选参数 `kind: Option<String>`（值限 `single`/`remote`）：
- `kind` 有值时：查 `enabled = 1 AND kind = ?` 的源 id 列表，传给 `fetch_and_merge(Some(&ids))`（复用现有成员子集路径，不改 service 核心）
- `kind` 与 `combined` 同时出现 → 400（互斥）
- 非法 kind 值 → 400

## 页面内容

### 共享预览组件 PreviewSection（原 preview.rs 渲染逻辑抽取）

```
PreviewSection {
    token: Signal<Option<String>>,
    kind: Option<&'static str>,      // Some("single")/Some("remote")
    combined: Option<String>,        // 组合名（二选一语义由调用方保证）
}
```

内部：本地状态（data/loading/error 信号）+ 刷新按钮 + 节点表 + 源错误卡。

| 页面 | PreviewSection 参数 | 触发 |
|------|---------------------|------|
| 本地订阅 | `kind: Some("single")` | 挂载 + 手动刷新 |
| 远程订阅 | `kind: Some("remote")` | 挂载 + 手动刷新 |
| 组合订阅 | `combined: Some(选中组合名)` | 下拉切换 + 手动刷新 |

组合页预览区上方有组合下拉（选项来自 combineds 单元）；未选组合时显示空态提示。

### 各页面

**本地订阅页**（sources.rs 改造，接收 kind 参数）：添加表单 kind 固定 single（类型下拉移除）；列表只显示 kind=single 源（从 sources 单元过滤）；CRUD/启停/单行刷新不变；列表上方计数徽章；下方 PreviewSection(kind=single)。

**远程订阅页**：同上，kind 固定 remote。

**组合订阅页**（combineds.rs 改造）：组合列表 + 新建/编辑弹窗不变；成员勾选维持现状（全部源可选，single 源可入组合）；下方组合下拉 + PreviewSection(combined)。

### 删除项

- `components/overview.rs`（连同 StatCard）删除
- `components/preview.rs` 页面外壳删除，渲染逻辑抽为 PreviewSection
- 原概览统计卡片（源总数/启用/失败源数/节点总数）职责下沉：计数徽章在列表上方，失败源信息在预览区源错误展示

## 数据层（data.rs）

- 删除 `preview` 单元（概览删除后无「全部源」预览消费方；各页预览为页面本地状态）；`UnitKey::Preview`、`fetch_preview`、required_units 的 preview 引用一并删除
- 单元归属：本地订阅 `[Sources]`；远程订阅 `[Sources]`；组合订阅 `[Combineds, Sources]`；配置 `[Config]`
- 预载/秒开/单飞/错误保留语义不变（MainShell 的 required_units 表更新）

## 文件清单

| 文件 | 动作 |
|------|------|
| `crates/server/src/routes/preview.rs` | 加 `?kind=` 参数（互斥校验） |
| `crates/server/web/src/components/overview.rs` | 删除 |
| `crates/server/web/src/components/preview.rs` | 渲染逻辑抽为 PreviewSection，迁至新建 `components/preview_section.rs`（三页共用独立组件） |
| `crates/server/web/src/components/sources.rs` | 改造：组件参数化（`kind: &'static str` prop），MainShell 两处实例化（本地/远程） |
| `crates/server/web/src/components/combineds.rs` | 追加 PreviewSection + 组合下拉 |
| `crates/server/web/src/main.rs` | MainShell 层级导航（NavGroup/NavLeaf）、展开状态、默认 tab=本地订阅 |
| `crates/server/web/src/data.rs` | 删 preview 单元 |
| `crates/server/web/index.html` | 分组/缩进/箭头 CSS |
| `scripts/ui-check.py` | 场景适配（见下） |

## ui-check.py 场景适配

| 场景 | 现状依赖 | 处理 |
|------|---------|------|
| `nav_preload` | 概览统计、概览回访、配置慢路径 | 改写：初始页=本地订阅；断言对象=本地订阅列表/预览区；新增分组折叠/展开断言 |
| `first_load_failure` | 概览需 sources+preview 两单元 | 改写：拦截 `/admin/preview*`，初始页本地订阅（sources 单元 + 预览区失败，单元失败仍提交切换语义等价） |
| `sources_crud` | 概览统计 +1 | 改写：本地订阅页计数徽章 +1 |
| `preview_filter` | 预览页下拉 | 改写：组合订阅页预览下拉切换 |
| `refresh_failure` | 概览统计/摘要行 | 改写：本地订阅页列表行数 + 预览区错误 |
| `combineds`、`config_password` | 组合/配置导航 | 点击目标改新菜单路径，断言基本不变 |

场景语义（旧页保持/转圈/错误可见/刷新恢复）全部保留，仅断言目标换新页面。

## 验证方式

1. cargo 门禁：`cargo upgrade -i` → `cargo fmt --all` → `cargo clippy --workspace` → `cargo test --workspace`（后端 preview kind 参数需要集成测试：`?kind=single` 只含 single 源、`?kind=remote` 只含 remote 源、kind+combined 互斥 400、非法 kind 400）
2. `dx build --web --release --debug-symbols false` 0 警告
3. `make smoke` 9/9（API 层不受影响）
4. ui-check.py：本环境无 chrome-headless-shell，语法 + 代码审读验证，运行期留待有浏览器环境
5. 浏览器人工核对：折叠/展开交互、选中祖先自动展开、三页预览区数据正确（kind 过滤）、组合页下拉切换、窄屏顶栏分组

## 不做的事（YAGNI）

- 不保留「全部源」预览视图（概览删除后无入口）
- 不做导航分组状态的持久化（刷新回到默认展开）
- 不重构 DataStore 为按 kind 分键的预览缓存（各页本地状态已够）
- 不动 smoke.sh（API 层无变化）
