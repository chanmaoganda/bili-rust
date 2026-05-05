use crate::api;
use crate::components::comments::Comments;
use crate::components::danmaku::DanmakuOverlay;
use crate::components::player::Player;
use crate::components::video_card::VideoCardView;
use crate::types::{fmt_ctime, fmt_views, VideoCard};
use leptos::prelude::*;
use leptos_router::hooks::{use_params_map, use_query_map};
use wasm_bindgen::JsCast;
use web_sys::{Element, HtmlInputElement, HtmlSelectElement, HtmlVideoElement};

#[component]
pub fn Watch() -> impl IntoView {
    let params = use_params_map();
    let query = use_query_map();
    let bvid = Signal::derive(move || params.read().get("bvid").unwrap_or_default());
    let cid = Signal::derive(move || {
        query.read().get("cid").and_then(|s| s.parse::<i64>().ok())
    });

    let qn = RwSignal::new(crate::prefs::get_preferred_qn());
    let resume_at = RwSignal::new(0.0_f64);

    // Reset resume position when navigating to a different video, and re-apply
    // the saved quality preference so it survives across videos.
    Effect::new(move |_| {
        let _ = bvid.get();
        resume_at.set(0.0);
        qn.set(crate::prefs::get_preferred_qn());
    });

    let play = LocalResource::new(move || {
        let bv = bvid.get();
        let c = cid.get();
        let q = qn.get();
        async move { api::get_play_info(&bv, c, q).await }
    });
    let related = LocalResource::new(move || {
        let bv = bvid.get();
        async move { api::get_related(&bv).await }
    });
    let view_info = LocalResource::new(move || {
        let bv = bvid.get();
        async move { api::get_view_info(&bv).await }
    });

    let on_time = Callback::new(move |t: f64| resume_at.set(t));

    // Danmaku state — survives quality switches because it keys on cid only.
    let video_sig = RwSignal::<Option<HtmlVideoElement>>::new(None);
    let shell_sig = RwSignal::<Option<Element>>::new(None);
    let dm_enabled = RwSignal::new(crate::prefs::get_danmaku_enabled());
    let dm_opacity = RwSignal::new(crate::prefs::get_danmaku_opacity());
    let danmaku_cid = Signal::derive(move || {
        // Guard against stale PlayInfo during a bvid switch: LocalResource keeps
        // returning the previous Ok value while refetching, which would let the
        // old video's danmaku list keep playing on the new <video>. Filter by bvid.
        let current_bv = bvid.get();
        match play.get().and_then(|r| r.ok()) {
            Some(p) if p.bvid == current_bv => Some(p.cid),
            Some(_) => None,
            None => cid.get(),
        }
    });
    let danmakus = LocalResource::new(move || {
        let c = danmaku_cid.get();
        async move {
            match c {
                Some(c) => api::get_danmaku(c).await.ok(),
                None => None,
            }
        }
    });
    let dm_signal: Signal<Option<Vec<crate::types::Danmaku>>> =
        Signal::derive(move || danmakus.get().and_then(|x| x));

    view! {
        <div class="watch">
            <div>
                {move || match play.get() {
                    None => view! { <div class="loading">"Loading player…"</div> }.into_any(),
                    Some(res) => match res {
                        Ok(info) => {
                            // Fall back to PlayInfo only when the dedicated /view fetch hasn't returned;
                            // PlayInfo's fast path leaves these empty, so view_info is the real source.
                            let fallback_title = info.title.clone();
                            let fallback_up_face = info.up_face.clone();
                            let fallback_up_name = info.up_name.clone();
                            let title_view = move || {
                                view_info
                                    .get()
                                    .and_then(|r| r.ok())
                                    .map(|v| v.title)
                                    .filter(|s| !s.is_empty())
                                    .unwrap_or_else(|| fallback_title.clone())
                            };
                            let up_face_view = move || {
                                view_info
                                    .get()
                                    .and_then(|r| r.ok())
                                    .map(|v| v.up_face)
                                    .filter(|s| !s.is_empty())
                                    .unwrap_or_else(|| fallback_up_face.clone())
                            };
                            let up_name_view = move || {
                                view_info
                                    .get()
                                    .and_then(|r| r.ok())
                                    .map(|v| v.up_name)
                                    .filter(|s| !s.is_empty())
                                    .unwrap_or_else(|| fallback_up_name.clone())
                            };
                            let qualities: Vec<(u32, String)> = info
                                .accept_quality
                                .iter()
                                .copied()
                                .zip(info.accept_description.iter().cloned())
                                .collect();
                            let current = info.current_quality;
                            let start = resume_at.get_untracked();
                            let shell_ref = NodeRef::<leptos::html::Div>::new();
                            Effect::new(move |_| {
                                if let Some(el) = shell_ref.get() {
                                    let e: Element = el.unchecked_into();
                                    shell_sig.set(Some(e));
                                }
                            });
                            view! {
                                <div class="player-shell" node_ref=shell_ref>
                                    <Player
                                        info=info
                                        start_at=start
                                        on_time=on_time
                                        video_out=video_sig
                                        fullscreen_target=shell_sig
                                    />
                                    <DanmakuOverlay
                                        video=video_sig
                                        danmakus=dm_signal
                                        enabled=dm_enabled.into()
                                        opacity=dm_opacity.into()
                                    />
                                </div>
                                <div class="player-bar">
                                    <h1>{title_view}</h1>
                                    <button
                                        class="dm-toggle"
                                        on:click=move |_| {
                                            let next = !dm_enabled.get();
                                            crate::prefs::set_danmaku_enabled(next);
                                            dm_enabled.set(next);
                                        }
                                    >
                                        {move || if dm_enabled.get() { "弹幕 开" } else { "弹幕 关" }}
                                    </button>
                                    <input
                                        class="dm-opacity"
                                        type="range"
                                        min="0.2"
                                        max="1"
                                        step="0.05"
                                        prop:value=move || dm_opacity.get().to_string()
                                        on:input=move |ev| {
                                            if let Some(inp) = ev.target().and_then(|t| t.dyn_into::<HtmlInputElement>().ok()) {
                                                if let Ok(v) = inp.value().parse::<f64>() {
                                                    crate::prefs::set_danmaku_opacity(v);
                                                    dm_opacity.set(v);
                                                }
                                            }
                                        }
                                    />
                                    {(!qualities.is_empty()).then(|| view! {
                                        <select
                                            class="quality"
                                            on:change=move |ev| {
                                                let target = ev.target().and_then(|t| t.dyn_into::<HtmlSelectElement>().ok());
                                                if let Some(sel) = target {
                                                    if let Ok(v) = sel.value().parse::<u32>() {
                                                        crate::prefs::set_preferred_qn(v);
                                                        qn.set(Some(v));
                                                    }
                                                }
                                            }
                                        >
                                            {qualities.into_iter().map(|(q, label)| {
                                                let selected = q == current;
                                                view! {
                                                    <option value=q.to_string() selected=selected>{label}</option>
                                                }
                                            }).collect_view()}
                                        </select>
                                    })}
                                </div>
                                <div class="up">
                                    <img src=up_face_view alt="" />
                                    <span>{up_name_view}</span>
                                </div>
                                <div class="info">
                                    {move || view_info.get().and_then(|r| r.ok()).map(|v| view! {
                                        <div class="stats">
                                            <span>{fmt_views(v.view)}" 播放"</span>
                                            <span>{fmt_views(v.danmaku)}" 弹幕"</span>
                                            <span>{fmt_views(v.like)}" 点赞"</span>
                                            <span>{fmt_views(v.coin)}" 投币"</span>
                                            <span>{fmt_views(v.favorite)}" 收藏"</span>
                                            <span>{fmt_views(v.share)}" 分享"</span>
                                            <span class="meta">
                                                {fmt_ctime(v.pubdate)}" · "{v.tname.clone()}
                                            </span>
                                        </div>
                                        {(!v.desc.is_empty()).then(|| view! {
                                            <pre class="desc">{v.desc.clone()}</pre>
                                        })}
                                    })}
                                </div>
                            }
                                .into_any()
                        }
                        Err(e) => view! { <div class="error">{e}</div> }.into_any(),
                    }
                }}
                <Comments bvid=bvid />
            </div>
            <aside class="related">
                {move || match related.get() {
                    None => view! { <div class="loading">"…"</div> }.into_any(),
                    Some(res) => match res {
                        Ok(items) => {
                            view! {
                                <For
                                    each=move || items.clone()
                                    key=|c: &VideoCard| c.bvid.clone()
                                    children=move |c| view! { <VideoCardView card=c /> }
                                />
                            }
                                .into_any()
                        }
                        Err(e) => view! { <div class="error">{e}</div> }.into_any(),
                    }
                }}
            </aside>
        </div>
    }
}
