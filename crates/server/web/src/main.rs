// crates/server/web/src/main.rs
mod api;
mod components;

use crate::api::request;
use components::combineds::Combineds;
use components::config::Config;
use components::icon::{Spinner, icon};
use components::login::{Login, clear_token, read_token, write_token};
use components::overview::Overview;
use components::preview::Preview;
use components::sources::Sources;
use components::toast::ToastProvider;
use dioxus::prelude::*;

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    let mut token = use_signal(|| read_token());
    // 启动时校验一次本地存储的 token：有效则进入主界面，仅 401（token 失效）时清除回登录页。
    // 网络故障/5xx 等瞬时错误保留 token——admin token 只在首次启动日志打印一次，
    // 误删会让用户无法从 UI 取回。
    let mut checking = use_signal(|| true);
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

// 主壳：侧边栏导航（窄屏自动收成顶栏，见 CSS @media 768px）。
#[component]
fn MainShell(token: Signal<Option<String>>) -> Element {
    let mut tab = use_signal(|| 0usize);
    // use_callback 让 on_goto 句柄跨渲染稳定（避免 EventHandler::new 在 render 体累积）。
    let on_goto = use_callback(move |t: usize| tab.set(t));
    rsx! {
        div { class: "app-shell",
            aside { class: "sidebar",
                div { class: "sidebar-brand",
                    {icon("logo", 22)}
                    span { "sub-merge" }
                }
                nav { class: "nav",
                    NavItem { name: "overview", label: "概览", active: *tab.read() == 0, onnav: move |_| tab.set(0) }
                    NavItem { name: "sources", label: "订阅源", active: *tab.read() == 1, onnav: move |_| tab.set(1) }
                    NavItem { name: "combineds", label: "组合订阅", active: *tab.read() == 2, onnav: move |_| tab.set(2) }
                    NavItem { name: "preview", label: "预览", active: *tab.read() == 3, onnav: move |_| tab.set(3) }
                    NavItem { name: "config", label: "配置", active: *tab.read() == 4, onnav: move |_| tab.set(4) }
                }
                div { class: "sidebar-footer",
                    span { class: "sidebar-version", "v0.1.0" }
                    button { class: "btn btn-ghost btn-sm", onclick: move |_| {
                        clear_token();
                        token.set(None);
                    },
                        {icon("logout", 14)}
                        "退出登录"
                    }
                }
            }
            main { class: "main",
                div { class: "page-wrap",
                    match *tab.read() {
                        0 => rsx! { Overview { token, on_goto } },
                        1 => rsx! { Sources { token } },
                        2 => rsx! { Combineds { token } },
                        3 => rsx! { Preview { token } },
                        _ => rsx! { Config { token } },
                    }
                }
            }
        }
    }
}

#[component]
fn NavItem(
    name: &'static str,
    label: &'static str,
    active: bool,
    onnav: EventHandler<MouseEvent>,
) -> Element {
    rsx! {
        button { class: if active { "nav-item active" } else { "nav-item" }, onclick: onnav,
            {icon(name, 16)}
            span { "{label}" }
        }
    }
}
