use crate::types::{fmt_duration, fmt_views, VideoCard};
use leptos::prelude::*;
use leptos_router::components::A;

#[component]
pub fn VideoCardView(card: VideoCard) -> impl IntoView {
    let href = if card.cid != 0 {
        format!("/watch/{}?cid={}", card.bvid, card.cid)
    } else {
        format!("/watch/{}", card.bvid)
    };
    view! {
        <A href=href>
            <div class="card">
                <img class="cover" src=card.pic.clone() loading="lazy" alt="" />
                <div class="meta">
                    <div class="title">{card.title.clone()}</div>
                    <div class="sub">
                        <span>{card.up_name.clone()}</span>
                        <span>
                            {fmt_views(card.view)} " · " {fmt_duration(card.duration)}
                        </span>
                    </div>
                </div>
            </div>
        </A>
    }
}
