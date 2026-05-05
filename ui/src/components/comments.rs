use crate::api;
use crate::types::{fmt_ctime, Comment};
use leptos::prelude::*;
use leptos_router::components::A;

#[component]
pub fn Comments(#[prop(into)] bvid: Signal<String>) -> impl IntoView {
    let items = RwSignal::new(Vec::<Comment>::new());
    let pn = RwSignal::new(1u32);
    let total = RwSignal::new(0i64);
    let loading = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let end_reached = RwSignal::new(false);
    let attempted = RwSignal::new(false);

    // StoredValue::new_local lets us reuse the non-Send closure across reactive
    // contexts. Same pattern as routes/home.rs.
    let load_more = StoredValue::new_local(move || {
        if loading.get_untracked() || end_reached.get_untracked() {
            return;
        }
        let bv = bvid.get_untracked();
        if bv.is_empty() {
            return;
        }
        let next_pn = pn.get_untracked();
        loading.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            // sort=2 → likes-first (matches Bilibili web default).
            match api::get_comments(&bv, next_pn, 2).await {
                Ok(page) => {
                    let count = page.acount.max(page.count);
                    total.set(count);
                    if page.replies.is_empty() {
                        end_reached.set(true);
                    } else {
                        let accumulated =
                            items.with_untracked(|v| v.len()) + page.replies.len();
                        items.update(|v| v.extend(page.replies));
                        pn.update(|p| *p += 1);
                        if count > 0 && accumulated as i64 >= count {
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

    // Reset and refetch when bvid changes (navigating between videos).
    Effect::new(move |_| {
        let _ = bvid.get();
        items.set(Vec::new());
        pn.set(1);
        total.set(0);
        end_reached.set(false);
        error.set(None);
        attempted.set(false);
        load_more.with_value(|f| f());
    });

    view! {
        <section class="comments">
            <h2>{move || {
                let t = total.get();
                if t > 0 { format!("评论 {t}") } else { "评论".to_string() }
            }}</h2>
            <For
                each=move || items.get()
                key=|c: &Comment| c.rpid
                children=move |c| view! { <CommentItem c=c /> }
            />
            {move || error.get().map(|e| view! {
                <div class="error">
                    {e}
                    <button on:click=move |_| {
                        error.set(None);
                        load_more.with_value(|f| f());
                    }>"Retry"</button>
                </div>
            })}
            {move || (attempted.get() && !loading.get()
                && items.with(|v| v.is_empty()) && error.with(|e| e.is_none()))
                .then(|| view! { <div class="empty">"暂无评论"</div> })}
            {move || (!end_reached.get() && !items.with(|v| v.is_empty())
                && error.with(|e| e.is_none()))
                .then(|| view! {
                    <button
                        class="load-more"
                        on:click=move |_| load_more.with_value(|f| f())
                        disabled=move || loading.get()
                    >
                        {move || if loading.get() { "Loading…" } else { "Load more" }}
                    </button>
                })}
            {move || (loading.get() && items.with(|v| v.is_empty()))
                .then(|| view! { <div class="loading">"Loading…"</div> })}
        </section>
    }
}

fn meta_for(ctime: i64, location: &str, like: i64) -> String {
    let mut parts = vec![fmt_ctime(ctime)];
    if !location.is_empty() {
        parts.push(location.to_string());
    }
    if like > 0 {
        parts.push(format!("赞 {like}"));
    }
    parts
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

#[component]
fn CommentItem(c: Comment) -> impl IntoView {
    let Comment {
        ctime,
        like,
        location,
        message,
        member,
        replies,
        ..
    } = c;
    let meta = meta_for(ctime, &location, like);
    let has_replies = !replies.is_empty();
    let href = format!("/space/{}", member.mid);
    let href_name = href.clone();

    view! {
        <div class="comment-item">
            <A href=href>
                <img class="avatar clickable" src=member.face alt="" />
            </A>
            <div class="comment-body">
                <A href=href_name>
                    <div class="comment-name">{member.uname}</div>
                </A>
                <div class="comment-message">{message}</div>
                <div class="comment-meta">{meta}</div>
                {has_replies.then(|| view! {
                    <div class="comment-replies">
                        <For
                            each=move || replies.clone()
                            key=|r: &Comment| r.rpid
                            children=move |r| view! { <SubReply c=r /> }
                        />
                    </div>
                })}
            </div>
        </div>
    }
}

#[component]
fn SubReply(c: Comment) -> impl IntoView {
    let Comment {
        ctime,
        like,
        location,
        message,
        member,
        ..
    } = c;
    let meta = meta_for(ctime, &location, like);
    let href = format!("/space/{}", member.mid);
    let href_name = href.clone();

    view! {
        <div class="comment-item">
            <A href=href>
                <img class="avatar clickable" src=member.face alt="" />
            </A>
            <div class="comment-body">
                <A href=href_name>
                    <div class="comment-name">{member.uname}</div>
                </A>
                <div class="comment-message">{message}</div>
                <div class="comment-meta">{meta}</div>
            </div>
        </div>
    }
}
