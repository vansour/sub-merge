// crates/server/web/src/components/toast.rs
// 全局 Toast：ToastProvider 提供 Signal<Vec<ToastMsg>> context，渲染右上角堆叠。
// 每条 4s 自动消失；use_effect 在 dioxus 0.8 alpha 无依赖数组（每次渲染都跑），
// 用 scheduled 信号保证定时器只注册一次。
use crate::components::icon::icon;
use dioxus::prelude::*;
use wasm_bindgen::prelude::*;
use submerge_web_core::fmt::{next_toast_id, toast_class, toast_icon};
pub use submerge_web_core::fmt::ToastKind; // 保持既有 `use crate::components::toast::ToastKind` 路径可用

#[derive(Debug, Clone, PartialEq)]
pub struct ToastMsg {
    pub id: u64,
    pub kind: ToastKind,
    pub text: String,
}

/// 追加一条 toast（自动分配自增 id）。
pub fn push_toast(mut toasts: Signal<Vec<ToastMsg>>, kind: ToastKind, text: impl Into<String>) {
    let id = next_toast_id();
    toasts.write().push(ToastMsg { id, kind, text: text.into() });
}

/// 在 ToastProvider 子树内读取 toast 信号。
pub fn use_toast() -> Signal<Vec<ToastMsg>> {
    use_context::<Signal<Vec<ToastMsg>>>()
}

/// wasm setTimeout 封装：ms 毫秒后执行 f（Closure 生命周期由 JS 定时器持有，forget 是有意的）。
pub(crate) fn schedule_timeout(ms: u32, f: impl FnOnce() + 'static) {
    let cb = Closure::once(f);
    if let Some(w) = web_sys::window() {
        let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(cb.as_ref().unchecked_ref(), ms as i32);
    }
    cb.forget();
}

#[component]
pub fn ToastProvider(children: Element) -> Element {
    let toasts = use_context_provider(|| Signal::new(Vec::<ToastMsg>::new()));
    let items = toasts.read().clone();
    rsx! {
        div { class: "toast-stack",
            for t in items {
                ToastCard { toasts, t }
            }
        }
        {children}
    }
}

#[component]
fn ToastCard(toasts: Signal<Vec<ToastMsg>>, t: ToastMsg) -> Element {
    let id = t.id;
    let kind = t.kind;
    let text = t.text.clone();
    let mut scheduled = use_signal(|| false);
    use_effect(move || {
        if scheduled() {
            return;
        }
        scheduled.set(true);
        let mut toasts = toasts.clone();
        schedule_timeout(4000, move || {
            toasts.write().retain(|x| x.id != id);
        });
    });

    let icon_name = toast_icon(kind);
    let kind_class = toast_class(kind);
    let mut toasts = toasts.clone();
    rsx! {
        div { class: format!("toast {kind_class}"),
            {icon(icon_name, 15)}
            span { "{text}" }
            button { class: "toast-close", onclick: move |_| { toasts.write().retain(|x| x.id != id); }, {icon("x", 13)} }
        }
    }
}
