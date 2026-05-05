mod api;
mod app;
mod components;
mod prefs;
mod routes;
mod state;
mod types;

use wasm_bindgen::JsCast;

fn main() {
    std::panic::set_hook(Box::new(|info| {
        let msg = format!("WASM PANIC: {info}");
        web_sys::console::error_1(&msg.clone().into());
        if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
            if let Some(app) = doc.get_element_by_id("app") {
                let pre = doc.create_element("pre").unwrap();
                let pre: web_sys::HtmlElement = pre.unchecked_into();
                pre.set_attribute(
                    "style",
                    "padding:24px;color:#ff7a7a;background:#181b22;\
                     font:13px/1.4 ui-monospace,monospace;white-space:pre-wrap;",
                )
                .ok();
                pre.set_inner_text(&msg);
                app.set_inner_html("");
                let _ = app.append_child(&pre);
            }
        }
    }));

    web_sys::console::log_1(&"bili-rust-ui: WASM started".into());

    // Clear the static placeholder so a successful mount is visible.
    let app_el = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("app"))
        .expect("missing #app element");
    app_el.set_inner_html("");

    leptos::mount::mount_to(app_el.unchecked_into(), app::App).forget();
    web_sys::console::log_1(&"bili-rust-ui: mounted".into());
}
