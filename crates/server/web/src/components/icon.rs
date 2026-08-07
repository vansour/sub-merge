// crates/server/web/src/components/icon.rs
// 内联 SVG 图标（Lucide 风格：stroke 线稿，fill=none）。
// 不依赖任何图标库，全部手写 path；颜色跟随 currentColor（由父元素 color 控制）。
use dioxus::prelude::*;

pub fn icon(name: &'static str, size: u32) -> Element {
    match name {
        // logo：嵌套六边形（工具/聚合感）
        "logo" => rsx! {
            svg { width: size, height: size, view_box: "0 0 24 24", fill: "none", stroke: "currentColor", stroke_width: "2", stroke_linecap: "round", stroke_linejoin: "round",
                path { d: "M12 2 20 6.5v11L12 22 4 17.5v-11L12 2Z" }
                path { d: "M12 6.5 16.5 9v6L12 17.5 7.5 15V9l4.5-2.5Z" }
            }
        },
        // sources：链节
        "sources" => rsx! {
            svg { width: size, height: size, view_box: "0 0 24 24", fill: "none", stroke: "currentColor", stroke_width: "2", stroke_linecap: "round", stroke_linejoin: "round",
                path { d: "M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71" }
                path { d: "M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71" }
            }
        },
        // combineds：层叠（多个组合）
        "combineds" => rsx! {
            svg { width: size, height: size, view_box: "0 0 24 24", fill: "none", stroke: "currentColor", stroke_width: "2", stroke_linecap: "round", stroke_linejoin: "round",
                path { d: "M12.83 2.18a2 2 0 0 0-1.66 0L2.6 6.08a1 1 0 0 0 0 1.83l8.58 3.91a2 2 0 0 0 1.66 0l8.58-3.9a1 1 0 0 0 0-1.83Z" }
                path { d: "m22 17.65-9.17 4.16a2 2 0 0 1-1.66 0L2 17.65" }
                path { d: "m22 12.65-9.17 4.16a2 2 0 0 1-1.66 0L2 12.65" }
            }
        },
        // preview：眼睛
        "preview" => rsx! {
            svg { width: size, height: size, view_box: "0 0 24 24", fill: "none", stroke: "currentColor", stroke_width: "2", stroke_linecap: "round", stroke_linejoin: "round",
                path { d: "M2.062 12.348a1 1 0 0 1 0-.696 10.75 10.75 0 0 1 19.876 0 1 1 0 0 1 0 .696 10.75 10.75 0 0 1-19.876 0" }
                circle { cx: "12", cy: "12", r: "3" }
            }
        },
        // chevron：右箭头（折叠分组展开指示）
        "chevron" => rsx! {
            svg { width: size, height: size, view_box: "0 0 24 24", fill: "none", stroke: "currentColor", stroke_width: "2", stroke_linecap: "round", stroke_linejoin: "round",
                path { d: "m9 18 6-6-6-6" }
            }
        },
        // clash：文件代码（Lucide file-code）
        "clash" => rsx! {
            svg { width: size, height: size, view_box: "0 0 24 24", fill: "none", stroke: "currentColor", stroke_width: "2", stroke_linecap: "round", stroke_linejoin: "round",
                path { d: "M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z" }
                path { d: "M14 2v4a2 2 0 0 0 2 2h4" }
                path { d: "m10 13-2 2 2 2" }
                path { d: "m14 17 2-2-2-2" }
            }
        },
        // config：齿轮
        "config" => rsx! {
            svg { width: size, height: size, view_box: "0 0 24 24", fill: "none", stroke: "currentColor", stroke_width: "2", stroke_linecap: "round", stroke_linejoin: "round",
                path { d: "M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" }
                circle { cx: "12", cy: "12", r: "3" }
            }
        },
        // logout：出门箭头
        "logout" => rsx! {
            svg { width: size, height: size, view_box: "0 0 24 24", fill: "none", stroke: "currentColor", stroke_width: "2", stroke_linecap: "round", stroke_linejoin: "round",
                path { d: "M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4" }
                polyline { points: "16 17 21 12 16 7" }
                line { x1: "21", x2: "9", y1: "12", y2: "12" }
            }
        },
        // refresh：环形箭头
        "refresh" => rsx! {
            svg { width: size, height: size, view_box: "0 0 24 24", fill: "none", stroke: "currentColor", stroke_width: "2", stroke_linecap: "round", stroke_linejoin: "round",
                path { d: "M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8" }
                path { d: "M21 3v5h-5" }
                path { d: "M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16" }
                path { d: "M8 16H3v5" }
            }
        },
        // copy：双矩形
        "copy" => rsx! {
            svg { width: size, height: size, view_box: "0 0 24 24", fill: "none", stroke: "currentColor", stroke_width: "2", stroke_linecap: "round", stroke_linejoin: "round",
                rect { width: "14", height: "14", x: "8", y: "8", rx: "2", ry: "2" }
                path { d: "M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2" }
            }
        },
        // trash：垃圾桶
        "trash" => rsx! {
            svg { width: size, height: size, view_box: "0 0 24 24", fill: "none", stroke: "currentColor", stroke_width: "2", stroke_linecap: "round", stroke_linejoin: "round",
                path { d: "M3 6h18" }
                path { d: "M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6" }
                path { d: "M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2" }
            }
        },
        // plus：加号
        "plus" => rsx! {
            svg { width: size, height: size, view_box: "0 0 24 24", fill: "none", stroke: "currentColor", stroke_width: "2", stroke_linecap: "round", stroke_linejoin: "round",
                path { d: "M5 12h14" }
                path { d: "M12 5v14" }
            }
        },
        // check：对勾
        "check" => rsx! {
            svg { width: size, height: size, view_box: "0 0 24 24", fill: "none", stroke: "currentColor", stroke_width: "2", stroke_linecap: "round", stroke_linejoin: "round",
                path { d: "M20 6 9 17l-5-5" }
            }
        },
        // x：关闭
        "x" => rsx! {
            svg { width: size, height: size, view_box: "0 0 24 24", fill: "none", stroke: "currentColor", stroke_width: "2", stroke_linecap: "round", stroke_linejoin: "round",
                path { d: "M18 6 6 18" }
                path { d: "m6 6 12 12" }
            }
        },
        // alert：三角警告
        "alert" => rsx! {
            svg { width: size, height: size, view_box: "0 0 24 24", fill: "none", stroke: "currentColor", stroke_width: "2", stroke_linecap: "round", stroke_linejoin: "round",
                path { d: "m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3" }
                path { d: "M12 9v4" }
                path { d: "M12 17h.01" }
            }
        },
        _ => rsx! { span {} },
    }
}

// 旋转加载图标：CSS .spinner 动画驱动。
#[component]
pub fn Spinner(size: u32) -> Element {
    rsx! {
        svg { width: size, height: size, view_box: "0 0 24 24", fill: "none", stroke: "currentColor", stroke_width: "2", stroke_linecap: "round", class: "spinner",
            path { d: "M21 12a9 9 0 1 1-6.219-8.56" }
        }
    }
}
