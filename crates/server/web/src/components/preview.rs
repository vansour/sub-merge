// crates/server/web/src/components/preview.rs
// 转换预览页：页面外壳（页头 + 组合过滤下拉）保留；「全部源」与「组合过滤」两视图
// 统一由 PreviewSection 渲染（自带挂载加载、刷新按钮与错误展示）。
// 下拉选项来自 combineds 缓存单元；combineds 单元失败时页内可见原因。
use crate::components::preview_section::PreviewSection;
use crate::data::DataStore;
use dioxus::prelude::*;

#[component]
pub fn Preview(token: Signal<Option<String>>) -> Element {
    let data = use_context::<DataStore>();
    let mut selected = use_signal(|| None::<String>);

    // 下拉选项来自 combineds 缓存单元（预渲染，与行/成员行同模式）。
    // selected 属性与当前选择比对；as_ref 借用，闭包不捕获 owned Vec。
    let combineds_state = data.combineds.read().clone();
    let combined_options: Vec<Element> = combineds_state
        .data
        .as_ref()
        .map(|list| {
            list.iter()
                .map(|c| {
                    let name = c.name.clone();
                    let is_selected = selected.read().as_deref() == Some(name.as_str());
                    rsx! {
                        option { value: name.clone(), selected: is_selected, "{name}" }
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    // 页内错误 = combineds 次级单元错误（下拉数据源：失败时没有选项可选，须可见原因）。
    // 预览区自身的加载/错误由 PreviewSection 自管。
    let page_error = combineds_state.error.clone();

    rsx! {
        div { class: "page-head",
            h1 { class: "page-title", "转换预览" }
            select {
                class: "preview-filter",
                value: selected.read().clone().unwrap_or_default(),
                onchange: move |e| {
                    let v = e.value();
                    let v = if v.is_empty() { None } else { Some(v) };
                    selected.set(v.clone());
                },
                option { value: "", "全部源" }
                {combined_options.into_iter()}
            }
        }
        if !page_error.is_empty() {
            p { class: "error-text", "{page_error}" }
        }
        PreviewSection { token, kind: None, combined: selected.read().clone() }
    }
}
