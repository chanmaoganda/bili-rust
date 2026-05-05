use crate::api;
use crate::components::player::Player;
use crate::components::video_card::VideoCardView;
use crate::types::VideoCard;
use leptos::prelude::*;
use leptos_router::hooks::{use_params_map, use_query_map};

#[component]
pub fn Watch() -> impl IntoView {
    let params = use_params_map();
    let query = use_query_map();
    let bvid = Signal::derive(move || params.read().get("bvid").unwrap_or_default());
    let cid = Signal::derive(move || {
        query.read().get("cid").and_then(|s| s.parse::<i64>().ok())
    });

    let play = LocalResource::new(move || {
        let bv = bvid.get();
        let c = cid.get();
        async move { api::get_play_info(&bv, c, None).await }
    });
    let related = LocalResource::new(move || {
        let bv = bvid.get();
        async move { api::get_related(&bv).await }
    });

    view! {
        <div class="watch">
            <div>
                {move || match play.get() {
                    None => view! { <div class="loading">"Loading player…"</div> }.into_any(),
                    Some(res) => match res {
                        Ok(info) => {
                            let title = info.title.clone();
                            let up_face = info.up_face.clone();
                            let up_name = info.up_name.clone();
                            view! {
                                <Player info=info />
                                <h1>{title}</h1>
                                <div class="up">
                                    <img src=up_face alt="" />
                                    <span>{up_name}</span>
                                </div>
                            }
                                .into_any()
                        }
                        Err(e) => view! { <div class="error">{e}</div> }.into_any(),
                    }
                }}
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
