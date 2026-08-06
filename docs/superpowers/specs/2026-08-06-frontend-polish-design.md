# sub-merge 前端美化设计（双主题 + 专业管理面板）

- 日期：2026-08-06
- 状态：已批准（用户确认设计各节）
- 范围：纯前端（crates/server/web），零后端改动，零新增依赖

## 1. 背景与目标

当前前端为 Dioxus 0.8 (WASM) + index.html 内联 CSS 的简朴样式：单一浅色主题、纯文字 Tab 导航、无图标/动效/反馈态。目标是把管理界面提升为**专业的现代化管理面板**。

### 已确认的需求
- **深/浅色双主题**，`prefers-color-scheme` 自动切换
- **保持自包含**：不引入任何 CDN / 外部库，图标用内联 SVG 手写
- **统计概览卡片**：源总数 / 启用中 / 节点总数 / 失败源数
- **交互反馈完善**：Toast、删除/轮换确认弹窗、按钮 loading、空状态
- **SVG 图标系统**：自建 icon.rs 组件
- **响应式布局**：桌面侧边栏，窄屏收成顶部横条
- **侧边栏导航**：概览 / 订阅源 / 预览 / 配置

### 非目标
- 后端 API 不改动（统计数据由现有 `/api/admin/sources` + `/api/admin/preview` 客户端聚合）
- 不引入 CSS 框架、图标库、字体 CDN
- 不做前端路由（仍是单页 Tab 切换）
- 不改动现有 API 契约（SourceDto / ConfigDto 字段不变）

## 2. 架构与数据流

无架构变化。仍是：Dioxus 组件树（App → Login | MainShell → 4 个页面组件）+ gloo-net fetch 调用现有后端 API。

新增的全局状态：
- **ToastContext**：`Signal<Vec<Toast>>`，任意组件可 push 成功/错误/信息 Toast，右上角堆叠，4s 自动消失
- 确认弹窗为局部组件（使用方组件持有 `Signal<Option<ConfirmRequest>>`）

## 3. 设计语言（Design Tokens）

全部定义在 `index.html` 的 `<style>` 中，用 CSS 自定义属性（`:root` + `@media (prefers-color-scheme: dark)`），类名沿用现有结构（container/card/button 等），不引入 CSS 预处理器。

| Token | 浅色 | 深色 |
|-------|------|------|
| --bg | #f6f7f9 | #0f1115 |
| --bg-elevated（侧边栏） | #ffffff | #13151b |
| --card | #ffffff | #17191f |
| --text | #17181c | #e6e8ee |
| --text-secondary | #6b7280 | #9aa1ad |
| --text-tertiary（占位/禁用） | #9ca3af | #6b7280 |
| --border | #e5e7eb | #262a33 |
| --accent | #2563eb | #38bdf8 |
| --accent-hover | #1d4ed8 | #7dd3fc |
| --accent-soft（强调色浅底） | #eff6ff | #10233f |
| --success | #16a34a | #4ade80 |
| --success-soft | #f0fdf4 | #12271a |
| --danger | #dc2626 | #f87171 |
| --danger-soft | #fef2f2 | #2a1517 |
| --warning | #d97706 | #fbbf24 |
| --warning-soft | #fffbeb | #2a2110 |
| --radius-card | 10px | 同 |
| --radius-control | 6px | 同 |
| --shadow-card | 0 1px 3px rgba(0,0,0,.08) | 0 1px 3px rgba(0,0,0,.4) |
| --font-mono | ui-monospace, SFMono-Regular, Consolas, monospace | 同 |

- 字号层级：页面标题 20px/600、小节标题 15px/600、正文 14px、表格 13px、徽章/说明 12px
- 颜色/背景过渡 150ms，主题切换平滑
- 所有交互元素（按钮/输入/导航项）有 `:focus-visible` 焦点环（2px accent 半透明）

## 4. 布局与导航

