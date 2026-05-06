use crate::api;
use crate::keys::{is_typing, KeydownGuard};
use crate::state::LoginVersion;
use leptos::prelude::*;
use leptos_router::hooks::use_navigate;
use leptos_router::NavigateOptions;
use web_sys::KeyboardEvent;

#[component]
pub fn NavShortcuts() -> impl IntoView {
    let version = use_context::<LoginVersion>().expect("LoginVersion context missing");

    let me = RwSignal::new((false, 0i64));
    let user = LocalResource::new(move || {
        let _ = version.get();
        async { api::get_user_info().await }
    });
    Effect::new(move |_| {
        user.get().map(|res| match res {
            Ok(u) if u.is_login => me.set((true, u.mid)),
            Ok(_) => me.set((false, 0)),
            Err(_) => {}
        });
    });

    Effect::new(
        move |_prev: Option<Option<KeydownGuard>>| -> Option<KeydownGuard> {
            let document = web_sys::window()?.document()?;
            let nav = use_navigate();
            KeydownGuard::install(move |ev: KeyboardEvent| {
                if ev.ctrl_key() || ev.meta_key() {
                    return;
                }
                if is_typing(&document) {
                    return;
                }
                let key = ev.key();
                if ev.alt_key() {
                    let path: Option<String> = match key.as_str() {
                        "1" => Some("/".to_string()),
                        "2" => Some("/history".to_string()),
                        "3" => Some("/toview".to_string()),
                        "4" => {
                            let (logged, mid) = me.get_untracked();
                            Some(if logged {
                                format!("/space/{mid}")
                            } else {
                                "/login".to_string()
                            })
                        }
                        "5" => Some("/login".to_string()),
                        _ => None,
                    };
                    if let Some(p) = path {
                        ev.prevent_default();
                        nav(&p, NavigateOptions::default());
                    }
                    return;
                }
                if key == "Escape" {
                    if document.fullscreen_element().is_some() {
                        return;
                    }
                    if let Some(history) = web_sys::window().and_then(|w| w.history().ok()) {
                        ev.prevent_default();
                        let _ = history.back();
                    }
                }
            })
        },
    );
}
