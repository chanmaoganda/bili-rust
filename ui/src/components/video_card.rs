use crate::types::{fmt_duration, fmt_views, VideoCard};
use leptos::prelude::*;
use leptos_router::components::A;

#[derive(Clone, Copy, Debug)]
pub enum DislikeReason {
    /// reason_id=1 — content not interesting (no extra params).
    Content,
    /// reason_id=2 — UP不再推荐 (sends mid).
    Up,
    /// reason_id=3 — 分区不再推荐 (sends rid=tid).
    Category,
}

#[component]
pub fn VideoCardView(
    card: VideoCard,
    /// Optional dislike menu callback. When `Some`, a ⋯ button appears on hover
    /// with three menu options (内容/UP主/分区). When `None` (e.g. related sidebar),
    /// no menu is rendered.
    #[prop(optional, into)]
    on_dislike: Option<Callback<(VideoCard, DislikeReason)>>,
) -> impl IntoView {
    // If the row carries a saved progress (history / toview), append `?t=` so
    // the watch route resumes at that position. Bilibili's `progress` is -1
    // when the video was finished — skip those.
    let resume_t = card.progress.filter(|p| *p > 0);
    let href = match (card.cid != 0, resume_t) {
        (true, Some(t)) => format!("/watch/{}?cid={}&t={}", card.bvid, card.cid, t),
        (true, None) => format!("/watch/{}?cid={}", card.bvid, card.cid),
        (false, Some(t)) => format!("/watch/{}?t={}", card.bvid, t),
        (false, None) => format!("/watch/{}", card.bvid),
    };
    let menu_open = RwSignal::new(false);

    let card_for_link = card.clone();
    let tname_for_badge = card.tname.clone();
    let rcmd_reason = card.rcmd_reason.clone();

    // Build the menu view if dislike support is enabled. Handlers are
    // re-created inside the reactive closure so the outer `move ||` stays Fn
    // (each open/close re-render gets fresh single-shot click closures).
    let menu_view = on_dislike.map(|cb| {
        let card_outer = card.clone();
        let on_btn = move |ev: web_sys::MouseEvent| {
            ev.stop_propagation();
            ev.prevent_default();
            menu_open.update(|v| *v = !*v);
        };

        view! {
            <button
                class="card-menu-btn"
                class:open=move || menu_open.get()
                on:click=on_btn
                title="不感兴趣"
            >
                "⋯"
            </button>
            {move || {
                if !menu_open.get() {
                    return None;
                }
                let card_a = card_outer.clone();
                let card_b = card_outer.clone();
                let card_c = card_outer.clone();
                let up_label = format!("UP「{}」不再推荐", card_outer.up_name);
                let cat_label = card_outer
                    .tname
                    .as_deref()
                    .map(|t| format!("分区「{t}」不再推荐"))
                    .unwrap_or_else(|| "分区不再推荐".to_string());
                let has_up = card_outer.up_mid != 0;
                let has_category = card_outer.tid.is_some();

                let close = move |ev: web_sys::MouseEvent| {
                    ev.stop_propagation();
                    ev.prevent_default();
                    menu_open.set(false);
                };
                let on_content = move |ev: web_sys::MouseEvent| {
                    ev.stop_propagation();
                    ev.prevent_default();
                    menu_open.set(false);
                    cb.run((card_a.clone(), DislikeReason::Content));
                };
                let on_up = move |ev: web_sys::MouseEvent| {
                    ev.stop_propagation();
                    ev.prevent_default();
                    menu_open.set(false);
                    cb.run((card_b.clone(), DislikeReason::Up));
                };
                let on_category = move |ev: web_sys::MouseEvent| {
                    ev.stop_propagation();
                    ev.prevent_default();
                    menu_open.set(false);
                    cb.run((card_c.clone(), DislikeReason::Category));
                };
                let stop = move |ev: web_sys::MouseEvent| ev.stop_propagation();

                Some(view! {
                    <div class="card-menu-backdrop" on:click=close></div>
                    <div class="card-menu" on:click=stop>
                        <button on:click=on_content>"不感兴趣"</button>
                        {has_up.then(|| view! {
                            <button on:click=on_up>{up_label}</button>
                        })}
                        {has_category.then(|| view! {
                            <button on:click=on_category>{cat_label}</button>
                        })}
                    </div>
                })
            }}
        }
    });

    view! {
        <div class="card-wrap">
            <A href=href>
                <div class="card">
                    <img class="cover" src=card_for_link.pic.clone() loading="lazy" alt="" />
                    {tname_for_badge.map(|t| view! { <span class="card-tname">{t}</span> })}
                    <div class="meta">
                        <div class="title">{card_for_link.title.clone()}</div>
                        {rcmd_reason.map(|r| view! { <div class="card-reason">{r}</div> })}
                        <div class="sub">
                            <span>{card_for_link.up_name.clone()}</span>
                            <span>
                                {fmt_views(card_for_link.view)} " · " {fmt_duration(card_for_link.duration)}
                            </span>
                        </div>
                    </div>
                </div>
            </A>
            {menu_view}
        </div>
    }
}