### 桌面 ≥768px
- 左侧固定 220px 侧边栏（`--bg-elevated` + 右边框）
  - 顶部：logo 图标 + "sub-merge" 字标（16px/600）
  - 中部：4 个导航项（图标 16px + 文字），当前项 accent 色文字 + accent-soft 圆角底；hover 有浅色底
  - 底部：版本号（"v0.1.0" 次要文字）+ 退出登录按钮（ghost 样式）
- 右侧主内容区：max-width 960px，左右 padding 24px

### 移动 <768px
- 侧边栏收成顶部横条：左侧 logo 字标，右侧 4 个图标导航（当前项 accent 色）+ 退出图标
- 内容区 padding 16px
- 表格外包 `overflow-x: auto` 容器

## 5. 组件体系

### 5.1 按钮（class: btn / btn-primary / btn-secondary / btn-danger / btn-ghost）
- primary：accent 底白字；secondary：中性灰底（深色模式用 --border 底）；danger：红底；ghost：透明底 + 边框文字
- 尺寸统一 8px 14px 内边距 + 6px 圆角；disabled 降低不透明度
- loading 态：按钮内嵌 14px SVG spinner（组件内提供 `Spinner`）

### 5.2 输入框与表单
- label 元素（12px 次要文字）+ input（13px，--card 底，focus 边框 accent + 2px 光晕）
- 错误信息 12px danger 色
- 登录页输入为 `type="password"`，支持 Enter 提交（`onkeydown` 判断 Enter）

### 5.3 表格
- 表头：12px 次要文字；行高 44px；行 hover 浅色底；分隔线 --border
- 单元格：名称列粗体、URL 列 mono 13px、超长 `max-width` + `text-overflow: ellipsis` + `title` 属性
- 徽章 Badge：12px 圆角胶囊，on=绿底绿字 / off=中性灰底灰字
- 协议徽章（预览页）：按协议类型配色（vless/trojan 等共享一组 6 色循环，浅色用柔和底 + 深色字，深色模式提亮）

### 5.4 统计卡片（概览页）
- 4 张并排（grid：桌面 4 列，平板 2 列，手机 1 列），每张：16px 图标（accent-soft 圆形底）+ 28px/600 数字 + 12px 次要文字说明
- 失败源卡片数字用 danger 色（=0 时显示 success 色）

### 5.5 Toast（toast.rs）
- 全局 `ToastProvider` 组件挂在 MainShell 根部，`use_context_provider`
- Toast 结构：{ id, kind: success|error|info, text }，右上角固定定位，堆叠，入场动画（translateY + fade，150ms）
- 4s 自动消失（spawn 定时）+ 手动关闭按钮（×）
- 颜色：success 绿字绿底 / error 红字红底 / info 次要色

### 5.6 确认弹窗（confirm.rs）
- 组件 `ConfirmDialog { request: Signal<Option<ConfirmRequest>> }`，请求含 title/message/confirm_text/危险标记
- 渲染：全屏半透明覆盖层（点击关闭）+ 居中卡片（标题 + 文案 + [取消/ghost] [确认/危险红]）
- 用于：删除订阅源、轮换管理 token（轮换订阅 token 为普通确认）

### 5.7 空状态与加载态
- 空状态：居中图标（48px 淡色）+ 主文案 14px + 辅助文案 12px 次要色 + 可选操作按钮
- 数据加载：页面顶部细进度条（可选）或按钮 spinner；简单方案：加载中显示次要色"加载中…"占位 + 按钮 loading

### 5.8 SVG 图标（icon.rs）
- `pub fn Icon(name: &str, size: u32, class: &str)` 组件，内部 match 返回 24×24 viewBox 的 `<svg><path/></svg>`，`fill="currentColor"`
- 图标集（16 个）：logo（菱形叠加）、overview（仪表盘）、sources（链节）、preview（眼睛）、config（齿轮）、logout（门+箭头）、refresh（环形箭头）、copy（双矩形）、trash、plus、check、x、alert-triangle、spinner（单独动画组件，stroke 弧线）、chevron-right

## 6. 页面设计

