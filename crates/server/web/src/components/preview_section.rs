// crates/server/web/src/components/preview_section.rs
// 共享预览区：节点表 + 源错误卡 + 刷新按钮。本地/远程/组合三页复用。
// kind: Some("single"/"remote") 按类型过滤；combined: Some(组合名) 按组合过滤；二者互斥由调用方保证。
// 拉取路径：kind 有值 → /admin/preview?kind={kind}；combined 有值 → /admin/preview?combined={name}；都无 → /admin/preview。
// 挂载自动加载 + kind/combined prop 变化自动重拉；刷新按钮文案「刷新预览」（ui-check 依赖）。
use crate::api::request;
use crate::components::icon::{Spinner, icon};
use dioxus::prelude::*;
use submerge_web_core::dto::PreviewResp;
use submerge_web_core::fmt::proto_class;

#[component]
pub fn PreviewSection(
    token: Signal<Option<String>>,
    kind: Option<&'static str>,
    combined: Option<String>,
) -> Element {
    let data = use_signal(|| None::<PreviewResp>);
    let loading = use_signal(|| false);
    let error = use_signal(String::new);
    // 请求序号：每次拉取 +1，响应到达时与当前序号比对，过期（慢的旧请求晚到）直接丢弃。
    // 外层绑定无需 mut：所有写入都在 load 闭包内对克隆句柄进行（Signal 句柄 Copy）。
    let req_seq = use_signal(|| 0u32);
    // 当前拉取参数键（kind+combined 组合）；变化时触发重拉
    let key = format!("{}|{}", kind.unwrap_or(""), combined.as_deref().unwrap_or(""));
    let mut loaded_key = use_signal(|| None::<String>);

    // load 只捕获 Copy 的信号句柄，参数 key 由调用方（effect / 刷新按钮）传入当前值，
    // 因此闭包本身 Copy，可被 effect 与刷新按钮各自 move 捕获，且无过期捕获。
    let load = move |key: String| {
        let token = token.read().clone();
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
            // key = "{kind}|{combined}"；kind 分支在前的字符串前缀判断对 combined 名安全
            // （combined 时 key 以 "|" 开头，不会命中 "single|"/"remote|"）。
            let path = if key.starts_with("single|") {
                "/admin/preview?kind=single".to_string()
            } else if key.starts_with("remote|") {
                "/admin/preview?kind=remote".to_string()
            } else {
                let name = key.split('|').nth(1).unwrap_or("");
                if name.is_empty() {
                    "/admin/preview".to_string()
                } else {
                    format!("/admin/preview?combined={name}")
                }
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

    // 挂载 + prop 变化时重拉。dioxus 0.8 alpha 的 use_effect 只在挂载与被读信号变化时重跑
    // （组件重渲染不会自动重跑），而 key 是非响应式局部值——用 use_reactive 把它挂进信号，
    // prop 变化 → key 变化 → effect 重跑；loaded_key 守卫保证同一 key 只拉取一次。
    use_effect(use_reactive((&key,), move |(key,)| {
        if loaded_key.read().as_deref() != Some(&key) {
            loaded_key.set(Some(key.clone()));
            load(key);
        }
    }));

    // 行预渲染（节点表），沿用原 preview.rs 模式。
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

    rsx! {
        div { class: "preview-toolbar",
            if let Some(r) = data.read().as_ref() {
                span { class: "badge on", "共 {r.total} 个节点" }
            }
            button { class: "btn btn-secondary", onclick: move |_| load(key.clone()), disabled: loading(),
                if loading() {
                    Spinner { size: 14 }
                } else {
                    {icon("refresh", 14)}
                }
                "刷新预览"
            }
        }
        if !error_text.is_empty() {
            p { class: "error-text", "{error_text}" }
        }
        if let Some(r) = data.read().as_ref() {
            if r.nodes.is_empty() {
                div { class: "empty",
                    {icon("preview", 36)}
                    span { class: "empty-title", "暂无节点" }
                    span { class: "empty-hint", "检查订阅源是否已启用、刷新后重试" }
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
