#![allow(unused_parens)]

use crate::api;
use crate::components::video_card::VideoCardView;
use crate::types::{fmt_ctime, ToviewPage, VideoCard};
use leptos::prelude::*;
use leptos::task::spawn_local;

fn confirm_or(msg: &str) -> bool {
    web_sys::window()
        .and_then(|w| w.confirm_with_message(msg).ok())
        .unwrap_or(false)
}

fn progress_label(progress: Option<i64>, duration: i64) -> String {
    match progress {
        None => String::new(),
        Some(p) if p < 0 => "已看完".to_string(),
        Some(0) => String::new(),
        Some(p) if duration > 0 => {
            let pct = (p * 100 / duration).clamp(0, 100);
            format!("已看 {pct}%")
        }
        Some(_) => String::new(),
    }
}

#[component]
pub fn Toview() -> impl IntoView {
    let cards = RwSignal::new(Vec::<VideoCard>::new());
    let count = RwSignal::new(0i32);
    let loading = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let attempted = RwSignal::new(false);

    let load = StoredValue::new_local(move || {
        if loading.get_untracked() {
            return;
        }
        loading.set(true);
        error.set(None);
        spawn_local(async move {
            match api::get_toview().await {
                Ok(ToviewPage { count: n, list }) => {
                    cards.set(list);
                    count.set(n);
                }
                Err(e) => error.set(Some(e)),
            }
            attempted.set(true);
            loading.set(false);
        });
    });

    Effect::new(move |_| {
        if !attempted.get() && !loading.get() {
            load.with_value(|f| f());
        }
    });

    let on_remove: Callback<i64> = Callback::new(move |aid: i64| {
        cards.update(|v| v.retain(|c| c.aid != aid));
        count.update(|n| *n = (*n - 1).max(0));
        spawn_local(async move {
            if let Err(e) = api::delete_toview(aid).await {
                web_sys::console::warn_1(&format!("delete_toview: {e}").into());
            }
        });
    });

    let on_clear = move |_| {
        if !confirm_or("确认清空稍后再看？此操作不可撤销。") {
            return;
        }
        spawn_local(async move {
            match api::clear_toview().await {
                Ok(()) => {
                    cards.set(Vec::new());
                    count.set(0);
                    error.set(None);
                }
                Err(e) => {
                    web_sys::console::error_1(&format!("clear_toview: {e}").into());
                    error.set(Some(e));
                }
            }
        });
    };

    let on_remove_viewed = move |_| {
        if !confirm_or("移除全部已看完的视频？") {
            return;
        }
        spawn_local(async move {
            match api::delete_viewed_toview().await {
                Ok(()) => {
                    // Reload — server is the source of truth for which rows
                    // counted as "viewed" (progress thresholds vary).
                    load.with_value(|f| f());
                }
                Err(e) => {
                    web_sys::console::error_1(&format!("delete_viewed: {e}").into());
                    error.set(Some(e));
                }
            }
        });
    };

    view! {
        <div class="toview-page">
            <div class="page-toolbar">
                <h1>"稍后再看 "{move || {
                    let n = count.get();
                    if n > 0 { format!("· {n}") } else { String::new() }
                }}</h1>
                <button on:click=on_remove_viewed>"移除已看完"</button>
                <button class="danger" on:click=on_clear>"清空全部"</button>
            </div>
            <div class="grid">
                <For
                    each=move || cards.get()
                    key=|c: &VideoCard| c.bvid.clone()
                    children=move |c| {
                        let aid = c.aid;
                        let add_at = c.add_at.unwrap_or(0);
                        let pl = progress_label(c.progress, c.duration);
                        view! {
                            <div class="list-row">
                                <VideoCardView card=c />
                                <div class="row-extra">
                                    {(add_at > 0).then(|| view! {
                                        <span class="row-time">"添加 "{fmt_ctime(add_at)}</span>
                                    })}
                                    {(!pl.is_empty()).then(|| view! {
                                        <span class="row-progress">{pl}</span>
                                    })}
                                    <button
                                        class="row-remove"
                                        title="从稍后再看移除"
                                        on:click=move |ev: web_sys::MouseEvent| {
                                            ev.stop_propagation();
                                            ev.prevent_default();
                                            on_remove.run(aid);
                                        }
                                    >"✕"</button>
                                </div>
                            </div>
                        }
                    }
                />
            </div>
            {move || error.get().map(|e| view! {
                <div class="error">
                    {e}
                    <button on:click=move |_| {
                        error.set(None);
                        load.with_value(|f| f());
                    }>"重试"</button>
                </div>
            })}
            {move || loading.get().then(|| view! { <div class="loading">"加载中…"</div> })}
            {move || (attempted.get() && cards.with(|c| c.is_empty())
                && !loading.get() && error.with(|e| e.is_none()))
                .then(|| view! { <div class="empty">"稍后再看为空"</div> })}
        </div>
    }
}