### 6.1 登录页（login.rs 重写）
- 全屏居中（flex + min-height 100vh）：深色模式下也居中卡片
- 卡片内容：logo 图标（40px accent 色）+ "sub-merge"（22px/700）+ 一行说明（13px 次要色）+ token 密码输入 + 错误提示 + 登录按钮（全宽，loading 态）
- Enter 提交

### 6.2 概览页（新 overview.rs）
- 加载：并行请求 sources + preview（`futures::join!` 或顺序 await）
- 4 张统计卡片：订阅源总数 / 启用中 / 节点总数 / 失败源数（= errors.len()）
- 下方两栏（桌面 grid 2 列）：
  - "订阅源" 摘要卡：启用/停用源列表（名称 + 徽章），点击可跳转到订阅源 tab（传信号）
  - "最近错误" 卡：preview.errors 逐条显示（warning 样式）；无错误显示"全部正常"空状态
- 右上角刷新按钮（loading 态）

### 6.3 订阅源页（sources.rs 改造）
- 顶部"添加订阅源"卡片：label + URL 输入（mono）+ label + 名称输入 + 添加按钮（loading）
- 列表卡片：表格；操作列按钮换图标 + 文字（紧凑：启用/停用、刷新、删除）
- 删除弹 ConfirmDialog；成功/失败 Toast
- 空状态：无源时显示引导（"添加第一个订阅源"按钮滚动聚焦到表单）
- 刷新单源：成功（`ok: true`）Toast"已刷新 N 个节点"，失败（`ok: false`）Toast 显示 reason（当前前端忽略该返回体，本次利用）

### 6.4 预览页（preview.rs 改造）
- 头部：标题 + "共 N 个节点" 计数徽章 + 刷新按钮（loading）
- 表格加协议徽章列配色；节点多时行 hover 高亮
- 错误区改为 warning-soft 卡片，图标 + 逐条显示
- 空数据空状态

### 6.5 配置页（config.rs 改造）
- "订阅链接"卡片：3 行链接行（格式名徽章 + mono 链接 ellipsis + 复制按钮）；复制成功按钮短暂变"✓ 已复制"（2s 后还原）+ Toast
- "Token"卡片：订阅/管理 token 分开两行（mono 显示，管理 token 用掩码展示默认 ····，提供"显示"切换）
- 轮换按钮：管理 token 红 danger + ConfirmDialog；订阅 token secondary + ConfirmDialog
- 轮换成功 Toast + 自动更新会话（现有逻辑保留）

## 7. 文件改动清单

| 文件 | 改动 |
|------|------|
| crates/server/web/index.html | CSS 全面重写（双主题 tokens + 全部组件样式） |
| crates/server/web/src/components/icon.rs | 新增：SVG 图标组件 |
| crates/server/web/src/components/toast.rs | 新增：ToastProvider + use_toast |
| crates/server/web/src/components/confirm.rs | 新增：ConfirmDialog |
| crates/server/web/src/components/overview.rs | 新增：概览页 |
| crates/server/web/src/main.rs | 侧边栏布局 + ToastProvider + tab 切换传信号给 overview |
| crates/server/web/src/components/login.rs | 重写样式与交互 |
| crates/server/web/src/components/sources.rs | 样式/确认/Toast/空状态/表单 loading |
| crates/server/web/src/components/preview.rs | 样式/协议徽章/计数/错误卡片 |
| crates/server/web/src/components/config.rs | 样式/链接卡片/掩码/确认/Toast |

后端（crates/server）、proxy-core 零改动。Cargo.toml 零新增依赖。

## 8. 验证

1. `dx build --web --release`（crates/server/web 下）通过
2. `make run` 启动后浏览器人工核对：
   - 5 个页面渲染与交互（登录 → 概览/订阅源/预览/配置）
   - 浅色/深色两套主题（系统切换后无样式错乱）
   - 响应式：桌面侧边栏、<768px 顶栏
   - Toast / 确认弹窗 / 空状态 / 按钮 loading 各触发一次
3. 不回归：源 CRUD、预览刷新、复制链接、token 轮换（含 admin token 轮换后会话同步）
