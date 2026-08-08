// crates/server/web/src/main.rs
mod api;
mod components;
mod data;

use crate::api::request;
use crate::data::DataStore;
use components::clash_config::ClashConfig;
use components::combineds::Combineds;
use components::config::Config;
use components::icon::{Spinner, icon};
use components::login::{Login, clear_token, read_token, write_token};
use components::sources::Sources;
use components::toast::ToastProvider;
use dioxus::prelude::*;
use wasm_bindgen::prelude::*; // Closure/JsCast（toast.rs 同款用法）

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    let mut token = use_signal(|| read_token());
    // 启动时校验一次本地存储的 token：有效则进入主界面，仅 401（token 失效）时清除回登录页。
    // 仅 401（会话失效/被删除）时清除本地会话回登录页；网络故障/5xx 等瞬时错误
    // 保留会话，避免误登出。
    let checking = use_signal(|| true);
    use_future(move || {
        let mut token = token;
        let mut checking = checking;
        async move {
            if let Some(t) = token() {
                match request("GET", "/admin/config", None, Some(&t)).await {
                    Ok(_) => {} // 有效，保留
                    Err(e) => {
                        if e.status == Some(401) {
                            clear_token();
                            token.set(None);
                        }
                    }
                }
            }
            checking.set(false);
        }
    });
    rsx! {
        match *checking.read() {
            true => rsx! {
                div { class: "login-wrap",
                    div { class: "login-card",
                        div { class: "login-logo", Spinner { size: 40 } }
                    }
                }
            },
            false => match token().as_deref() {
                Some(_) => rsx! {
                    // ToastProvider 必须包在需要 toast 的子树外层（context 只向下传播）。
                    ToastProvider {
                        MainShell { token }
                    }
                },
                None => rsx! {
                    Login {
                        on_login: move |t: String| {
                            write_token(&t);
                            token.set(Some(t));
                        },
                    }
                },
            },
        }
    }
}

