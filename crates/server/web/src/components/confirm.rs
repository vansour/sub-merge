// crates/server/web/src/components/confirm.rs
// 通用确认弹窗：页面持有 ConfirmState 信号（open=false 时不渲染任何内容），
// 确认/取消/遮罩点击都会关闭；danger=true 时确认按钮为红色。
use dioxus::prelude::*;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ConfirmState {
    pub open: bool,
    pub title: String,
    pub message: String,
    pub confirm_text: String,
    pub danger: bool,
}

#[component]
pub fn ConfirmDialog(state: Signal<ConfirmState>, on_confirm: EventHandler<()>) -> Element {
    if !state.read().open {
        // dioxus 0.8 alpha: Element = Result<VNode, RenderError>，空元素用 VNode::empty()。
        return VNode::empty();
    }
    let title = state.read().title.clone();
    let message = state.read().message.clone();
    let confirm_text = state.read().confirm_text.clone();
    let danger = state.read().danger;
    let mut state = state.clone();
    let on_confirm = on_confirm.clone();
    rsx! {
        div { class: "modal-overlay", onclick: move |_| state.set(ConfirmState::default()),
            div { class: "modal", onclick: move |e| e.stop_propagation(),
                h3 { class: "modal-title", "{title}" }
                p { class: "modal-message", "{message}" }
                div { class: "modal-actions",
                    button { class: "btn btn-ghost", onclick: move |_| state.set(ConfirmState::default()), "取消" }
                    button { class: if danger { "btn btn-danger" } else { "btn btn-primary" },
                        onclick: move |_| on_confirm.call(()),
                        "{confirm_text}"
                    }
                }
            }
        }
    }
}
