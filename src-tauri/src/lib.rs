pub mod api;
mod auth;
mod commands;
pub mod cookies;
pub mod danmaku;
mod stream;
pub mod wbi;

use std::sync::Arc;

use tauri::Manager;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::api::Bili;
use crate::cookies::Cookies;

// Held in Tauri-managed state for the app's lifetime; dropping flushes any
// pending log writes from `tracing_appender::non_blocking`.
struct LogGuard(#[allow(dead_code)] WorkerGuard);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let log_guard = init_tracing(app.handle());

            let cookie_path = std::env::var("BILI_COOKIES")
                .ok()
                .map(std::path::PathBuf::from)
                .unwrap_or_else(default_cookie_path);

            // Cookies are optional at startup — without them the user lands on /login
            // and the QR flow writes them in. Only hard-fail on truly broken JSON.
            let bili = match Cookies::load(&cookie_path) {
                Ok(c) => Arc::new(Bili::new(c).expect("init bili client")),
                Err(e) => {
                    tracing::warn!(
                        path = %cookie_path.display(),
                        err = %e,
                        "starting without cookies — log in via /login"
                    );
                    Arc::new(Bili::empty().expect("init empty bili client"))
                }
            };
            bili.set_cookies_path(cookie_path);

            // Best-effort: attach `buvid3/buvid4/b_nut/_uuid` if the saved cookies
            // are missing them. Bilibili's risk-control rejects write endpoints
            // (`archive/like`, `coin/add`, `relation/modify`) with `code=-403
            // 账号异常` when these device-fingerprint cookies aren't present.
            let bili_for_fingerprint = bili.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = bili_for_fingerprint.ensure_fingerprint_cookies().await {
                    tracing::warn!(error = %e, "ensure_fingerprint_cookies failed");
                }
            });

            app.manage(bili);
            if let Some(g) = log_guard {
                app.manage(LogGuard(g));
            }
            Ok(())
        })
        .register_asynchronous_uri_scheme_protocol("bilistream", |ctx, request, responder| {
            let bili = ctx.app_handle().state::<Arc<Bili>>().inner().clone();
            stream::handle(bili, ctx, request, responder);
        })
        .register_asynchronous_uri_scheme_protocol("biliimg", |ctx, request, responder| {
            let bili = ctx.app_handle().state::<Arc<Bili>>().inner().clone();
            stream::handle_image(bili, ctx, request, responder);
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_user_info,
            commands::get_rcmd,
            commands::feed_dislike,
            commands::get_related,
            commands::get_play_info,
            commands::get_last_play,
            commands::get_view_info,
            commands::get_danmaku,
            commands::get_comments,
            commands::post_comment,
            commands::send_danmaku,
            commands::get_action_state,
            commands::like_video,
            commands::coin_video,
            commands::triple_video,
            commands::follow_user,
            commands::is_following,
            commands::get_followings,
            commands::get_space_info,
            commands::get_space_videos,
            commands::report_heartbeat,
            commands::get_history,
            commands::delete_history,
            commands::clear_history,
            commands::get_toview,
            commands::add_toview,
            commands::delete_toview,
            commands::delete_viewed_toview,
            commands::clear_toview,
            commands::qr_login_start,
            commands::qr_login_poll,
            commands::get_decoder_probe,
            commands::log_player_event,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

// Set up `tracing` with two sinks: stderr (visible during `cargo tauri dev`)
// and a daily-rolling file under the platform log dir (the only signal a
// packaged Windows GUI build leaves behind, since there's no console).
// Returns the non-blocking writer's guard, which must outlive the app.
fn init_tracing(app: &tauri::AppHandle) -> Option<WorkerGuard> {
    // Filter precedence: `BILI_LOG` (app-specific) > `RUST_LOG` > built-in
    // default. Default is plain `info` — per-segment proxy logs and dash.js
    // fragment events are emitted at DEBUG and would otherwise dominate the
    // file. Bump with e.g. `BILI_LOG=bili_rust_lib=debug` (or `trace` for WBI
    // signing) to get them back when diagnosing.
    const DEFAULT_FILTER: &str = "info";
    if std::env::var_os("BILI_LOG").is_none() && std::env::var_os("RUST_LOG").is_none() {
        // Ship the effective default as an actual env var so it shows up in
        // `/proc/<pid>/environ`, is inherited by any child the app spawns, and
        // is discoverable for users who want to know what to override.
        std::env::set_var("BILI_LOG", DEFAULT_FILTER);
    }
    let env_filter = std::env::var("BILI_LOG")
        .ok()
        .and_then(|s| tracing_subscriber::EnvFilter::try_new(s).ok())
        .or_else(|| tracing_subscriber::EnvFilter::try_from_default_env().ok())
        .unwrap_or_else(|| tracing_subscriber::EnvFilter::new(DEFAULT_FILTER));
    let stderr_layer = tracing_subscriber::fmt::layer().with_writer(std::io::stderr);

    let log_dir = app.path().app_log_dir().ok();
    let (file_layer, guard) = match log_dir.as_ref() {
        Some(dir) => {
            if let Err(e) = std::fs::create_dir_all(dir) {
                eprintln!("warning: cannot create log dir {}: {e}", dir.display());
                (None, None)
            } else {
                let appender = tracing_appender::rolling::daily(dir, "bili-rust.log");
                let (nb, guard) = tracing_appender::non_blocking(appender);
                let layer = tracing_subscriber::fmt::layer()
                    .with_writer(nb)
                    .with_ansi(false);
                (Some(layer), Some(guard))
            }
        }
        None => (None, None),
    };

    let registry = tracing_subscriber::registry()
        .with(env_filter)
        .with(stderr_layer);
    if let Some(file_layer) = file_layer {
        registry.with(file_layer).init();
    } else {
        registry.init();
    }

    if let Some(dir) = log_dir {
        tracing::info!(log_dir = %dir.display(), "tracing initialized");
    } else {
        tracing::warn!("could not resolve app log dir; file logging disabled");
    }
    guard
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
