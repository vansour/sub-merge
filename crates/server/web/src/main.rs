// crates/server/web/src/main.rs
use dioxus::prelude::*;

mod api;

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    rsx! {
        div { class: "container",
            h1 { "sub-merge 管理" }
            p { "界面待完善" }
        }
    }
}
