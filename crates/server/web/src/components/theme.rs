// crates/server/web/src/components/theme.rs
// 主题管理：html[data-theme] 三态（light/dark/system）+ localStorage 持久化。
// 启动时（dioxus 挂载前）调用 apply_theme 防首帧闪烁；ThemeSwitcher 渲染三态分段按钮。
use crate::components::icon::icon;
use dioxus::prelude::*;
use wasm_bindgen::JsCast; // unchecked_ref（MainShell 同款用法）

pub const THEME_KEY: &str = "submerge_theme";

// system 跟随监听器保活存 thread_local（MainShell MQ_LISTENER 同模式），卸载时移除。
// mql 一并保存：match_media 每次返回新对象，移除监听必须用注册时的同一个 MediaQueryList。
thread_local! {
    static MQ_LISTENER: std::cell::RefCell<Option<(web_sys::MediaQueryList, wasm_bindgen::closure::Closure<dyn FnMut(web_sys::Event)>)>> =
        const { std::cell::RefCell::new(None) };
}

// 与 login.rs read_token 同模式：localStorage 读取失败/未设置一律回退 "system"。
pub fn read_theme() -> String {
    let v = web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|s| s.get_item(THEME_KEY).ok().flatten());
    match v.as_deref() {
        Some("light") | Some("dark") => v.unwrap(),
        _ => "system".to_string(),
    }
}

pub fn apply_theme(theme: &str) {
    if let Some(doc) = web_sys::window().and_then(|w| w.document())
        && let Some(html) = doc.document_element()
    {
        let _ = html.set_attribute("data-theme", theme);
    }
}

pub fn write_theme(theme: &str) {
    if let Some(s) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = s.set_item(THEME_KEY, theme);
    }
}

/// 三态分段切换器：☀ 浅色 / ⚪ 系统 / 🌙 深色。挂载时注册 system 跟随监听，卸载移除。
#[component]
pub fn ThemeSwitcher() -> Element {
    let mut theme = use_signal(read_theme);
    // 注册时应用一次（挂载时保证与存储一致）
    apply_theme(&theme());

    // system 跟随：仅当 data-theme=system 时 matchMedia 变化即时重应用。
    // 用守卫信号保证监听只注册一次（use_effect 无依赖数组，组件重渲染会重跑）。
    let mut inited = use_signal(|| false);
    use_effect(move || {
        if inited() {
            return;
        }
        inited.set(true);
        if let Some(mql) = web_sys::window()
            .and_then(|w| w.match_media("(prefers-color-scheme: dark)").ok().flatten())
        {
            let cb = wasm_bindgen::closure::Closure::wrap(Box::new(move |_e: web_sys::Event| {
                if read_theme() == "system" {
                    apply_theme("system");
                }
            }));
            let et: &web_sys::EventTarget = mql.unchecked_ref();
            let _ = et.add_event_listener_with_callback("change", cb.as_ref().unchecked_ref());
            MQ_LISTENER.with(|c| *c.borrow_mut() = Some((mql, cb)));
        }
    });
    use_drop(|| {
        MQ_LISTENER.with(|c| {
            if let Some((mql, cb)) = c.borrow_mut().take() {
                let et: &web_sys::EventTarget = mql.unchecked_ref();
                let _ =
                    et.remove_event_listener_with_callback("change", cb.as_ref().unchecked_ref());
            }
        });
    });

    let mut set = move |t: &'static str| {
        write_theme(t);
        apply_theme(t);
        theme.set(t.to_string());
        // 主题过渡动画：一次性 class，200ms 后移除（schedule_timeout 同模式）
        if let Some(doc) = web_sys::window().and_then(|w| w.document())
            && let Some(html) = doc.document_element()
        {
            let _ = html.class_list().add_1("theme-transition");
            let html = html.clone();
            crate::components::toast::schedule_timeout(250, move || {
                let _ = html.class_list().remove_1("theme-transition");
            });
        }
    };

    rsx! {
        div { class: "theme-switcher",
            button { class: if theme() == "light" { "seg active" } else { "seg" }, title: "浅色", onclick: move |_| set("light"), {icon("sun", 15)} }
            button { class: if theme() == "system" { "seg active" } else { "seg" }, title: "跟随系统", onclick: move |_| set("system"), {icon("config", 14)} }
            button { class: if theme() == "dark" { "seg active" } else { "seg" }, title: "深色", onclick: move |_| set("dark"), {icon("moon", 15)} }
        }
    }
}
