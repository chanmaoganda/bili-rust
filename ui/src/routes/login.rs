use crate::api;
use crate::state::LoginVersion;
use crate::types::QrStart;
use leptos::prelude::*;
use leptos_router::hooks::use_navigate;
use leptos_router::NavigateOptions;
use qrcode::{Color, QrCode};
use std::time::Duration;

#[component]
pub fn Login() -> impl IntoView {
    let version = use_context::<LoginVersion>().expect("LoginVersion context missing");
    let qr = RwSignal::new(None::<Result<QrStart, String>>);
    let status = RwSignal::new("等待二维码…".to_string());
    // Bumped each time the user hits "刷新二维码"; the polling effect re-runs
    // when this changes so an expired code starts a fresh session.
    let regenerate = RwSignal::new(0u32);

    Effect::new(move |_| {
        let _ = regenerate.get();
        qr.set(None);
        status.set("等待二维码…".to_string());
        leptos::task::spawn_local(async move {
            match api::qr_login_start().await {
                Ok(s) => {
                    status.set("请使用 Bilibili 手机端扫码".to_string());
                    qr.set(Some(Ok(s)));
                }
                Err(e) => qr.set(Some(Err(e))),
            }
        });
    });

    // Poll loop: only runs while the active QR has a key and isn't confirmed.
    Effect::new(move |_| {
        let key = match qr.get() {
            Some(Ok(s)) => s.qrcode_key,
            _ => return,
        };
        let nav = use_navigate();
        leptos::task::spawn_local(async move {
            loop {
                gloo_timers::future::sleep(Duration::from_secs(2)).await;
                match api::qr_login_poll(&key).await {
                    Ok(p) => match p.status.as_str() {
                        "scanning" => status.set("请使用 Bilibili 手机端扫码".to_string()),
                        "scanned" => status.set("已扫描,请在手机端确认".to_string()),
                        "expired" => {
                            status.set("二维码已过期,请刷新".to_string());
                            return;
                        }
                        "confirmed" => {
                            status.set("登录成功".to_string());
                            // Tell the rest of the app to refresh anything keyed
                            // on the logged-in user (Header avatar, Space "me"
                            // check, …) before we navigate away.
                            version.bump();
                            nav("/", NavigateOptions::default());
                            return;
                        }
                        other => status.set(format!("未知状态: {other}")),
                    },
                    Err(e) => {
                        status.set(format!("轮询失败: {e}"));
                        return;
                    }
                }
            }
        });
    });

    let refresh = move |_| regenerate.update(|v| *v += 1);

    view! {
        <div class="login-page">
            <h1>"扫码登录"</h1>
            {move || match qr.get() {
                None => view! { <div class="loading">"加载二维码…"</div> }.into_any(),
                Some(Err(e)) => view! {
                    <div class="error">{e}
                        <button on:click=refresh>"重试"</button>
                    </div>
                }.into_any(),
                Some(Ok(s)) => view! {
                    <div class="qr-box" inner_html=qr_svg(&s.url)></div>
                    <div class="qr-payload">{s.url.clone()}</div>
                }.into_any(),
            }}
            <div class="qr-status">{move || status.get()}</div>
            <button class="refresh" on:click=refresh>"刷新二维码"</button>
        </div>
    }
}

/// Render a QR-code payload to an inline SVG string. Modules are 8px each
/// with a 4-module quiet zone, scaling to ~232px for a typical Bilibili QR.
fn qr_svg(payload: &str) -> String {
    let code = match QrCode::new(payload.as_bytes()) {
        Ok(c) => c,
        Err(_) => return String::from("<div class='error'>QR encode failed</div>"),
    };
    let modules = code.width();
    let scale: u32 = 6;
    let quiet: u32 = 4;
    let total = (modules as u32 + quiet * 2) * scale;

    let colors = code.to_colors();
    let mut rects = String::new();
    for y in 0..modules {
        for x in 0..modules {
            if matches!(colors[y * modules + x], Color::Dark) {
                let cx = (x as u32 + quiet) * scale;
                let cy = (y as u32 + quiet) * scale;
                rects.push_str(&format!(
                    "<rect x='{cx}' y='{cy}' width='{scale}' height='{scale}' fill='black'/>"
                ));
            }
        }
    }
    format!(
        "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 {total} {total}' \
         width='{total}' height='{total}' style='background:#fff;border-radius:8px;'>\
         {rects}</svg>"
    )
}
