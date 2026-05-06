use crate::api;
use crate::components::video_card::{DislikeReason, VideoCardView};
use crate::state::RecommendState;
use crate::types::VideoCard;
use leptos::prelude::*;
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

/// CSV of `av_<aid>` for the last N cards already shown — server-side dedup.
fn last_showlist_csv(cards: &[VideoCard], n: usize) -> String {
    let start = cards.len().saturating_sub(n);
    cards[start..]
        .iter()
        .map(|c| format!("av_{}", c.aid))
        .collect::<Vec<_>>()
        .join(",")
}

#[component]
pub fn Home() -> impl IntoView {
    let state = use_context::<RecommendState>().expect("RecommendState context missing");
    let RecommendState {
        cards,
        fresh_idx,
        brush,
        loading,
        error,
        attempted,
        end_reached,
        scroll_y,
    } = state;

    // StoredValue::new_local lets us share a non-Send closure across reactive
    // contexts. Calling: load_more.with_value(|f| f()).
    let load_more = StoredValue::new_local(move || {
        if loading.get_untracked() || end_reached.get_untracked() {
            return;
        }
        loading.set(true);
        error.set(None);
        let idx = fresh_idx.get_untracked();
        let br = brush.get_untracked();
        let last = cards.with_untracked(|v| last_showlist_csv(v, 20));
        leptos::task::spawn_local(async move {
            match api::get_rcmd(idx, br, &last).await {
                Ok(mut more) => {
                    if more.is_empty() {
                        end_reached.set(true);
                    } else {
                        cards.update(|v| v.append(&mut more));
                        fresh_idx.update(|i| *i += 1);
                    }
                }
                Err(e) => error.set(Some(e)),
            }
            attempted.set(true);
            loading.set(false);
        });
    });

    // Initial load on first ever mount. The attempted guard means subsequent
    // remounts (back-nav from /watch) skip refetching — the previously loaded
    // cards in context are kept.
    Effect::new(move |_| {
        if !attempted.get() && !loading.get() {
            load_more.with_value(|f| f());
        }
    });

    // Restore scroll position when remounting (e.g. after back-nav). Runs once.
    Effect::new(move |prev: Option<()>| {
        if prev.is_some() {
            return;
        }
        let y = scroll_y.get_untracked();
        if y > 0.0 {
            if let Some(win) = web_sys::window() {
                win.scroll_to_with_x_and_y(0.0, y);
            }
        }
    });

    // Save scroll position on unmount so the next mount can restore it.
    on_cleanup(move || {
        if let Some(win) = web_sys::window() {
            scroll_y.set(win.scroll_y().unwrap_or(0.0));
        }
    });

    let sentinel_ref = NodeRef::<leptos::html::Div>::new();
    // Tracks the sentinel's current visibility. Updated by the observer.
    let sentinel_visible = RwSignal::new(false);

    // Observer just records whether the sentinel is in view. The decision to
    // call load_more lives in a separate Effect so it can react to the loading
    // flag flipping back to false (which the observer wouldn't otherwise see —
    // IntersectionObserver only fires on state transitions, so a sentinel
    // that's already in view stays silent until you scroll).
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

    // Drive load_more whenever the sentinel is visible and we're idle. Re-runs
    // when loading flips false after a fetch, so short feeds keep loading until
    // the viewport is filled or end_reached.
    Effect::new(move |_| {
        if sentinel_visible.get()
            && !loading.get()
            && !end_reached.get()
            && error.with(|e| e.is_none())
        {
            load_more.with_value(|f| f());
        }
    });

    let on_reset = move |_| {
        if loading.get_untracked() {
            return;
        }
        state.reset();
        if let Some(win) = web_sys::window() {
            win.scroll_to_with_x_and_y(0.0, 0.0);
        }
        load_more.with_value(|f| f());
    };

    let on_rotate = move |_| {
        if loading.get_untracked() {
            return;
        }
        state.rotate();
        if let Some(win) = web_sys::window() {
            win.scroll_to_with_x_and_y(0.0, 0.0);
        }
        load_more.with_value(|f| f());
    };

    let on_dislike: Callback<(VideoCard, DislikeReason)> =
        Callback::new(move |(card, reason): (VideoCard, DislikeReason)| {
            // Optimistic remove — keep UI snappy. If the API call fails the
            // user can refresh to bring the item back.
            cards.update(|v| v.retain(|c| c.bvid != card.bvid));
            let aid = card.aid;
            let (mid, rid, tag_id, reason_id) = match reason {
                DislikeReason::Content => (None, None, None, 1u32),
                DislikeReason::Up => (Some(card.up_mid), None, None, 2),
                DislikeReason::Category => (None, card.tid, None, 3),
            };
            leptos::task::spawn_local(async move {
                if let Err(e) = api::feed_dislike("av", aid, mid, rid, tag_id, reason_id).await {
                    web_sys::console::warn_1(&format!("dislike failed: {e}").into());
                }
            });
        });

    view! {
        <div>
            <div class="feed-toolbar">
                <button
                    class="refresh"
                    on:click=on_rotate
                    disabled=move || loading.get()
                    title="换一换 — 保留会话，请求新的一页"
                >
                    "↻ 换一换"
                </button>
                <button
                    class="refresh"
                    on:click=on_reset
                    disabled=move || loading.get()
                    title="重置 — 清空当前会话，从头开始"
                >
                    "⟲ 重置"
                </button>
            </div>
            <div class="grid">
                <For
                    each=move || cards.get()
                    key=|c: &VideoCard| c.bvid.clone()
                    children=move |c| view! { <VideoCardView card=c on_dislike=on_dislike /> }
                />
            </div>
            {move || (attempted.get() && cards.with(|c| c.is_empty()) && error.with(|e| e.is_none()))
                .then(|| view! { <div class="empty">"No videos returned."</div> })}
            {move || error.get().map(|e| view! {
                <div class="error">
                    {e}
                    <button on:click=move |_| {
                        error.set(None);
                        load_more.with_value(|f| f());
                    }>"Retry"</button>
                </div>
            })}
            {move || loading.get().then(|| view! { <div class="loading">"Loading…"</div> })}
            <div node_ref=sentinel_ref class="scroll-sentinel"></div>
        </div>
    }
}
