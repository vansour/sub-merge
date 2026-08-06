// crates/server/web/src/components/mod.rs
pub mod combineds;
pub mod config;
pub mod confirm;
pub mod icon;
pub mod login;
pub mod overview;
pub mod preview;
pub mod sources;
pub mod toast;

// 剪贴板写入（config 页与组合订阅页共用）。web-sys 0.3.103 实测签名：
//   Window::navigator() -> Navigator（直接返回）；Navigator::clipboard() -> Clipboard
//   Clipboard::write_text(&str) -> js_sys::Promise（用 JsFuture await）
pub async fn copy_text(text: String) -> Result<(), String> {
    let nav = web_sys::window().map(|w| w.navigator()).ok_or("无窗口")?;
    let clip = nav.clipboard();
    wasm_bindgen_futures::JsFuture::from(clip.write_text(&text))
        .await
        .map_err(|e| format!("{:?}", e))?;
    Ok(())
}
