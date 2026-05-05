pub mod api;
mod commands;
pub mod cookies;
pub mod danmaku;
mod stream;
pub mod wbi;

use std::sync::Arc;

use crate::api::Bili;
use crate::cookies::Cookies;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,bili_rust_lib=debug".into()),
        )
        .init();

    let cookie_path = std::env::var("BILI_COOKIES")
        .ok()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(default_cookie_path);

    let cookies = Cookies::load(&cookie_path)
        .unwrap_or_else(|e| panic!("loading cookies from {}: {e:#}", cookie_path.display()));
    let bili = Arc::new(Bili::new(cookies).expect("init bili client"));

    let bili_for_scheme = bili.clone();
    let bili_for_img = bili.clone();

    tauri::Builder::default()
        .manage(bili)
        .register_asynchronous_uri_scheme_protocol("bilistream", move |ctx, request, responder| {
            stream::handle(bili_for_scheme.clone(), ctx, request, responder);
        })
        .register_asynchronous_uri_scheme_protocol("biliimg", move |ctx, request, responder| {
            stream::handle_image(bili_for_img.clone(), ctx, request, responder);
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_user_info,
            commands::get_rcmd,
            commands::get_related,
            commands::get_play_info,
            commands::get_danmaku,
            commands::get_comments,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn default_cookie_path() -> std::path::PathBuf {
    // src-tauri/../cookies.json — dev cwd is usually src-tauri
    let cwd = std::env::current_dir().unwrap_or_default();
    let here = cwd.join("cookies.json");
    if here.exists() {
        return here;
    }
    cwd.parent().map(|p| p.join("cookies.json")).unwrap_or(here)
}
