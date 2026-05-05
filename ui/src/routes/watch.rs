use crate::api;
use crate::components::player::Player;
use crate::components::video_card::VideoCardView;
use crate::types::VideoCard;
use leptos::prelude::*;
use leptos_router::hooks::{use_params_map, use_query_map};
use wasm_bindgen::JsCast;
use web_sys::HtmlSelectElement;

#[component]
pub fn Watch() -> impl IntoView {
    let params = use_params_map();
    let query = use_query_map();
    let bvid = Signal::derive(move || params.read().get("bvid").unwrap_or_default());
    let cid = Signal::derive(move || {
        query.read().get("cid").and_then(|s| s.parse::<i64>().ok())
    });

    let qn = RwSignal::new(None::<u32>);
    let resume_at = RwSignal::new(0.0_f64);

    // Reset resume position when navigating to a different video.
    Effect::new(move |_| {
        let _ = bvid.get();
        resume_at.set(0.0);
        qn.set(None);
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

    let on_time = Callback::new(move |t: f64| resume_at.set(t));

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
                            let qualities: Vec<(u32, String)> = info
                                .accept_quality
                                .iter()
                                .copied()
                                .zip(info.accept_description.iter().cloned())
                                .collect();
                            let current = info.current_quality;
                            let start = resume_at.get_untracked();
                            view! {
                                <Player info=info start_at=start on_time=on_time />
                                <div class="player-bar">
                                    <h1>{title}</h1>
                                    {(!qualities.is_empty()).then(|| view! {
                                        <select
                                            class="quality"
                                            on:change=move |ev| {
                                                let target = ev.target().and_then(|t| t.dyn_into::<HtmlSelectElement>().ok());
                                                if let Some(sel) = target {
                                                    if let Ok(v) = sel.value().parse::<u32>() {
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
