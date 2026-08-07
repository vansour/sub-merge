// crates/server/web/src/components/login.rs
use crate::api::request;
use crate::components::icon::{icon, Spinner};
use dioxus::prelude::*;

// localStorage key（会话 token，替代旧的 admin token 直存）
const STORAGE_KEY: &str = "submerge_admin_session";

pub fn read_token() -> Option<String> {
    let w = web_sys::window()?;
    let s = w.local_storage().ok().flatten()?;
    s.get_item(STORAGE_KEY).ok().flatten()
}
pub fn write_token(t: &str) {
    if let Some(s) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = s.set_item(STORAGE_KEY, t);
    }
}
pub fn clear_token() {
    if let Some(s) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = s.remove_item(STORAGE_KEY);
    }
}

#[component]
pub fn Login(on_login: EventHandler<String>) -> Element {
    let needs_setup = use_signal(|| None::<bool>); // None=加载中（写入只经 probe 闭包的克隆句柄）
    let mut input = use_signal(String::new);
    let mut login_pass = use_signal(String::new);
    let mut setup_user = use_signal(String::new);
    let mut setup_pass = use_signal(String::new);
    let mut setup_pass2 = use_signal(String::new);
    let mut error = use_signal(String::new);
    let mut loading = use_signal(|| false);

    // 探测 setup 状态的可复用闭包：挂载时调用一次，探测失败后可由「重试」按钮再次调用。
    let probe = use_callback(move |_: ()| {
        let mut needs_setup = needs_setup.clone();
        let mut error = error.clone();
        spawn(async move {
            match request("GET", "/admin/setup-status", None, None).await {
                Ok(body) => match serde_json::from_str::<serde_json::Value>(&body) {
                    Ok(v) => {
                        needs_setup.set(Some(v["needs_setup"].as_bool().unwrap_or(false)));
                        error.set(String::new());
                    }
                    Err(e) => error.set(format!("解析失败: {e}")),
                },
                Err(e) => error.set(format!("检查初始化状态失败: {e}")),
            }
        });
    });
    // 挂载时探测一次：use_future 单参数只在挂载时运行一次（不随重渲染重跑），
    // 无需额外的 None 守卫；probe.call 立即 spawn 请求并返回，探测失败时
    // needs_setup 保持 None（加载中），由「重试」按钮再次调用 probe 恢复。
    use_future(move || async move {
        probe.call(());
    });

    // 登录：POST /admin/login，成功拿到会话 token 交给 App 写入。
    let mut do_submit = move || {
        let user = input.read().clone();
        let pass = login_pass.read().clone();
        if user.is_empty() || pass.is_empty() {
            error.set("请填写用户名与密码".into());
            return;
        }
        loading.set(true);
        spawn(async move {
            let body = serde_json::json!({"username": user, "password": pass}).to_string();
            match request("POST", "/admin/login", Some(body), None).await {
                Ok(b) => match serde_json::from_str::<serde_json::Value>(&b) {
                    Ok(v) => match v["token"].as_str() {
                        Some(t) if !t.is_empty() => on_login.call(t.to_string()),
                        _ => error.set("登录响应缺少 token，请重试".into()),
                    },
                    Err(e) => error.set(format!("解析失败: {e}")),
                },
                Err(e) => error.set(format!("登录失败: {e}")),
            }
            loading.set(false);
        });
    };

    // 创建账号（与登录共用 loading/error，成功后自动登录）
    let mut do_setup = move || {
        let user = setup_user.read().clone();
        let pass = setup_pass.read().clone();
        let pass2 = setup_pass2.read().clone();
        if user.is_empty() || pass.is_empty() {
            error.set("请填写用户名与密码".into());
            return;
        }
        if pass != pass2 {
            error.set("两次输入的密码不一致".into());
            return;
        }
        loading.set(true);
        spawn(async move {
            let body =
                serde_json::json!({"username": user, "password": pass, "password_confirm": pass2})
                    .to_string();
            match request("POST", "/admin/setup", Some(body), None).await {
                Ok(_) => {
                    // 创建成功 → 自动登录
                    let login_body = serde_json::json!({"username": user, "password": pass})
                        .to_string();
                    match request("POST", "/admin/login", Some(login_body), None).await {
                        Ok(b) => match serde_json::from_str::<serde_json::Value>(&b) {
                            Ok(v) => match v["token"].as_str() {
                                Some(t) if !t.is_empty() => on_login.call(t.to_string()),
                                _ => error.set("登录响应缺少 token，请重试".into()),
                            },
                            Err(e) => error.set(format!("解析失败: {e}")),
                        },
                        Err(e) => error.set(format!("登录失败: {e}")),
                    }
                }
                Err(e) => error.set(format!("创建失败: {e}")),
            }
            loading.set(false);
        });
    };

    // 三态主体在 rsx 外预计算（沿用 MainShell content 的模式）：
    // None=探测 setup 状态（全页转圈）；true=首建引导；false=登录。
    let body: Element = match *needs_setup.read() {
        None => rsx! {
            // 探测失败（网络错误/非 JSON）时 needs_setup 保持 None：错误可见 + 重试按钮，
            // 否则用户卡在无限转圈页无任何提示、也无法恢复。
            if !error.read().is_empty() {
                p { class: "error-text", "{error}" }
                button { class: "btn btn-primary", onclick: move |_| probe.call(()), "重试" }
            } else {
                div { class: "login-logo", Spinner { size: 40 } }
            }
        },
        Some(true) => rsx! {
            div { class: "field",
                input {
                    type: "text",
                    placeholder: "用户名",
                    value: setup_user,
                    oninput: move |e| setup_user.set(e.value()),
                }
            }
            div { class: "field",
                input {
                    type: "password",
                    placeholder: "密码",
                    value: setup_pass,
                    oninput: move |e| setup_pass.set(e.value()),
                }
            }
            div { class: "field",
                input {
                    type: "password",
                    placeholder: "确认密码",
                    value: setup_pass2,
                    oninput: move |e| setup_pass2.set(e.value()),
                    onkeydown: move |e| {
                        if e.key() == Key::Enter {
                            do_setup();
                        }
                    },
                }
            }
            if !error.read().is_empty() {
                p { class: "error-text", "{error}" }
            }
            button { class: "btn btn-primary", onclick: move |_| do_setup(), disabled: *loading.read(),
                if *loading.read() {
                    Spinner { size: 14 }
                } else {
                    "创建并登录"
                }
            }
        },
        Some(false) => rsx! {
            div { class: "field",
                input {
                    type: "text",
                    placeholder: "用户名",
                    value: input,
                    oninput: move |e| input.set(e.value()),
                }
            }
            div { class: "field",
                input {
                    type: "password",
                    placeholder: "密码",
                    value: login_pass,
                    oninput: move |e| login_pass.set(e.value()),
                    onkeydown: move |e| {
                        if e.key() == Key::Enter {
                            do_submit();
                        }
                    },
                }
            }
            if !error.read().is_empty() {
                p { class: "error-text", "{error}" }
            }
            button { class: "btn btn-primary", onclick: move |_| do_submit(), disabled: *loading.read(),
                if *loading.read() {
                    Spinner { size: 14 }
                } else {
                    "登录"
                }
            }
        },
    };

    rsx! {
        div { class: "login-wrap",
            div { class: "login-card",
                div { class: "login-logo", {icon("logo", 40)} }
                div { class: "login-title", "sub-merge" }
                p { class: "login-sub", "订阅聚合与转换管理" }
                {body}
            }
        }
    }
}
