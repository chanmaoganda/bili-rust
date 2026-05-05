use crate::types::*;
use serde::{Deserialize, Serialize};
use serde_wasm_bindgen::{from_value, to_value};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"], js_name = invoke, catch)]
    async fn invoke_raw(cmd: &str, args: JsValue) -> Result<JsValue, JsValue>;
}

async fn invoke<R, A>(cmd: &str, args: A) -> Result<R, String>
where
    R: for<'de> Deserialize<'de>,
    A: Serialize,
{
    let args = to_value(&args).map_err(|e| format!("serialize args: {e}"))?;
    let v = invoke_raw(cmd, args)
        .await
        .map_err(|e| format!("invoke {cmd}: {:?}", e))?;
    from_value::<R>(v).map_err(|e| format!("deserialize {cmd} result: {e}"))
}

#[derive(Serialize)]
struct NoArgs {}

#[derive(Serialize)]
struct RcmdArgs<'a> {
    #[serde(rename = "freshIdx")]
    fresh_idx: u32,
    brush: u32,
    #[serde(rename = "lastShowlist")]
    last_showlist: &'a str,
}

#[derive(Serialize)]
struct DislikeArgs<'a> {
    goto: &'a str,
    id: i64,
    mid: Option<i64>,
    rid: Option<i64>,
    #[serde(rename = "tagId")]
    tag_id: Option<i64>,
    #[serde(rename = "reasonId")]
    reason_id: u32,
}

#[derive(Serialize)]
struct BvidArgs<'a> {
    bvid: &'a str,
}

#[derive(Serialize)]
struct PlayArgs<'a> {
    bvid: &'a str,
    cid: Option<i64>,
    qn: Option<u32>,
}

#[derive(Serialize)]
struct CidArgs {
    cid: i64,
}

#[derive(Serialize)]
struct CommentArgs<'a> {
    bvid: &'a str,
    pn: u32,
    sort: u32,
}

pub async fn get_user_info() -> Result<UserInfo, String> {
    invoke("get_user_info", NoArgs {}).await
}

pub async fn get_rcmd(
    fresh_idx: u32,
    brush: u32,
    last_showlist: &str,
) -> Result<Vec<VideoCard>, String> {
    invoke(
        "get_rcmd",
        RcmdArgs { fresh_idx, brush, last_showlist },
    )
    .await
}

pub async fn feed_dislike(
    goto: &str,
    id: i64,
    mid: Option<i64>,
    rid: Option<i64>,
    tag_id: Option<i64>,
    reason_id: u32,
) -> Result<(), String> {
    invoke(
        "feed_dislike",
        DislikeArgs { goto, id, mid, rid, tag_id, reason_id },
    )
    .await
}

pub async fn get_related(bvid: &str) -> Result<Vec<VideoCard>, String> {
    invoke("get_related", BvidArgs { bvid }).await
}

pub async fn get_play_info(
    bvid: &str,
    cid: Option<i64>,
    qn: Option<u32>,
) -> Result<PlayInfo, String> {
    invoke("get_play_info", PlayArgs { bvid, cid, qn }).await
}

pub async fn get_view_info(bvid: &str) -> Result<ViewInfo, String> {
    invoke("get_view_info", BvidArgs { bvid }).await
}

pub async fn get_danmaku(cid: i64) -> Result<Vec<Danmaku>, String> {
    invoke("get_danmaku", CidArgs { cid }).await
}

pub async fn get_comments(bvid: &str, pn: u32, sort: u32) -> Result<CommentPage, String> {
    invoke("get_comments", CommentArgs { bvid, pn, sort }).await
}

#[derive(Serialize)]
struct ActionStateArgs<'a> {
    bvid: &'a str,
    mid: i64,
}

#[derive(Serialize)]
struct LikeArgs<'a> {
    bvid: &'a str,
    like: bool,
}

#[derive(Serialize)]
struct CoinArgs<'a> {
    bvid: &'a str,
    multiply: u8,
    #[serde(rename = "withLike")]
    with_like: bool,
}

#[derive(Serialize)]
struct FollowArgs {
    mid: i64,
    follow: bool,
}

pub async fn get_action_state(bvid: &str, mid: i64) -> Result<ActionState, String> {
    invoke("get_action_state", ActionStateArgs { bvid, mid }).await
}

pub async fn like_video(bvid: &str, like: bool) -> Result<(), String> {
    invoke("like_video", LikeArgs { bvid, like }).await
}

pub async fn coin_video(bvid: &str, multiply: u8, with_like: bool) -> Result<(), String> {
    invoke("coin_video", CoinArgs { bvid, multiply, with_like }).await
}

pub async fn triple_video(bvid: &str) -> Result<TripleResult, String> {
    invoke("triple_video", BvidArgs { bvid }).await
}

pub async fn follow_user(mid: i64, follow: bool) -> Result<(), String> {
    invoke("follow_user", FollowArgs { mid, follow }).await
}
