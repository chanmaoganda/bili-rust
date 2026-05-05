use crate::api;
use crate::components::video_card::VideoCardView;
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

#[component]
pub fn Home() -> impl IntoView {
    let cards = RwSignal::new(Vec::<VideoCard>::new());
    let fresh_idx = RwSignal::new(0u32);
    let loading = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let attempted = RwSignal::new(false);
    let end_reached = RwSignal::new(false);

    // StoredValue::new_local lets us share a non-Send closure across reactive
    // contexts. Calling: load_more.with_value(|f| f()).
    let load_more = StoredValue::new_local(move || {
        if loading.get_untracked() || end_reached.get_untracked() {
            return;
        }
        loading.set(true);
        error.set(None);
        let idx = fresh_idx.get_untracked();
        leptos::task::spawn_local(async move {
            match api::get_rcmd(idx).await {
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

    // Initial load — fire once. The attempted guard prevents re-firing after
    // an empty response.
    Effect::new(move |_| {
        if !attempted.get() && !loading.get() {
            load_more.with_value(|f| f());
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
    Effect::new(move |_prev: Option<Option<ObserverGuard>>| -> Option<ObserverGuard> {
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
            IntersectionObserver::new_with_options(closure.as_ref().unchecked_ref(), &init).ok()?;
        observer.observe(&sentinel_el);

        Some(ObserverGuard {
            observer,
            _closure: closure,
        })
    });

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

    view! {
        <div>
            <div class="grid">
                <For
                    each=move || cards.get()
                    key=|c: &VideoCard| c.bvid.clone()
                    children=move |c| view! { <VideoCardView card=c /> }
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
