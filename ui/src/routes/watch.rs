// `view!` macro misparses bare `>` in closure bodies as JSX tag-close, so a
// few `class:` / `prop:` bindings need explicit parens that rustc then flags.
#![allow(unused_parens)]

use crate::api;
use crate::components::comments::Comments;
use crate::components::danmaku::DanmakuOverlay;
use crate::components::stats_hud::StatsHud;
use crate::components::player::Player;
use crate::components::video_card::VideoCardView;
use crate::keys::{is_typing, KeydownGuard};
use crate::types::{fmt_ctime, fmt_views, ActionState, VideoCard};
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::A;
use leptos_router::hooks::{use_params_map, use_query_map};
use wasm_bindgen::JsCast;
use web_sys::{Element, HtmlInputElement, HtmlSelectElement, HtmlVideoElement, KeyboardEvent};

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
    let stats_hud_enabled = RwSignal::new(crate::prefs::get_stats_hud_enabled());
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

    // Action state — like/coin/favorite/follow. Lifted to component level so the
    // keyboard handler (A=triple) and the UI buttons share state, and so we can
    // optimistically toggle locally before the server round-trip.
    let action = RwSignal::new(ActionState::default());
    let like_delta = RwSignal::new(0i64);
    let coin_delta = RwSignal::new(0i64);
    let fav_delta = RwSignal::new(0i64);
    let action_pending = RwSignal::new(false);

    let coin_open = RwSignal::new(false);
    let coin_count = RwSignal::new(1u8);
    let coin_with_like = RwSignal::new(true);

    // up_mid: prefer view_info (authoritative) and fall back to play info.
    let up_mid = Signal::derive(move || {
        view_info
            .get()
            .and_then(|r| r.ok())
            .map(|v| v.up_mid)
            .filter(|m| *m > 0)
            .or_else(|| {
                play.get()
                    .and_then(|r| r.ok())
                    .map(|p| p.up_mid)
                    .filter(|m| *m > 0)
            })
            .unwrap_or(0)
    });

    // Reset action state + deltas + popover on bvid change.
    Effect::new(move |_| {
        let _ = bvid.get();
        action.set(ActionState::default());
        like_delta.set(0);
        coin_delta.set(0);
        fav_delta.set(0);
        coin_open.set(false);
    });

    // Fetch fresh action state whenever (bvid, up_mid) changes. We bridge the
    // async result into `action` instead of using LocalResource so the same
    // signal can be optimistically updated by click handlers.
    Effect::new(move |_| {
        let bv = bvid.get();
        let mid = up_mid.get();
        if bv.is_empty() || mid <= 0 {
            return;
        }
        let bv2 = bv.clone();
        spawn_local(async move {
            match api::get_action_state(&bv2, mid).await {
                Ok(s) => {
                    // Drop stale responses if the user already navigated away.
                    if bvid.get_untracked() == bv2 {
                        action.set(s);
                    }
                }
                Err(e) => web_sys::console::warn_1(
                    &format!("get_action_state: {e}").into(),
                ),
            }
        });
    });

    // Action callbacks — Copy `Callback`s so both buttons and keyboard share them.
    let do_like = Callback::new(move |_: ()| {
        if action_pending.get_untracked() {
            return;
        }
        let bv = bvid.get_untracked();
        if bv.is_empty() {
            return;
        }
        let next = !action.get_untracked().liked;
        action.update(|a| a.liked = next);
        like_delta.update(|d| *d += if next { 1 } else { -1 });
        action_pending.set(true);
        spawn_local(async move {
            let res = api::like_video(&bv, next).await;
            action_pending.set(false);
            if let Err(e) = res {
                web_sys::console::error_1(&format!("like_video: {e}").into());
                action.update(|a| a.liked = !next);
                like_delta.update(|d| *d -= if next { 1 } else { -1 });
            }
        });
    });

    let do_coin = Callback::new(move |(multiply, with_like): (u8, bool)| {
        if action_pending.get_untracked() {
            return;
        }
        if action.get_untracked().coined > 0 {
            return; // 投币不可撤销
        }
        let bv = bvid.get_untracked();
        if bv.is_empty() {
            return;
        }
        let m = multiply.clamp(1, 2);
        coin_open.set(false);
        let prev_liked = action.get_untracked().liked;
        action.update(|a| {
            a.coined = m as i64;
            if with_like {
                a.liked = true;
            }
        });
        coin_delta.update(|d| *d += m as i64);
        if with_like && !prev_liked {
            like_delta.update(|d| *d += 1);
        }
        action_pending.set(true);
        spawn_local(async move {
            let res = api::coin_video(&bv, m, with_like).await;
            action_pending.set(false);
            if let Err(e) = res {
                web_sys::console::error_1(&format!("coin_video: {e}").into());
                action.update(|a| {
                    a.coined = 0;
                    if with_like && !prev_liked {
                        a.liked = false;
                    }
                });
                coin_delta.update(|d| *d -= m as i64);
                if with_like && !prev_liked {
                    like_delta.update(|d| *d -= 1);
                }
            }
        });
    });

    let do_triple = Callback::new(move |_: ()| {
        if action_pending.get_untracked() {
            return;
        }
        let bv = bvid.get_untracked();
        if bv.is_empty() {
            return;
        }
        let prev = action.get_untracked();
        // Optimistic: assume all three succeed.
        action.update(|a| {
            a.liked = true;
            if a.coined == 0 {
                a.coined = 1;
            }
            a.favorited = true;
        });
        if !prev.liked {
            like_delta.update(|d| *d += 1);
        }
        if prev.coined == 0 {
            coin_delta.update(|d| *d += 1);
        }
        if !prev.favorited {
            fav_delta.update(|d| *d += 1);
        }
        action_pending.set(true);
        spawn_local(async move {
            let res = api::triple_video(&bv).await;
            action_pending.set(false);
            match res {
                Ok(r) => {
                    // Reconcile against per-action results — server may
                    // partially succeed (e.g. already-coined videos).
                    action.update(|a| {
                        if !r.like && !prev.liked {
                            a.liked = false;
                            like_delta.update(|d| *d -= 1);
                        }
                        if !r.coin && prev.coined == 0 {
                            a.coined = 0;
                            coin_delta.update(|d| *d -= 1);
                        }
                        if !r.fav && !prev.favorited {
                            a.favorited = false;
                            fav_delta.update(|d| *d -= 1);
                        }
                    });
                }
                Err(e) => {
                    web_sys::console::error_1(&format!("triple_video: {e}").into());
                    action.set(prev.clone());
                    if !prev.liked {
                        like_delta.update(|d| *d -= 1);
                    }
                    if prev.coined == 0 {
                        coin_delta.update(|d| *d -= 1);
                    }
                    if !prev.favorited {
                        fav_delta.update(|d| *d -= 1);
                    }
                }
            }
        });
    });

    let do_follow = Callback::new(move |_: ()| {
        if action_pending.get_untracked() {
            return;
        }
        let mid = up_mid.get_untracked();
        if mid <= 0 {
            return;
        }
        let next = !action.get_untracked().followed;
        action.update(|a| a.followed = next);
        action_pending.set(true);
        spawn_local(async move {
            let res = api::follow_user(mid, next).await;
            action_pending.set(false);
            if let Err(e) = res {
                web_sys::console::error_1(&format!("follow_user: {e}").into());
                action.update(|a| a.followed = !next);
            }
        });
    });

    // Global keyboard shortcuts for the watch page. Re-installs whenever the
    // <video> element appears (after the player mounts). The KeydownGuard's
    // Drop removes the listener when the route unmounts or the effect re-runs.
    Effect::new(
        move |_prev: Option<Option<KeydownGuard>>| -> Option<KeydownGuard> {
            let video = video_sig.get()?;
            let document = web_sys::window()?.document()?;
            KeydownGuard::install(move |ev: KeyboardEvent| {
                // Preserve browser shortcuts when modifiers are held.
                if ev.ctrl_key() || ev.meta_key() || ev.alt_key() {
                    return;
                }
                if is_typing(&document) {
                    return;
                }
                match ev.key().as_str() {
                    "d" | "D" => {
                        ev.prevent_default();
                        let next = !dm_enabled.get_untracked();
                        crate::prefs::set_danmaku_enabled(next);
                        dm_enabled.set(next);
                    }
                    "ArrowLeft" => {
                        ev.prevent_default();
                        let t = (video.current_time() - 5.0).max(0.0);
                        video.set_current_time(t);
                    }
                    "ArrowRight" => {
                        ev.prevent_default();
                        let dur = video.duration();
                        let mut t = video.current_time() + 5.0;
                        if dur.is_finite() && t > dur {
                            t = dur;
                        }
                        video.set_current_time(t);
                    }
                    "ArrowUp" => {
                        ev.prevent_default();
                        let v = (video.volume() + 0.1).clamp(0.0, 1.0);
                        let _ = video.set_volume(v);
                    }
                    "ArrowDown" => {
                        ev.prevent_default();
                        let v = (video.volume() - 0.1).clamp(0.0, 1.0);
                        let _ = video.set_volume(v);
                    }
                    " " => {
                        ev.prevent_default();
                        if video.paused() {
                            let _ = video.play();
                        } else {
                            let _ = video.pause();
                        }
                    }
                    "m" | "M" => {
                        ev.prevent_default();
                        video.set_muted(!video.muted());
                    }
                    "a" | "A" => {
                        ev.prevent_default();
                        do_triple.run(());
                    }
                    _ => {}
                }
            })
        },
    );

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
                            let info_for_hud = info.clone();
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
                                    {move || stats_hud_enabled.get().then(|| view! {
                                        <StatsHud video=video_sig info=info_for_hud.clone() />
                                    })}
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
                                    <button
                                        class="stats-toggle"
                                        title="解码诊断面板"
                                        on:click=move |_| {
                                            let next = !stats_hud_enabled.get();
                                            crate::prefs::set_stats_hud_enabled(next);
                                            stats_hud_enabled.set(next);
                                        }
                                    >
                                        {move || if stats_hud_enabled.get() { "Stats 开" } else { "Stats 关" }}
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
                                <div class="actions">
                                    <button
                                        class="act act-like"
                                        class:is-active=move || action.get().liked
                                        prop:disabled=move || action_pending.get()
                                        title="点赞 (Q)"
                                        on:click=move |_| do_like.run(())
                                    >
                                        <span class="icon">"👍"</span>
                                        <span class="count">{move || {
                                            let base = view_info.get().and_then(|r| r.ok()).map(|v| v.like).unwrap_or(0);
                                            fmt_views((base + like_delta.get()).max(0))
                                        }}</span>
                                    </button>
                                    <div class="coin-wrap">
                                        <button
                                            class="act act-coin"
                                            class:is-active=move || (action.get().coined > 0)
                                            prop:disabled=move || (action_pending.get() || action.get().coined > 0)
                                            title="投币 (W)"
                                            on:click=move |_| {
                                                if action.get_untracked().coined > 0 { return; }
                                                coin_open.update(|v| *v = !*v);
                                            }
                                        >
                                            <span class="icon">"🪙"</span>
                                            <span class="count">{move || {
                                                let base = view_info.get().and_then(|r| r.ok()).map(|v| v.coin).unwrap_or(0);
                                                fmt_views((base + coin_delta.get()).max(0))
                                            }}</span>
                                        </button>
                                        {move || coin_open.get().then(|| view! {
                                            <div class="coin-popover">
                                                <div class="coin-row">
                                                    <label>
                                                        <input
                                                            type="radio"
                                                            name="coin-count"
                                                            prop:checked=move || coin_count.get() == 1
                                                            on:change=move |_| coin_count.set(1)
                                                        />
                                                        " 1 币"
                                                    </label>
                                                    <label>
                                                        <input
                                                            type="radio"
                                                            name="coin-count"
                                                            prop:checked=move || coin_count.get() == 2
                                                            on:change=move |_| coin_count.set(2)
                                                        />
                                                        " 2 币"
                                                    </label>
                                                </div>
                                                <label class="coin-with-like">
                                                    <input
                                                        type="checkbox"
                                                        prop:checked=move || coin_with_like.get()
                                                        on:change=move |ev| {
                                                            if let Some(inp) = ev.target().and_then(|t| t.dyn_into::<HtmlInputElement>().ok()) {
                                                                coin_with_like.set(inp.checked());
                                                            }
                                                        }
                                                    />
                                                    " 同时点赞"
                                                </label>
                                                <div class="coin-row">
                                                    <button
                                                        class="coin-confirm"
                                                        on:click=move |_| do_coin.run((coin_count.get(), coin_with_like.get()))
                                                    >"确定"</button>
                                                    <button
                                                        class="coin-cancel"
                                                        on:click=move |_| coin_open.set(false)
                                                    >"取消"</button>
                                                </div>
                                            </div>
                                        })}
                                    </div>
                                    <button
                                        class="act act-triple"
                                        class:is-active=move || {
                                            let a = action.get();
                                            a.liked && a.coined > 0 && a.favorited
                                        }
                                        prop:disabled=move || action_pending.get()
                                        title="一键三连 (A)"
                                        on:click=move |_| do_triple.run(())
                                    >
                                        <span class="icon">"⭐"</span>
                                        <span>"三连"</span>
                                    </button>
                                </div>
                                <div class="up">
                                    <A href=move || format!("/space/{}", up_mid.get())>
                                        <span class="up-link">
                                            <img src=up_face_view alt="" />
                                            <span>{up_name_view}</span>
                                        </span>
                                    </A>
                                    <button
                                        class="follow-btn"
                                        class:is-active=move || action.get().followed
                                        prop:disabled=move || action_pending.get() || up_mid.get() <= 0
                                        on:click=move |_| do_follow.run(())
                                    >
                                        {move || if action.get().followed { "已关注" } else { "+ 关注" }}
                                    </button>
                                </div>
                                <div class="info">
                                    {move || view_info.get().and_then(|r| r.ok()).map(|v| view! {
                                        <div class="stats">
                                            <span>{fmt_views(v.view)}" 播放"</span>
                                            <span>{fmt_views(v.danmaku)}" 弹幕"</span>
                                            <span>{fmt_views(v.like + like_delta.get())}" 点赞"</span>
                                            <span>{fmt_views(v.coin + coin_delta.get())}" 投币"</span>
                                            <span>{fmt_views(v.favorite + fav_delta.get())}" 收藏"</span>
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
