use crate::api;
use crate::types::{fmt_ctime, Comment, CommentMember, UserInfo};
use leptos::prelude::*;
use leptos_router::components::A;
use wasm_bindgen::JsCast;
use web_sys::{HtmlTextAreaElement, KeyboardEvent};

#[component]
pub fn Comments(
    #[prop(into)] bvid: Signal<String>,
    #[prop(into)] aid: Signal<i64>,
) -> impl IntoView {
    let items = RwSignal::new(Vec::<Comment>::new());
    let pn = RwSignal::new(1u32);
    let total = RwSignal::new(0i64);
    let loading = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let end_reached = RwSignal::new(false);
    let attempted = RwSignal::new(false);

    // Current-user lookup for the optimistic local Comment we prepend after
    // a successful post. We don't gate posting on this — the post uses the
    // server-side cookies — but if the lookup hasn't resolved yet we just
    // skip the optimistic insert and fall back to a refresh.
    let me = LocalResource::new(|| async { api::get_user_info().await.ok() });

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
                        let accumulated = items.with_untracked(|v| v.len()) + page.replies.len();
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

    let on_posted_top = move |c: Comment| {
        items.update(|v| v.insert(0, c));
        total.update(|t| *t += 1);
    };

    let on_posted_reply = move |parent_rpid: i64, c: Comment| {
        items.update(|v| {
            if let Some(parent) = v.iter_mut().find(|p| p.rpid == parent_rpid) {
                parent.replies.push(c);
                parent.rcount += 1;
            }
        });
    };

    view! {
        <section class="comments">
            <h2>{move || {
                let t = total.get();
                if t > 0 { format!("评论 {t}") } else { "评论".to_string() }
            }}</h2>
            <Composer
                aid=aid
                root=None
                parent=None
                me=me
                placeholder="发条友善的评论吧"
                on_posted=Callback::new(move |c| on_posted_top(c))
            />
            <For
                each=move || items.get()
                key=|c: &Comment| c.rpid
                children=move |c| view! {
                    <CommentItem
                        c=c.clone()
                        aid=aid
                        me=me
                        on_reply_posted=Callback::new(move |new_c: Comment| on_posted_reply(c.rpid, new_c))
                    />
                }
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

fn now_secs() -> i64 {
    (js_sys::Date::now() / 1000.0) as i64
}

fn synth_local_comment(rpid: i64, message: String, me: Option<&UserInfo>) -> Comment {
    let member = match me {
        Some(u) => CommentMember {
            mid: u.mid,
            uname: u.uname.clone(),
            face: u.face.clone(),
        },
        None => CommentMember {
            mid: 0,
            uname: "我".to_string(),
            face: String::new(),
        },
    };
    Comment {
        rpid,
        mid: member.mid,
        ctime: now_secs(),
        like: 0,
        rcount: 0,
        message,
        member,
        replies: Vec::new(),
        location: String::new(),
    }
}

#[component]
fn Composer(
    #[prop(into)] aid: Signal<i64>,
    root: Option<i64>,
    parent: Option<i64>,
    me: LocalResource<Option<UserInfo>>,
    #[prop(into)] placeholder: String,
    on_posted: Callback<Comment>,
) -> impl IntoView {
    let text = RwSignal::new(String::new());
    let posting = RwSignal::new(false);
    let err = RwSignal::new(None::<String>);

    let submit = move || {
        if posting.get_untracked() {
            return;
        }
        let msg = text.get_untracked().trim().to_string();
        if msg.is_empty() {
            return;
        }
        let a = aid.get_untracked();
        if a == 0 {
            err.set(Some("视频信息未加载".into()));
            return;
        }
        posting.set(true);
        err.set(None);
        leptos::task::spawn_local(async move {
            match api::post_comment(a, &msg, root, parent).await {
                Ok(p) => {
                    let me_snap = me.get().and_then(|x| x);
                    let synth = synth_local_comment(p.rpid, msg, me_snap.as_ref());
                    on_posted.run(synth);
                    text.set(String::new());
                }
                Err(e) => err.set(Some(e)),
            }
            posting.set(false);
        });
    };

    let on_keydown = move |ev: KeyboardEvent| {
        // Ctrl/Cmd+Enter submits — matches Bilibili web client and keeps a bare
        // Enter free for inserting newlines.
        if ev.key() == "Enter" && (ev.ctrl_key() || ev.meta_key()) {
            ev.prevent_default();
            submit();
        }
    };

    let on_input = move |ev: web_sys::Event| {
        if let Some(t) = ev
            .target()
            .and_then(|t| t.dyn_into::<HtmlTextAreaElement>().ok())
        {
            text.set(t.value());
        }
    };

    view! {
        <div class="comment-composer">
            <textarea
                class="comment-input"
                placeholder=placeholder
                prop:value=move || text.get()
                on:input=on_input
                on:keydown=on_keydown
                disabled=move || posting.get()
            ></textarea>
            <div class="comment-composer-row">
                {move || err.get().map(|e| view! { <span class="error">{e}</span> })}
                <button
                    class="comment-submit"
                    on:click=move |_| submit()
                    disabled=move || posting.get() || text.with(|s| s.trim().is_empty())
                >
                    {move || if posting.get() { "发送中…" } else { "发送" }}
                </button>
            </div>
        </div>
    }
}

#[component]
fn CommentItem(
    c: Comment,
    #[prop(into)] aid: Signal<i64>,
    me: LocalResource<Option<UserInfo>>,
    on_reply_posted: Callback<Comment>,
) -> impl IntoView {
    let Comment {
        rpid,
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

    let local_replies = RwSignal::new(Vec::<Comment>::new());
    let reply_open = RwSignal::new(false);

    let on_reply = move |c: Comment| {
        local_replies.update(|v| v.push(c.clone()));
        reply_open.set(false);
        on_reply_posted.run(c);
    };
    let placeholder = format!("回复 @{}", member.uname);

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
                <div class="comment-meta">
                    {meta}
                    <button
                        class="comment-reply-btn"
                        on:click=move |_| reply_open.update(|v| *v = !*v)
                    >
                        {move || if reply_open.get() { "取消" } else { "回复" }}
                    </button>
                </div>
                {has_replies.then(|| view! {
                    <div class="comment-replies">
                        <For
                            each=move || replies.clone()
                            key=|r: &Comment| r.rpid
                            children=move |r| view! { <SubReply c=r /> }
                        />
                    </div>
                })}
                <div class="comment-replies">
                    <For
                        each=move || local_replies.get()
                        key=|r: &Comment| r.rpid
                        children=move |r| view! { <SubReply c=r /> }
                    />
                </div>
                {move || reply_open.get().then(|| view! {
                    <Composer
                        aid=aid
                        root=Some(rpid)
                        parent=Some(rpid)
                        me=me
                        placeholder=placeholder.clone()
                        on_posted=Callback::new(move |c| on_reply(c))
                    />
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
