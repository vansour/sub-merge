// crates/server/web/src/data.rs
// 集中式数据层:四个 API 拉取函数 + 页面间共享的单元缓存(DataStore)。
// 页面不再各自 use_future 拉取,改为从 DataStore 读缓存;MainShell 编排预载。
use crate::api::request;
use submerge_web_core::dto::{CombinedDto, ConfigDto, PreviewSummary, SourceDto};

pub async fn fetch_sources(token: Option<&str>) -> Result<Vec<SourceDto>, String> {
    let body = request("GET", "/admin/sources", None, token).await?;
    serde_json::from_str(&body).map_err(|e| format!("解析失败: {}", e))
}

pub async fn fetch_combineds(token: Option<&str>) -> Result<Vec<CombinedDto>, String> {
    let body = request("GET", "/admin/combineds", None, token).await?;
    serde_json::from_str(&body).map_err(|e| format!("解析失败: {}", e))
}

pub async fn fetch_preview_summary(token: Option<&str>) -> Result<PreviewSummary, String> {
    let body = request("GET", "/admin/preview", None, token).await?;
    serde_json::from_str(&body).map_err(|e| format!("解析失败: {}", e))
}

pub async fn fetch_config(token: Option<&str>) -> Result<ConfigDto, String> {
    let body = request("GET", "/admin/config", None, token).await?;
    serde_json::from_str(&body).map_err(|e| format!("解析失败: {}", e))
}
