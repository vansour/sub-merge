// 前端展示映射/格式化纯函数（协议配色、类型文案、订阅链接路径、toast 映射与 id 分配）。

/// Toast 种类。文案由 push_toast 调用方提供，这里只负责图标与样式映射。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ToastKind {
    Success,
    Error,
    Info,
}

/// Toast 图标名（icon.rs 的 SVG 名）。
pub fn toast_icon(kind: ToastKind) -> &'static str {
    match kind {
        ToastKind::Success => "check",
        ToastKind::Error => "alert",
        ToastKind::Info => "config",
    }
}

/// Toast 样式类（index.html 的 .toast.success/.error/.info）。
pub fn toast_class(kind: ToastKind) -> &'static str {
    match kind {
        ToastKind::Success => "success",
        ToastKind::Error => "error",
        ToastKind::Info => "info",
    }
}

/// 协议 → 配色（CSS --proto-0..5）。同族协议同色。
pub fn proto_class(protocol: &str) -> &'static str {
    match protocol {
        "ss" | "ssr" => "proto-0",
        "vmess" | "vless" => "proto-1",
        "trojan" => "proto-2",
        "hysteria" | "hysteria2" => "proto-3",
        "tuic" => "proto-4",
        _ => "proto-5",
    }
}

/// 订阅源类型 → 展示文案（单条/远程）。
pub fn kind_label(kind: &str) -> &'static str {
    if kind == "single" { "单条" } else { "远程" }
}

/// 组合订阅输出路径（不含 base origin，origin 由组件从 window.location 拼装）。
/// 与现状一致不做 URL 编码。
pub fn subscribe_path(name: &str, fmt: &str) -> String {
    format!("/subscribe/{name}?format={fmt}")
}

thread_local! {
    static NEXT_ID: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
}

/// 分配自增 toast id（从 1 开始）。
pub fn next_toast_id() -> u64 {
    NEXT_ID.with(|c| {
        let v = c.get();
        c.set(v + 1);
        v
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proto_class_known_protocols() {
        assert_eq!(proto_class("ss"), "proto-0");
        assert_eq!(proto_class("ssr"), "proto-0");
        assert_eq!(proto_class("vmess"), "proto-1");
        assert_eq!(proto_class("vless"), "proto-1");
        assert_eq!(proto_class("trojan"), "proto-2");
        assert_eq!(proto_class("hysteria"), "proto-3");
        assert_eq!(proto_class("hysteria2"), "proto-3");
        assert_eq!(proto_class("tuic"), "proto-4");
    }

    #[test]
    fn proto_class_fallback() {
        assert_eq!(proto_class("wireguard"), "proto-5");
        assert_eq!(proto_class(""), "proto-5");
    }

    #[test]
    fn kind_label_branches() {
        assert_eq!(kind_label("single"), "单条");
        assert_eq!(kind_label("remote"), "远程");
        assert_eq!(kind_label("unknown"), "远程"); // 非 single 一律远程（与现状一致）
    }

    #[test]
    fn subscribe_path_builds() {
        assert_eq!(
            subscribe_path("home", "v2ray"),
            "/subscribe/home?format=v2ray"
        );
        // 现状不做 URL 编码：含特殊字符时原样拼接。
        assert_eq!(
            subscribe_path("my sub", "v2ray"),
            "/subscribe/my sub?format=v2ray"
        );
    }

    #[test]
    fn toast_mappings() {
        assert_eq!(toast_icon(ToastKind::Success), "check");
        assert_eq!(toast_icon(ToastKind::Error), "alert");
        assert_eq!(toast_icon(ToastKind::Info), "config");
        assert_eq!(toast_class(ToastKind::Success), "success");
        assert_eq!(toast_class(ToastKind::Error), "error");
        assert_eq!(toast_class(ToastKind::Info), "info");
    }

    #[test]
    fn toast_ids_monotonic() {
        let a = next_toast_id();
        let b = next_toast_id();
        assert_eq!(b, a + 1);
    }
}
