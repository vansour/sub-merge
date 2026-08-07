// crates/server/web/src/components/mod.rs
pub mod combineds;
pub mod config;
pub mod confirm;
pub mod icon;
pub mod login;
pub mod preview_section;
pub mod sources;
pub mod toast;

// 剪贴板写入（config 页与组合订阅页共用）。web-sys 0.3.103 实测签名：
//   Window::navigator() -> Navigator（直接返回）；Navigator::clipboard() -> Clipboard
//   Clipboard::write_text(&str) -> js_sys::Promise（用 JsFuture await）
pub async fn copy_text(text: String) -> Result<(), String> {
    let nav = web_sys::window().map(|w| w.navigator()).ok_or("无窗口")?;
    // 非安全上下文（http://局域网IP 等非 localhost 访问）下 navigator.clipboard 为
    // undefined：直接 writeText 会抛未捕获 JS 异常，web-sys 非 Result 导入将其转成
    // Rust panic，wasm32 panic=abort 直接让整页失效（实测点击复制按钮后页面卡死）。
    // 先探测属性存在性，不可用时返回 Err 走 toast 提示，不崩溃。
    let clip_value = js_sys::Reflect::get(nav.as_ref(), &wasm_bindgen::JsValue::from_str("clipboard"))
        .map_err(|e| format!("无法读取剪贴板: {:?}", e))?;
    if clip_value.is_undefined() || clip_value.is_null() {
        return Err("剪贴板不可用：需 HTTPS 或 localhost 访问".into());
    }
    let clip: web_sys::Clipboard = clip_value.into();
    wasm_bindgen_futures::JsFuture::from(clip.write_text(&text))
        .await
        .map_err(|e| format!("{:?}", e))?;
    Ok(())
}
