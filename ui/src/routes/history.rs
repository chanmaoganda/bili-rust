// `view!` macro misparses bare `>` in closure bodies as JSX tag-close, so a
// few `class:` / `prop:` bindings need explicit parens that rustc then flags.
#![allow(unused_parens)]

use crate::api;
use crate::components::video_card::VideoCardView;
use crate::types::{fmt_ctime, HistoryPage, VideoCard};
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{IntersectionObserver, IntersectionObserverEntry, IntersectionObserverInit};

struct ObserverGuard {
    observer: IntersectionObserver,
    _closure: Closure<dyn FnMut(js_sys::Array, IntersectionObserver)>,
}

impl Drop for ObserverGuard {
    fn drop(&mut self) {
        self.observer.disconnect();
    }
}

fn confirm_or(msg: &str) -> bool {
    web_sys::window()
        .and_then(|w| w.confirm_with_message(msg).ok())
        .unwrap_or(false)
}

fn progress_label(progress: Option<i64>, duration: i64) -> String {
    match progress {
        None => String::new(),
        Some(p) if p < 0 => "已看完".to_string(),
        Some(0) => "未观看".to_string(),
        Some(p) if duration > 0 => {
            let pct = (p * 100 / duration).clamp(0, 100);
            format!("已看 {pct}%")
        }
        Some(_) => String::new(),
    }
}

#[component]
pub fn History() -> impl IntoView {
    let cards = RwSignal::new(Vec::<VideoCard>::new());
    let cursor_max = RwSignal::new(0i64);
    let cursor_view_at = RwSignal::new(0i64);
    let loading = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let attempted = RwSignal::new(false);
    let end_reached = RwSignal::new(false);

    let load_more = StoredValue::new_local(move || {
        if loading.get_untracked() || end_reached.get_untracked() {
            return;
        }
        loading.set(true);
        error.set(None);
        let m = cursor_max.get_untracked();
        let v = cursor_view_at.get_untracked();
        spawn_local(async move {
            match api::get_history(m, v).await {
                Ok(HistoryPage {
                    mut list,
                    cursor_max: nm,
                    cursor_view_at: nv,
                    has_more,
                }) => {
                    if list.is_empty() {
                        end_reached.set(true);
                    } else {
                        cards.update(|cs| cs.append(&mut list));
                        cursor_max.set(nm);
                        cursor_view_at.set(nv);
                        if !has_more {
                            end_reached.set(true);
                        }
                    }
                }
                Err(e) => error.set(Some(e)),
            }
            attempted.set(true);
            loading.set(false);
        });
    });

    Effect::new(move |_| {
        if !attempted.get() && !loading.get() {
            load_more.with_value(|f| f());
        }
    });

    let sentinel_ref = NodeRef::<leptos::html::Div>::new();
    let sentinel_visible = RwSignal::new(false);
    Effect::new(
        move |_prev: Option<Option<ObserverGuard>>| -> Option<ObserverGuard> {
            let sentinel = sentinel_ref.get()?;
            let sentinel_el: web_sys::Element = sentinel.unchecked_into();
            let closure = Closure::<dyn FnMut(js_sys::Array, IntersectionObserver)>::new(
                move |entries: js_sys::Array, _obs: IntersectionObserver| {
                    let intersecting = entries
                        .iter()
                        .filter_map(|v| v.dyn_into::<IntersectionObserverEntry>().ok())
                        .any(|e| e.is_intersecting());
                    sentinel_visible.set(intersecting);
                },
            );
            let init = IntersectionObserverInit::new();
            init.set_root_margin("300px");
            let observer =
                IntersectionObserver::new_with_options(closure.as_ref().unchecked_ref(), &init)
                    .ok()?;
            observer.observe(&sentinel_el);
            Some(ObserverGuard {
                observer,
                _closure: closure,
            })
        },
    );
    Effect::new(move |_| {
        if sentinel_visible.get()
            && !loading.get()
            && !end_reached.get()
            && error.with(|e| e.is_none())
        {
            load_more.with_value(|f| f());
        }
    });

    let on_remove: Callback<i64> = Callback::new(move |aid: i64| {
        cards.update(|v| v.retain(|c| c.aid != aid));
        spawn_local(async move {
            if let Err(e) = api::delete_history(aid).await {
                web_sys::console::warn_1(&format!("delete_history: {e}").into());
            }
        });
    });

    let on_clear = move |_| {
        if !confirm_or("确认清空全部历史记录？此操作不可撤销。") {
            return;
        }
        spawn_local(async move {
            match api::clear_history().await {
                Ok(()) => {
                    cards.set(Vec::new());
                    cursor_max.set(0);
                    cursor_view_at.set(0);
                    end_reached.set(true);
                    error.set(None);
                }
                Err(e) => {
                    web_sys::console::error_1(&format!("clear_history: {e}").into());
                    error.set(Some(e));
                }
            }
        });
    };

    view! {
        <div class="history-page">
            <div class="page-toolbar">
                <h1>"历史记录"</h1>
                <button class="danger" on:click=on_clear>"清空全部"</button>
            </div>
            <div class="grid">
                <For
                    each=move || cards.get()
                    key=|c: &VideoCard| format!("{}-{}", c.bvid, c.view_at.unwrap_or(0))
                    children=move |c| {
                        let aid = c.aid;
                        let view_at = c.view_at.unwrap_or(0);
                        let pl = progress_label(c.progress, c.duration);
                        view! {
                            <div class="list-row">
                                <VideoCardView card=c />
                                <div class="row-extra">
                                    {(view_at > 0).then(|| view! {
                                        <span class="row-time">"观看 "{fmt_ctime(view_at)}</span>
                                    })}
                                    {(!pl.is_empty()).then(|| view! {
                                        <span class="row-progress">{pl}</span>
                                    })}
                                    <button
                                        class="row-remove"
                                        title="从历史中移除"
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
                        load_more.with_value(|f| f());
                    }>"重试"</button>
                </div>
            })}
            {move || loading.get().then(|| view! { <div class="loading">"加载中…"</div> })}
            {move || (attempted.get() && cards.with(|c| c.is_empty()) && error.with(|e| e.is_none()))
                .then(|| view! { <div class="empty">"暂无历史记录"</div> })}
            <div node_ref=sentinel_ref class="scroll-sentinel"></div>
        </div>
    }
}
