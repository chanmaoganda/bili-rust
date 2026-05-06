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
        RcmdArgs {
            fresh_idx,
            brush,
            last_showlist,
        },
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
        DislikeArgs {
            goto,
            id,
            mid,
            rid,
            tag_id,
            reason_id,
        },
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

#[derive(Serialize)]
struct LastPlayArgs<'a> {
    bvid: &'a str,
    cid: i64,
}

pub async fn get_last_play(bvid: &str, cid: i64) -> Result<LastPlay, String> {
    invoke("get_last_play", LastPlayArgs { bvid, cid }).await
}

pub async fn get_danmaku(cid: i64) -> Result<Vec<Danmaku>, String> {
    invoke("get_danmaku", CidArgs { cid }).await
}

pub async fn get_comments(bvid: &str, pn: u32, sort: u32) -> Result<CommentPage, String> {
    invoke("get_comments", CommentArgs { bvid, pn, sort }).await
}

#[derive(Serialize)]
struct PostCommentArgs<'a> {
    aid: i64,
    message: &'a str,
    root: Option<i64>,
    parent: Option<i64>,
}

pub async fn post_comment(
    aid: i64,
    message: &str,
    root: Option<i64>,
    parent: Option<i64>,
) -> Result<PostedComment, String> {
    invoke(
        "post_comment",
        PostCommentArgs {
            aid,
            message,
            root,
            parent,
        },
    )
    .await
}

#[derive(Serialize)]
struct SendDanmakuArgs<'a> {
    bvid: &'a str,
    aid: i64,
    cid: i64,
    msg: &'a str,
    #[serde(rename = "progressMs")]
    progress_ms: i64,
    mode: u8,
    color: u32,
    fontsize: u8,
}

#[allow(clippy::too_many_arguments)]
pub async fn send_danmaku(
    bvid: &str,
    aid: i64,
    cid: i64,
    msg: &str,
    progress_ms: i64,
    mode: u8,
    color: u32,
    fontsize: u8,
) -> Result<PostedDanmaku, String> {
    invoke(
        "send_danmaku",
        SendDanmakuArgs {
            bvid,
            aid,
            cid,
            msg,
            progress_ms,
            mode,
            color,
            fontsize,
        },
    )
    .await
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
    invoke(
        "coin_video",
        CoinArgs {
            bvid,
            multiply,
            with_like,
        },
    )
    .await
}

pub async fn triple_video(bvid: &str) -> Result<TripleResult, String> {
    invoke("triple_video", BvidArgs { bvid }).await
}

pub async fn follow_user(mid: i64, follow: bool) -> Result<(), String> {
    invoke("follow_user", FollowArgs { mid, follow }).await
}

pub async fn get_decoder_probe() -> Result<DecoderProbe, String> {
    invoke("get_decoder_probe", NoArgs {}).await
}

#[derive(Serialize)]
struct PlayerLogArgs<'a> {
    label: &'a str,
    detail: &'a str,
}

pub async fn log_player_event(label: &str, detail: &str) -> Result<(), String> {
    invoke("log_player_event", PlayerLogArgs { label, detail }).await
}

#[derive(Serialize)]
struct MidArgs {
    mid: i64,
}

#[derive(Serialize)]
struct SpaceVideosArgs {
    mid: i64,
    pn: u32,
    ps: u32,
}

pub async fn is_following(mid: i64) -> Result<bool, String> {
    invoke("is_following", MidArgs { mid }).await
}

pub async fn get_space_info(mid: i64) -> Result<SpaceInfo, String> {
    invoke("get_space_info", MidArgs { mid }).await
}

pub async fn get_space_videos(mid: i64, pn: u32, ps: u32) -> Result<SpaceVideoPage, String> {
    invoke("get_space_videos", SpaceVideosArgs { mid, pn, ps }).await
}

#[derive(Serialize)]
struct PageArgs {
    pn: u32,
    ps: u32,
}

pub async fn get_followings(pn: u32, ps: u32) -> Result<FollowingsPage, String> {
    invoke("get_followings", PageArgs { pn, ps }).await
}

#[derive(Serialize)]
struct HeartbeatArgs<'a> {
    bvid: &'a str,
    aid: i64,
    cid: i64,
    #[serde(rename = "playedTime")]
    played_time: i64,
    duration: i64,
    #[serde(rename = "playType")]
    play_type: u8,
}

pub async fn report_heartbeat(
    bvid: &str,
    aid: i64,
    cid: i64,
    played_time: i64,
    duration: i64,
    play_type: u8,
) -> Result<(), String> {
    invoke(
        "report_heartbeat",
        HeartbeatArgs {
            bvid,
            aid,
            cid,
            played_time,
            duration,
            play_type,
        },
    )
    .await
}

#[derive(Serialize)]
struct HistoryArgs {
    max: i64,
    #[serde(rename = "viewAt")]
    view_at: i64,
}

#[derive(Serialize)]
struct AidArgs {
    aid: i64,
}

pub async fn get_history(max: i64, view_at: i64) -> Result<HistoryPage, String> {
    invoke("get_history", HistoryArgs { max, view_at }).await
}

pub async fn delete_history(aid: i64) -> Result<(), String> {
    invoke("delete_history", AidArgs { aid }).await
}

pub async fn clear_history() -> Result<(), String> {
    invoke("clear_history", NoArgs {}).await
}

pub async fn get_toview() -> Result<ToviewPage, String> {
    invoke("get_toview", NoArgs {}).await
}

pub async fn add_toview(bvid: &str) -> Result<(), String> {
    invoke("add_toview", BvidArgs { bvid }).await
}

pub async fn delete_toview(aid: i64) -> Result<(), String> {
    invoke("delete_toview", AidArgs { aid }).await
}

pub async fn delete_viewed_toview() -> Result<(), String> {
    invoke("delete_viewed_toview", NoArgs {}).await
}

pub async fn clear_toview() -> Result<(), String> {
    invoke("clear_toview", NoArgs {}).await
}

pub async fn qr_login_start() -> Result<QrStart, String> {
    invoke("qr_login_start", NoArgs {}).await
}

#[derive(Serialize)]
struct QrPollArgs<'a> {
    #[serde(rename = "qrcodeKey")]
    qrcode_key: &'a str,
}

pub async fn qr_login_poll(qrcode_key: &str) -> Result<QrPoll, String> {
    invoke("qr_login_poll", QrPollArgs { qrcode_key }).await
}
