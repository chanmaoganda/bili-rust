use crate::api;
use crate::components::video_card::VideoCardView;
use crate::state::FolloweesState;
use crate::types::{fmt_ctime, FollowingItem, FollowingsPage, SpaceVideoPage, VideoCard};
use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::use_navigate;
use leptos_router::NavigateOptions;
use std::collections::HashSet;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{IntersectionObserver, IntersectionObserverEntry, IntersectionObserverInit};

const PAGE_SIZE: u32 = 50;

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
pub fn Followees() -> impl IntoView {
    // Persistent across remounts (provided in app.rs). The user's selection in
    // particular must survive a side-trip to /space/:mid for inspection.
    let state = use_context::<FolloweesState>().expect("FolloweesState context missing");
    let FolloweesState {
        items,
        total,
        next_pn,
        attempted,
        end_reached,
        query,
        selected,
        scroll_y,
        preview,
    } = state;

    // Transient — fine to rebuild on remount.
    let loading = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let pending = RwSignal::new(HashSet::<i64>::new());
    let banner = RwSignal::new(None::<String>);

    // Logged-out gate. The `/followees` page is meaningless without a session,
    // so bounce to /login on first load if the user hasn't signed in.
    let auth_checked = RwSignal::new(false);
    Effect::new(move |prev: Option<()>| {
        if prev.is_some() {
            return;
        }
        let nav = use_navigate();
        leptos::task::spawn_local(async move {
            match api::get_user_info().await {
                Ok(u) if u.is_login => auth_checked.set(true),
                _ => nav("/login", NavigateOptions::default()),
            }
        });
    });

    let load_more = StoredValue::new_local(move || {
        if loading.get_untracked() || end_reached.get_untracked() {
            return;
        }
        let pn = next_pn.get_untracked();
        loading.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            match api::get_followings(pn, PAGE_SIZE).await {
                Ok(FollowingsPage {
                    list, total: t, ..
                }) => {
                    total.set(t);
                    if list.is_empty() {
                        end_reached.set(true);
                    } else {
                        let after = items.with_untracked(|v| v.len()) + list.len();
                        items.update(|v| v.extend(list));
                        next_pn.update(|p| *p += 1);
                        if t > 0 && after as i64 >= t {
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
        if auth_checked.get() && !attempted.get() && !loading.get() {
            load_more.with_value(|f| f());
        }
    });

    // Restore scroll position on remount (e.g. coming back from /space/:mid).
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

    // Save scroll position on unmount.
    on_cleanup(move || {
        if let Some(win) = web_sys::window() {
            scroll_y.set(win.scroll_y().unwrap_or(0.0));
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
            && auth_checked.get()
            && !loading.get()
            && !end_reached.get()
            && error.with(|e| e.is_none())
        {
            load_more.with_value(|f| f());
        }
    });

    // Filtered view, recomputed on items / query change.
    let visible = Signal::derive(move || {
        let q = query.get().trim().to_lowercase();
        items.with(|v| {
            v.iter()
                .filter(|it| {
                    if q.is_empty() {
                        return true;
                    }
                    it.uname.to_lowercase().contains(&q) || it.sign.to_lowercase().contains(&q)
                })
                .cloned()
                .collect::<Vec<_>>()
        })
    });

    let select_all_visible = move |_| {
        let v = visible.get();
        selected.update(|s| {
            for it in &v {
                s.insert(it.mid);
            }
        });
    };
    let clear_selection = move |_| {
        selected.update(|s| s.clear());
    };

    // Run unfollow on a fixed set of mids. Optimistically removes them, then
    // calls follow_user(mid, false) one-by-one with a small concurrency cap.
    // Failed mids get re-inserted at their original index and stay in `selected`.
    let run_unfollow = StoredValue::new_local(move |targets: Vec<i64>| {
        if targets.is_empty() {
            return;
        }
        let confirm_msg = if targets.len() == 1 {
            "确定取消关注该用户？".to_string()
        } else {
            format!("确定取消关注 {} 个用户？此操作不可撤销。", targets.len())
        };
        let win = match web_sys::window() {
            Some(w) => w,
            None => return,
        };
        let confirmed = win.confirm_with_message(&confirm_msg).unwrap_or(false);
        if !confirmed {
            return;
        }

        // Snapshot original indices so we can rollback on failure.
        let snapshot: Vec<(usize, FollowingItem)> = items.with_untracked(|v| {
            v.iter()
                .enumerate()
                .filter(|(_, it)| targets.contains(&it.mid))
                .map(|(i, it)| (i, it.clone()))
                .collect()
        });
        if snapshot.is_empty() {
            return;
        }

        // Optimistic remove + mark pending.
        let target_set: HashSet<i64> = targets.iter().copied().collect();
        items.update(|v| v.retain(|it| !target_set.contains(&it.mid)));
        pending.update(|p| p.extend(target_set.iter().copied()));
        banner.set(None);

        leptos::task::spawn_local(async move {
            // Sequential is fine for the volumes we expect; serialising avoids
            // tripping Bilibili's burst risk-control on the unfollow endpoint.
            let mut failures: Vec<(usize, FollowingItem)> = Vec::new();
            let mut successes: i64 = 0;
            for (idx, it) in snapshot.iter() {
                match api::follow_user(it.mid, false).await {
                    Ok(()) => {
                        successes += 1;
                    }
                    Err(e) => {
                        web_sys::console::warn_1(
                            &format!("unfollow {} failed: {e}", it.mid).into(),
                        );
                        failures.push((*idx, it.clone()));
                    }
                }
            }
            // Rollback failures into their original positions (sort ascending so
            // splice indices remain valid as we re-insert).
            if !failures.is_empty() {
                let mut sorted = failures.clone();
                sorted.sort_by_key(|(i, _)| *i);
                items.update(|v| {
                    for (i, it) in sorted {
                        let pos = i.min(v.len());
                        v.insert(pos, it);
                    }
                });
            }
            // Update total by the number of *actual* successes.
            total.update(|t| *t = (*t - successes).max(0));
            // Selection: keep failed mids selected, drop successes.
            let failed_mids: HashSet<i64> = failures.iter().map(|(_, it)| it.mid).collect();
            selected.update(|s| {
                s.retain(|m| failed_mids.contains(m));
            });
            pending.update(|p| {
                for m in target_set.iter() {
                    p.remove(m);
                }
            });
            banner.set(if failures.is_empty() {
                None
            } else {
                Some(format!(
                    "{} / {} 失败，请重试",
                    failures.len(),
                    snapshot.len()
                ))
            });
        });
    });

    let bulk_unfollow = move |_| {
        let targets: Vec<i64> = selected.with(|s| s.iter().copied().collect());
        run_unfollow.with_value(|f| f(targets));
    };

    // Right-pane: load `preview` user's recent videos. Keyed on preview mid so
    // switching subjects refetches.
    let preview_videos = LocalResource::new(move || {
        let mid = preview.get().map(|p| p.mid).unwrap_or(0);
        async move {
            if mid <= 0 {
                Ok::<Vec<VideoCard>, String>(Vec::new())
            } else {
                api::get_space_videos(mid, 1, 8)
                    .await
                    .map(|SpaceVideoPage { list, .. }: SpaceVideoPage| list)
            }
        }
    });

    view! {
        <div class="followees-page">
            <section class="followees-main">
                <div class="followees-header">
                    <h1>
                        "我的关注 "
                        <span class="muted">{move || format!("({})", total.get())}</span>
                    </h1>
                    <input
                        class="followees-search"
                        type="search"
                        placeholder="搜索昵称 / 简介"
                        prop:value=move || query.get()
                        on:input=move |ev| query.set(event_target_value(&ev))
                    />
                </div>

                <div class="followees-actionbar">
                    <span>"已选择 " {move || selected.with(|s| s.len())}</span>
                    <button on:click=select_all_visible>"全选当前可见"</button>
                    <button
                        on:click=clear_selection
                        prop:disabled=move || selected.with(|s| s.is_empty())
                    >"清空选择"</button>
                    <button
                        class="unfollow-btn primary"
                        on:click=bulk_unfollow
                        prop:disabled=move || selected.with(|s| s.is_empty())
                    >
                        {move || format!("取消关注 ({})", selected.with(|s| s.len()))}
                    </button>
                    {move || banner.get().map(|m| view! { <span class="banner-error">{m}</span> })}
                </div>

                <ul class="followees-list">
                    <For
                        each=move || visible.get()
                        key=|it: &FollowingItem| it.mid
                        children=move |it| {
                            let mid = it.mid;
                            let space_href = format!("/space/{mid}");
                            let face = it.face.clone();
                            let uname = it.uname.clone();
                            let sign = it.sign.clone();
                            let mtime = it.mtime;
                            let item_for_preview = it.clone();
                            let is_checked = move || selected.with(|s| s.contains(&mid));
                            let toggle = move |_| selected.update(|s| {
                                if !s.insert(mid) { s.remove(&mid); }
                            });
                            let row_unfollow = move |ev: web_sys::MouseEvent| {
                                ev.stop_propagation();
                                run_unfollow.with_value(|f| f(vec![mid]));
                            };
                            let on_preview = {
                                let it = item_for_preview.clone();
                                move |_| preview.set(Some(it.clone()))
                            };
                            let is_pending = move || pending.with(|p| p.contains(&mid));
                            let is_active = move || preview.with(|p| p.as_ref().map(|p| p.mid) == Some(mid));
                            view! {
                                <li
                                    class="followee-row"
                                    class:is-pending=is_pending
                                    class:is-active=is_active
                                >
                                    <label class="followee-check">
                                        <input
                                            type="checkbox"
                                            prop:checked=is_checked
                                            on:change=toggle
                                        />
                                        <span class="followee-check-box"></span>
                                    </label>
                                    <button class="followee-preview-btn" on:click=on_preview title="查看视频">
                                        <img class="followee-avatar" src=face alt="" />
                                        <span class="followee-meta">
                                            <span class="followee-name">{uname}</span>
                                            <span class="followee-sign">{sign}</span>
                                            <span class="followee-mtime">
                                                {if mtime > 0 { fmt_ctime(mtime) } else { String::new() }}
                                            </span>
                                        </span>
                                    </button>
                                    <A href=space_href>
                                        <span class="followee-space-link" title="个人主页">"主页 →"</span>
                                    </A>
                                    <button
                                        class="unfollow-btn"
                                        on:click=row_unfollow
                                        prop:disabled=is_pending
                                    >"取消关注"</button>
                                </li>
                            }
                        }
                    />
                </ul>

                {move || error.get().map(|e| view! {
                    <div class="error">{e}
                        <button on:click=move |_| {
                            error.set(None);
                            load_more.with_value(|f| f());
                        }>"重试"</button>
                    </div>
                })}
                {move || loading.get().then(|| view! { <div class="loading">"加载中…"</div> })}
                {move || (attempted.get() && items.with(|v| v.is_empty())
                    && error.with(|e| e.is_none()))
                    .then(|| view! { <div class="empty">"暂无关注"</div> })}
                {move || (!items.with(|v| v.is_empty()) && !visible.with(|v| v.is_empty())
                    && end_reached.get())
                    .then(|| view! { <div class="empty muted">"已到底"</div> })}
                <div node_ref=sentinel_ref class="scroll-sentinel"></div>
            </section>

            <aside class="followees-preview">
                {move || match preview.get() {
                    None => view! {
                        <div class="preview-empty muted">"点击头像查看该 UP 的视频"</div>
                    }.into_any(),
                    Some(p) => {
                        let space_href = format!("/space/{}", p.mid);
                        let face = p.face.clone();
                        let uname = p.uname.clone();
                        let close = move |_| preview.set(None);
                        view! {
                            <div class="preview-head">
                                <img class="preview-avatar" src=face alt="" />
                                <div class="preview-meta">
                                    <div class="preview-name">{uname}</div>
                                    <A href=space_href>
                                        <span class="preview-space-link">"前往主页 →"</span>
                                    </A>
                                </div>
                                <button class="preview-close" on:click=close title="关闭">"✕"</button>
                            </div>
                            <div class="preview-videos">
                                {move || match preview_videos.get() {
                                    None => view! { <div class="loading">"加载中…"</div> }.into_any(),
                                    Some(Err(e)) => view! { <div class="error">{e}</div> }.into_any(),
                                    Some(Ok(list)) if list.is_empty() => {
                                        view! { <div class="empty muted">"暂无投稿"</div> }.into_any()
                                    }
                                    Some(Ok(list)) => view! {
                                        <div class="preview-grid">
                                            <For
                                                each=move || list.clone()
                                                key=|c: &VideoCard| c.bvid.clone()
                                                children=move |c| view! { <VideoCardView card=c /> }
                                            />
                                        </div>
                                    }.into_any(),
                                }}
                            </div>
                        }.into_any()
                    }
                }}
            </aside>
        </div>
    }
}
