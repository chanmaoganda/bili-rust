use crate::api;
use crate::components::video_card::VideoCardView;
use crate::types::{fmt_views, SpaceInfo, SpaceVideoPage, VideoCard};
use leptos::prelude::*;
use leptos_router::hooks::use_params_map;

#[component]
pub fn Space() -> impl IntoView {
    let params = use_params_map();
    let mid = Signal::derive(move || {
        params
            .read()
            .get("mid")
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0)
    });

    let info = LocalResource::new(move || {
        let m = mid.get();
        async move {
            if m <= 0 {
                Err("invalid mid".to_string())
            } else {
                api::get_space_info(m).await
            }
        }
    });

    // Logged-in user — used to switch the action button between "follow" and
    // "this is you".
    let me = LocalResource::new(|| async { api::get_user_info().await });
    let following = RwSignal::new(false);
    let follow_pending = RwSignal::new(false);

    // Refresh follow state whenever mid changes (and ignore self).
    Effect::new(move |_| {
        let m = mid.get();
        let my_mid = me.get().and_then(|r| r.ok()).map(|u| u.mid).unwrap_or(0);
        following.set(false);
        if m <= 0 || m == my_mid {
            return;
        }
        leptos::task::spawn_local(async move {
            if let Ok(b) = api::is_following(m).await {
                following.set(b);
            }
        });
    });

    let videos = RwSignal::new(Vec::<VideoCard>::new());
    let page = RwSignal::new(1u32);
    let total = RwSignal::new(0i64);
    let v_loading = RwSignal::new(false);
    let v_error = RwSignal::new(None::<String>);
    let end_reached = RwSignal::new(false);

    let load_videos = StoredValue::new_local(move || {
        if v_loading.get_untracked() || end_reached.get_untracked() {
            return;
        }
        let m = mid.get_untracked();
        if m <= 0 {
            return;
        }
        let pn = page.get_untracked();
        v_loading.set(true);
        v_error.set(None);
        leptos::task::spawn_local(async move {
            match api::get_space_videos(m, pn, 30).await {
                Ok(SpaceVideoPage { list, count, .. }) => {
                    total.set(count);
                    if list.is_empty() {
                        end_reached.set(true);
                    } else {
                        let accumulated = videos.with_untracked(|v| v.len()) + list.len();
                        videos.update(|v| v.extend(list));
                        page.update(|p| *p += 1);
                        if count > 0 && accumulated as i64 >= count {
                            end_reached.set(true);
                        }
                    }
                }
                Err(e) => v_error.set(Some(e)),
            }
            v_loading.set(false);
        });
    });

    // Reset the video list whenever mid changes (navigating between users).
    Effect::new(move |_| {
        let _ = mid.get();
        videos.set(Vec::new());
        page.set(1);
        total.set(0);
        end_reached.set(false);
        v_error.set(None);
        load_videos.with_value(|f| f());
    });

    let toggle_follow = move |_| {
        if follow_pending.get_untracked() {
            return;
        }
        let m = mid.get_untracked();
        let want = !following.get_untracked();
        follow_pending.set(true);
        leptos::task::spawn_local(async move {
            match api::follow_user(m, want).await {
                Ok(()) => following.set(want),
                Err(e) => {
                    web_sys::console::warn_1(&format!("follow failed: {e}").into());
                }
            }
            follow_pending.set(false);
        });
    };

    view! {
        <div class="space-page">
            {move || match info.get() {
                None => view! { <div class="loading">"加载中…"</div> }.into_any(),
                Some(Err(e)) => view! { <div class="error">{e}</div> }.into_any(),
                Some(Ok(s)) => {
                    let SpaceInfo {
                        mid: s_mid,
                        name,
                        face,
                        sign,
                        level,
                        following: f_count,
                        follower,
                        ..
                    } = s;
                    let my_mid = me
                        .get()
                        .and_then(|r| r.ok())
                        .map(|u| u.mid)
                        .unwrap_or(0);
                    let is_self = my_mid != 0 && s_mid == my_mid;
                    view! {
                        <div class="space-header">
                            <img class="space-avatar" src=face alt="" />
                            <div class="space-meta">
                                <h1>{name}</h1>
                                <div class="space-sub">
                                    <span>"Lv " {level}</span>
                                    <span>"关注 " {fmt_views(f_count)}</span>
                                    <span>"粉丝 " {fmt_views(follower)}</span>
                                </div>
                                <div class="space-sign">{sign}</div>
                            </div>
                            {if is_self {
                                view! { <span class="space-self">"这是你"</span> }.into_any()
                            } else {
                                view! {
                                    <button
                                        class="follow-btn"
                                        class:is-active=move || following.get()
                                        prop:disabled=move || follow_pending.get()
                                        on:click=toggle_follow
                                    >
                                        {move || if following.get() { "已关注" } else { "+ 关注" }}
                                    </button>
                                }.into_any()
                            }}
                        </div>
                    }.into_any()
                }
            }}
            <h2 class="space-section">
                "投稿"
                {move || {
                    let t = total.get();
                    if t > 0 { format!(" · {t}") } else { String::new() }
                }}
            </h2>
            <div class="grid">
                <For
                    each=move || videos.get()
                    key=|c: &VideoCard| c.bvid.clone()
                    children=move |c| view! { <VideoCardView card=c /> }
                />
            </div>
            {move || v_error.get().map(|e| view! {
                <div class="error">{e}
                    <button on:click=move |_| {
                        v_error.set(None);
                        load_videos.with_value(|f| f());
                    }>"重试"</button>
                </div>
            })}
            {move || (!end_reached.get() && !v_loading.get()
                && !videos.with(|v| v.is_empty())
                && v_error.with(|e| e.is_none()))
                .then(|| view! {
                    <button
                        class="load-more"
                        on:click=move |_| load_videos.with_value(|f| f())
                    >
                        "加载更多"
                    </button>
                })}
            {move || v_loading.get().then(|| view! { <div class="loading">"加载中…"</div> })}
            {move || (end_reached.get() && videos.with(|v| v.is_empty())
                && v_error.with(|e| e.is_none()))
                .then(|| view! { <div class="empty">"暂无投稿"</div> })}
        </div>
    }
}
