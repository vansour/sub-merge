//! submerge-web-core：sub-merge 前端纯逻辑（API 契约 DTO、错误类型、映射/格式化函数）。
//! 无 dioxus/web-sys 依赖，可在 host 原生目标直接 cargo test。
//! 依赖方向：submerge-web（web crate）→ 本 crate；本 crate 不依赖任何前端渲染代码。

pub mod dto;
