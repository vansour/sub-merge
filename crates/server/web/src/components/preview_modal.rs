// crates/server/web/src/components/preview_modal.rs
// 全屏预览弹窗（每订阅子功能）：节点表 + 源错误卡 + 失败重试。源/组合两页复用。
// source_id: Some(源 id) → /admin/preview?source_id={id}；combined: Some(组合名) → /admin/preview?combined={name}；
// 都无 → /admin/preview。二者互斥由调用方保证。
// 挂载时锁背景滚动（body overflow hidden），卸载恢复（use_drop）——不用页面级监听器。
// token 从 DataStore context 读（MainShell 提供），props 保持 source_id/combined/on_close 三参。
// on_close 采用 confirm.rs 的 EventHandler<()> 模式（call(()) 调用，无需事件值）。
use crate::api::request;
use crate::components::icon::{Spinner, icon};
use crate::data::DataStore;
use dioxus::prelude::*;
use submerge_web_core::dto::PreviewResp;
use submerge_web_core::fmt::proto_class;

#[component]
pub fn PreviewModal(
    source_id: Option<i64>,
    combined: Option<String>,
    on_close: EventHandler<()>,
) -> Element {
    let store = use_context::<DataStore>();
    let data = use_signal(|| None::<PreviewResp>);
    let loading = use_signal(|| true);
    let error = use_signal(String::new);
    // 请求序号：每次拉取 +1，响应到达时与当前序号比对，过期（慢的旧请求晚到）直接丢弃。
    let req_seq = use_signal(|| 0u32);
    // 当前拉取参数键（source_id/combined 组合）；挂载触发一次拉取
    let key = match source_id {
        Some(id) => format!("sid:{id}"),
        None => format!("c:{}", combined.clone().unwrap_or_default()),
    };
    let mut loaded_key = use_signal(|| None::<String>);

    // load 只捕获 Copy 的信号句柄，参数 key 由调用方（effect / 重试按钮）传入当前值，
    // 因此闭包本身 Copy，可被 effect 与按钮各自 move 捕获，且无过期捕获。
    let load = move |key: String| {
        let token = store.token.read().clone();
        let mut data = data.clone();
        let mut loading = loading.clone();
        let mut error = error.clone();
        let mut req_seq = req_seq.clone();
        let seq = {
            let mut s = req_seq.write();
            *s += 1;
            *s
        };
        spawn(async move {
            loading.set(true);
            error.set(String::new());
            let path = if let Some(rest) = key.strip_prefix("sid:") {
                format!("/admin/preview?source_id={rest}")
            } else if let Some(name) = key.strip_prefix("c:") {
                if name.is_empty() {
                    "/admin/preview".to_string()
                } else {
                    format!("/admin/preview?combined={name}")
                }
            } else {
                "/admin/preview".to_string()
            };
            match request("GET", &path, None, token.as_deref()).await {
                Ok(body) => {
                    if *req_seq.read() != seq {
                        return; // 过期响应（更新的请求已发起）直接丢弃
                    }
                    match serde_json::from_str::<PreviewResp>(&body) {
                        Ok(r) => data.set(Some(r)),
                        Err(e) => error.set(format!("解析失败: {}", e)),
                    }
                }
                Err(e) => {
                    if *req_seq.read() == seq {
                        error.set(e.to_string());
                    }
                }
            }
            // loading 关闭同样受 seq 保护：过期请求不得关掉新请求的 loading
            if *req_seq.read() == seq {
                loading.set(false);
            }
        });
    };

    // 挂载时拉取。dioxus 0.8 alpha 的 use_effect 只在挂载与被读信号变化时重跑
    // （组件重渲染不会自动重跑），而 key 是非响应式局部值——用 use_reactive 把它挂进信号；
    // loaded_key 守卫保证同一 key 只拉取一次。
    use_effect(use_reactive((&key,), move |(key,)| {
        if loaded_key.read().as_deref() != Some(&key) {
            loaded_key.set(Some(key.clone()));
            load(key);
        }
    }));

    // 挂载时锁背景滚动；卸载恢复。use_effect 无依赖数组、体内不读信号 → 只执行一次。
    use_effect(move || {
        if let Some(body) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.body())
        {
            let _ = body.style().set_property("overflow", "hidden");
        }
    });
    use_drop(|| {
        if let Some(body) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.body())
        {
            let _ = body.style().remove_property("overflow");
        }
    });

    // 行预渲染（节点表），沿用原 preview_section.rs 模式。
    let rows: Vec<Element> = data
        .read()
        .as_ref()
        .map(|r| {
            r.nodes
                .iter()
                .map(|n| {
                    let name = n.name.clone();
                    let protocol = n.protocol.clone();
                    let server = n.server.clone();
                    let port = n.port;
                    rsx! {
                        tr {
                            td { class: "cell-name", "{name}" }
                            td { span { class: format!("proto {}", proto_class(&protocol)), "{protocol}" } }
                            td { class: "cell-url", "{server}" }
                            td { "{port}" }
                        }
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    // 行预渲染（源错误卡）。
    let error_rows: Vec<Element> = data
        .read()
        .as_ref()
        .map(|r| {
            r.errors
                .iter()
                .map(|e| {
                    let e = e.clone();
                    rsx! {
                        div { class: "error-line", {icon("alert", 14)} span { "{e}" } }
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    let error_text = error.read().clone();
    let close_head = on_close.clone();
    let close_foot = on_close.clone();

    rsx! {
        div { class: "fullscreen-modal",
            div { class: "fullscreen-modal-head",
                h2 { class: "fullscreen-modal-title", "预览" }
                button { class: "btn btn-ghost btn-sm", onclick: move |_| close_head.call(()), {icon("x", 16)} }
            }
            div { class: "fullscreen-modal-body",
                if loading() {
                    div { class: "empty", Spinner { size: 28 } }
                } else if !error_text.is_empty() {
                    div { class: "empty",
                        {icon("alert", 36)}
                        span { class: "empty-title", "加载失败" }
                        span { class: "empty-hint", "{error_text}" }
                        button { class: "btn btn-secondary", onclick: move |_| load(key.clone()), {icon("refresh", 14)} "重试" }
                    }
                } else {
                    if let Some(r) = data.read().as_ref() {
                        if r.nodes.is_empty() {
                            div { class: "empty",
                                {icon("preview", 36)}
                                span { class: "empty-title", "暂无节点" }
                                span { class: "empty-hint", "该订阅源暂无可用节点" }
                            }
                        } else {
                            div { class: "table-wrap",
                                table {
                                    thead {
                                        tr { th { "名称" } th { "协议" } th { "服务器" } th { "端口" } }
                                    }
                                    tbody {
                                        {rows.into_iter()}
                                    }
                                }
                            }
                        }
                        if !r.errors.is_empty() {
                            h2 { class: "card-title", style: "margin-top: 20px", "源错误" }
                            div { class: "warning-box", {error_rows.into_iter()} }
                        }
                    }
                }
            }
            div { class: "fullscreen-modal-foot",
                button { class: "btn btn-secondary", onclick: move |_| close_foot.call(()), "关闭" }
            }
        }
    }
}