// 主壳:侧边栏导航(<900px 汉堡抽屉,见 CSS @media 900px;桌面常驻侧栏)。
// 切换策略:目标 tab 所需数据单元全部就绪才切换(旧页保持 + 菜单项转圈);
// 已加载单元缓存,回访秒开。数据层见 data.rs 的 DataStore。
#[component]
fn MainShell(token: Signal<Option<String>>) -> Element {
    let mut tab = use_signal(|| 0usize);
    let mut pending = use_signal(|| None::<usize>);
    let data = DataStore::provide(token);

    // 叶子索引：0=本地订阅 1=远程订阅 2=组合订阅 3=Clash 配置 4=配置
    // 分组名："subs"（订阅管理）"single"（单条订阅）
    // 展开状态默认值由下方 is_mobile 跟随 effect 在挂载时写入（collapse mobile / expand desktop）。
    let mut open_groups = use_signal(std::collections::HashSet::<&'static str>::new);

    // 移动端抽屉开关
    let mut menu_open = use_signal(|| false);
    // 断点判定：跨断点动态跟随（matchMedia change 事件，跨断点单次触发零防抖）。
    // 挂载时读一次初值；注册监听用守卫信号保证只注册一次（use_effect 无依赖数组）。
    let is_mobile = use_signal(|| {
        web_sys::window()
            .and_then(|w| w.match_media("(max-width: 900px)").ok().flatten())
            .map(|m| m.matches())
            .unwrap_or(false)
    });
    let mut mq_inited = use_signal(|| false);
    use_effect(move || {
        if mq_inited() {
            return;
        }
        mq_inited.set(true);
        if let Some(mql) =
            web_sys::window().and_then(|w| w.match_media("(max-width: 900px)").ok().flatten())
        {
            // 回调内写信号：捕获 Copy 的 Signal 句柄并 let mut 绑定（Signal::set 需可变）。
            let mut s = is_mobile;
            let cb = Closure::wrap(Box::new(move |_e: web_sys::Event| {
                let now = web_sys::window()
                    .and_then(|w| w.match_media("(max-width: 900px)").ok().flatten())
                    .map(|m| m.matches())
                    .unwrap_or(false);
                s.set(now);
            }));
            // 页面生命周期监听：forget 保活（toast.rs schedule_timeout 同模式）。
            // web-sys 0.3：addEventListener 在 EventTarget 上，签名收 &js_sys::Function
            // （MediaQueryList 经 wasm-bindgen extends 生成 AsRef/JsCast，unchecked_ref 转 EventTarget）。
            let et: &web_sys::EventTarget = mql.unchecked_ref();
            let _ = et.add_event_listener_with_callback("change", cb.as_ref().unchecked_ref());
            cb.forget();
        }
    });
    // 跨断点跟随：分组默认折叠状态切换（手动开关在断点内仍有效——
    // 本 effect 只读 is_mobile，不读 open_groups，故不会覆盖手动开关）
    use_effect(move || {
        if *is_mobile.read() {
            open_groups.write().clear();
        } else {
            open_groups.write().extend(["subs", "single"]);
        }
    });

    let mut go = move |i: usize| {
        if *tab.read() == i {
            return;
        }
        if *pending.read() == Some(i) {
            return;
        }
        // 选中叶子时祖先分组强制展开
        open_groups.write().insert("subs");
        menu_open.set(false); // 选中导航项关闭移动端抽屉
        if i < 2 {
            open_groups.write().insert("single");
        }
        if data.all_ready(i) {
            pending.set(None);
            tab.set(i);
        } else {
            data.ensure_loaded(i);
            pending.set(Some(i));
        }
    };

    // 当前页单元未加载(Idle)则自动预载;Error 不自动重试(由页内刷新按钮负责),避免死循环。
    if pending.read().is_none() && data.any_idle(*tab.read()) {
        data.ensure_loaded(*tab.read());
        pending.set(Some(*tab.read()));
    }

    // 加载完成提交切换:目标 tab 全部非 Loading(Ready/Error)时落定。
    use_effect(move || {
        let p = pending.read().clone();
        if let Some(p) = p {
            if data.all_finished(p) {
                tab.set(p);
                pending.set(None);
            }
        }
    });

    // Esc 关抽屉 + 打开时锁背景滚动（body overflow；关闭恢复）。
    // 注册监听用守卫信号保证只注册一次；keydown 在 window 上（转 EventTarget，同 mq 模式）。
    let mut esc_inited = use_signal(|| false);
    use_effect(move || {
        if esc_inited() {
            return;
        }
        esc_inited.set(true);
        let mut menu = menu_open;
        let cb = Closure::wrap(Box::new(move |e: web_sys::KeyboardEvent| {
            if e.key() == "Escape" && *menu.read() {
                menu.set(false);
            }
        }));
        if let Some(w) = web_sys::window() {
            let et: &web_sys::EventTarget = w.unchecked_ref();
            let _ = et.add_event_listener_with_callback("keydown", cb.as_ref().unchecked_ref());
        }
        cb.forget();
    });
    use_effect(move || {
        let open = *menu_open.read();
        if let Some(body) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.body())
        {
            if open {
                let _ = body.style().set_property("overflow", "hidden");
            } else {
                let _ = body.style().remove_property("overflow");
            }
        }
    });

    // pending 等于当前 tab 时(首次登录的默认页)无旧页可保持,渲染全页 loading。
    let content: Element = if *pending.read() == Some(*tab.read()) {
        rsx! { div { class: "page-loading", Spinner { size: 28 } } }
    } else {
        match *tab.read() {
            0 => rsx! { Sources { token, kind: "single" } },
            1 => rsx! { Sources { token, kind: "remote" } },
            2 => rsx! { Combineds { token } },
            3 => rsx! { ClashConfig { token } },
            _ => rsx! { Config { token } },
        }
    };

    rsx! {
        div { class: "app-shell",
            div { class: "topbar",
                button { class: "btn btn-ghost btn-sm topbar-menu", onclick: move |_| menu_open.set(true), {icon("menu", 20)} }
                div { class: "topbar-brand",
                    {icon("logo", 20)}
                    span { "sub-merge" }
                }
            }
            if *menu_open.read() {
                div { class: "drawer-overlay", onclick: move |_| menu_open.set(false) }
            }
            aside { class: if *menu_open.read() { "sidebar open" } else { "sidebar" },
                div { class: "sidebar-brand",
                    {icon("logo", 22)}
                    span { "sub-merge" }
                }
                nav { class: "nav",
                    NavGroup {
                        label: "订阅管理", icon_name: "sources", open: open_groups.read().contains("subs"),
                        on_toggle: move |_| {
                            let mut g = open_groups.write();
                            if g.contains("subs") { g.remove("subs"); } else { g.insert("subs"); }
                        },
                        NavGroup {
                            label: "单条订阅", icon_name: "combineds", open: open_groups.read().contains("single"),
                            on_toggle: move |_| {
                                let mut g = open_groups.write();
                                if g.contains("single") { g.remove("single"); } else { g.insert("single"); }
                            },
                            NavLeaf { name: "local", label: "本地订阅", active: *tab.read() == 0, loading: *pending.read() == Some(0), onnav: move |_| go(0) }
                            NavLeaf { name: "remote", label: "远程订阅", active: *tab.read() == 1, loading: *pending.read() == Some(1), onnav: move |_| go(1) }
                        }
                        NavLeaf { name: "combineds", label: "组合订阅", active: *tab.read() == 2, loading: *pending.read() == Some(2), onnav: move |_| go(2) }
                    }
                    NavLeaf { name: "clash", label: "Clash 配置", active: *tab.read() == 3, loading: *pending.read() == Some(3), onnav: move |_| go(3) }
                    NavLeaf { name: "config", label: "配置", active: *tab.read() == 4, loading: *pending.read() == Some(4), onnav: move |_| go(4) }
                }
                div { class: "sidebar-footer",
                    span { class: "sidebar-version", "v0.0.1" }
                    button { class: "btn btn-ghost btn-sm", onclick: move |_| {
                        let t = token.read().clone();
                        let mut token = token.clone();
                        spawn(async move {
                            // 服务端注销会话（失败也照清本地，本地退出兜底）
                            if let Some(t) = t {
                                let _ = request("POST", "/admin/logout", None, Some(&t)).await;
                            }
                            clear_token();
                            token.set(None);
                        });
                    },
                        {icon("logout", 14)}
                        "退出登录"
                    }
                }
            }
            main { class: "main",
                div { class: "page-wrap",
                    {content}
                }
            }
        }
    }
}

// 导航分组：可折叠头部 + 子项（NavLeaf / 嵌套 NavGroup）。children 以 Element 传入，
// 在 if open 分支内直接插入（元素形式，不嵌套 rsx! 宏，符合坑清单）。
#[component]
fn NavGroup(
    label: &'static str,
    icon_name: &'static str,
    open: bool,
    on_toggle: EventHandler<MouseEvent>,
    children: Element,
) -> Element {
    rsx! {
        div { class: "nav-group",
            button { class: "nav-item nav-group-head", onclick: on_toggle,
                {icon(icon_name, 16)}
                span { "{label}" }
                span { class: format!("nav-chevron{}", if open { " open" } else { "" }),
                    {icon("chevron", 12)}
                }
            }
            if open {
                div { class: "nav-group-children", {children} }
            }
        }
    }
}

#[component]
fn NavLeaf(
    name: &'static str,
    label: &'static str,
    active: bool,
    loading: bool,
    onnav: EventHandler<MouseEvent>,
) -> Element {
    rsx! {
        button { class: if active { "nav-item active" } else { "nav-item" }, onclick: onnav,
            {icon(name, 16)}
            span { "{label}" }
            if loading { Spinner { size: 12 } }
        }
    }
}
