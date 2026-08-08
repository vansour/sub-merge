// crates/server/web/src/components/skeleton.rs
// 骨架屏：shimmer 扫光灰块。三基础形态——表格（订阅源/预览）、卡片（配置页）、列表行（组合页）。
// 全部由 CSS（.skeleton-* + shimmer 动画）驱动，尊重 prefers-reduced-motion。
use dioxus::prelude::*;

#[component]
pub fn SkeletonTable(rows: u8) -> Element {
    // 表头 + rows 行，每行 名称/类型/URL 三块不同宽度灰条
    let row_els: Vec<Element> = (0..rows)
        .map(|_| {
            rsx! {
                tr {
                    td { div { class: "skeleton-block", style: "width: 38%" } }
                    td { div { class: "skeleton-block", style: "width: 18%" } }
                    td { div { class: "skeleton-block", style: "width: 62%" } }
                }
            }
        })
        .collect();
    rsx! {
        div { class: "table-wrap", "aria-hidden": "true",
            table {
                thead { tr { th { div { class: "skeleton-block", style: "width: 30%" } } th { div { class: "skeleton-block", style: "width: 20%" } } th { div { class: "skeleton-block", style: "width: 40%" } } th { div { class: "skeleton-block", style: "width: 24%" } } } }
                tbody { {row_els.into_iter()} }
            }
        }
    }
}

#[component]
pub fn SkeletonCard(rows: u8) -> Element {
    // 标题条 + rows 个内容条
    let row_els: Vec<Element> = (0..rows)
        .map(|_| rsx! { div { class: "skeleton-block", style: "width: 100%; margin-top: 10px" } })
        .collect();
    rsx! {
        div { class: "card", "aria-hidden": "true",
            div { class: "skeleton-block", style: "width: 30%; height: 15px" }
            {row_els.into_iter()}
        }
    }
}

#[component]
pub fn SkeletonList(rows: u8) -> Element {
    // 组合页行形态：名称条 + 徽章位
    let row_els: Vec<Element> = (0..rows)
        .map(|_| {
            rsx! {
                div { class: "combined-row",
                    div { class: "skeleton-block", style: "width: 30%" }
                    div { class: "skeleton-block", style: "width: 10%; margin-left: auto" }
                }
            }
        })
        .collect();
    rsx! {
        div { "aria-hidden": "true", {row_els.into_iter()} }
    }
}
