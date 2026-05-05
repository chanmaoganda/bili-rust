use crate::api;
use crate::components::video_card::VideoCardView;
use crate::types::VideoCard;
use leptos::prelude::*;

#[component]
pub fn Home() -> impl IntoView {
    let cards = RwSignal::new(Vec::<VideoCard>::new());
    let fresh_idx = RwSignal::new(0u32);
    let loading = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let attempted = RwSignal::new(false);

    let load_more = move || {
        if loading.get() {
            return;
        }
        loading.set(true);
        error.set(None);
        let idx = fresh_idx.get();
        leptos::task::spawn_local(async move {
            match api::get_rcmd(idx).await {
                Ok(mut more) => {
                    cards.update(|v| v.append(&mut more));
                    fresh_idx.update(|i| *i += 1);
                }
                Err(e) => error.set(Some(e)),
            }
            attempted.set(true);
            loading.set(false);
        });
    };

    // initial load — fire once. Without the attempted guard this re-fires
    // forever when the API returns zero items.
    Effect::new({
        let load_more = load_more.clone();
        move |_| {
            if !attempted.get() && !loading.get() {
                load_more();
            }
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
            {move || error.get().map(|e| view! { <div class="error">{e}</div> })}
            {move || (attempted.get() && cards.with(|c| c.is_empty()) && error.with(|e| e.is_none()))
                .then(|| view! { <div class="empty">"No videos returned."</div> })}
            <button
                class="load-more"
                on:click=move |_| load_more()
                disabled=move || loading.get()
            >
                {move || if loading.get() { "Loading…" } else { "Load more" }}
            </button>
        </div>
    }
}
