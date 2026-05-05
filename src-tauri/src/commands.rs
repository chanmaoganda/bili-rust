use crate::api::Bili;
use crate::danmaku::Danmaku;
use crate::stream;
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;
use tauri::State;

pub type BiliState<'a> = State<'a, Arc<Bili>>;

fn err<E: std::fmt::Display>(e: E) -> String {
    format!("{e:#}")
}

#[derive(Serialize)]
pub struct UserInfo {
    pub is_login: bool,
    pub mid: i64,
    pub uname: String,
    pub face: String,
}

#[derive(Serialize)]
pub struct VideoCard {
    pub bvid: String,
    pub aid: i64,
    pub cid: i64,
    pub title: String,
    pub pic: String,
    pub duration: i64,
    pub view: i64,
    pub up_name: String,
    pub up_face: String,
    pub up_mid: i64,
    pub rcmd_reason: Option<String>,
    pub tname: Option<String>,
    pub tid: Option<i64>,
}

#[derive(Serialize)]
pub struct PlayInfo {
    pub bvid: String,
    pub cid: i64,
    pub title: String,
    pub duration: i64,
    pub up_name: String,
    pub up_face: String,
    pub up_mid: i64,
    pub video: Vec<DashTrack>,
    pub audio: Vec<DashTrack>,
    pub accept_quality: Vec<u32>,
    pub accept_description: Vec<String>,
    pub current_quality: u32,
}

#[derive(Serialize)]
pub struct ViewInfo {
    pub bvid: String,
    pub aid: i64,
    pub cid: i64,
    pub title: String,
    pub desc: String,
    pub pic: String,
    pub duration: i64,
    pub pubdate: i64,
    pub tname: String,
    pub tid: i64,
    pub up_name: String,
    pub up_face: String,
    pub up_mid: i64,
    pub view: i64,
    pub danmaku: i64,
    pub reply: i64,
    pub favorite: i64,
    pub coin: i64,
    pub like: i64,
    pub share: i64,
}

#[derive(Serialize)]
pub struct CommentMember {
    pub mid: i64,
    pub uname: String,
    pub face: String,
}

#[derive(Serialize)]
pub struct Comment {
    pub rpid: i64,
    pub mid: i64,
    pub ctime: i64,
    pub like: i64,
    pub rcount: i64,
    pub message: String,
    pub member: CommentMember,
    pub replies: Vec<Comment>,
    pub location: String,
}

#[derive(Serialize)]
pub struct CommentPage {
    pub page: u32,
    pub size: u32,
    pub count: i64,
    pub acount: i64,
    pub replies: Vec<Comment>,
}

#[derive(Serialize)]
pub struct DashTrack {
    pub id: i64,
    pub mime: String,
    pub codecs: String,
    pub bandwidth: i64,
    pub width: i64,
    pub height: i64,
    pub frame_rate: String,
    pub base_url: String,
    pub init_range: String,  // "start-end"
    pub index_range: String, // "start-end"
}

#[derive(Serialize)]
pub struct DecoderProbe {
    /// GStreamer hardware decoder element factories detected via gst-inspect-1.0.
    pub hw_decoders: Vec<String>,
    pub libva_driver: Option<String>,
    pub gst_vaapi_all_drivers: Option<String>,
    pub webkit_disable_compositing: Option<String>,
    /// `false` if `gst-inspect-1.0` is missing or failed; `error` carries the reason.
    pub gst_inspect_ok: bool,
    pub error: Option<String>,
}

#[tauri::command]
pub async fn get_user_info(state: BiliState<'_>) -> Result<UserInfo, String> {
    let nav = state.nav().await.map_err(err)?;
    let face = proxy_image(&nav.data.face);
    Ok(UserInfo {
        is_login: nav.data.is_login,
        mid: nav.data.mid,
        uname: nav.data.uname,
        face,
    })
}

#[tauri::command]
pub async fn get_rcmd(
    state: BiliState<'_>,
    fresh_idx: u32,
    brush: u32,
    last_showlist: String,
) -> Result<Vec<VideoCard>, String> {
    let items = state
        .rcmd(fresh_idx, brush, 24, &last_showlist)
        .await
        .map_err(err)?;
    Ok(items.iter().filter_map(card_from_rcmd_item).collect())
}

#[tauri::command]
pub async fn feed_dislike(
    state: BiliState<'_>,
    goto: String,
    id: i64,
    mid: Option<i64>,
    rid: Option<i64>,
    tag_id: Option<i64>,
    reason_id: u32,
) -> Result<(), String> {
    state
        .dislike(&goto, id, mid, rid, tag_id, reason_id)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn get_related(state: BiliState<'_>, bvid: String) -> Result<Vec<VideoCard>, String> {
    let items = state.related(&bvid).await.map_err(err)?;
    Ok(items.iter().filter_map(card_from_related_item).collect())
}

#[tauri::command]
pub async fn get_play_info(
    state: BiliState<'_>,
    bvid: String,
    cid: Option<i64>,
    qn: Option<u32>,
) -> Result<PlayInfo, String> {
    let t0 = std::time::Instant::now();
    let qn = qn.unwrap_or(80);

    // Fast path — frontend already has cid (from rcmd/related).
    // Slow path — fall back to /view to look it up.
    let (cid, view_meta) = match cid {
        Some(c) => (c, None),
        None => {
            let v = state.view(&bvid).await.map_err(err)?;
            let cid = v.cid;
            (cid, Some(v))
        }
    };

    let raw = state.play_url(&bvid, cid, qn).await.map_err(err)?;
    let dash = raw
        .get("dash")
        .ok_or_else(|| "no dash in playurl response".to_string())?;

    let video: Vec<DashTrack> = dash
        .get("video")
        .and_then(|a| a.as_array())
        .map(|arr| arr.iter().filter_map(track_from).collect())
        .unwrap_or_default();
    let audio: Vec<DashTrack> = dash
        .get("audio")
        .and_then(|a| a.as_array())
        .map(|arr| arr.iter().filter_map(track_from).collect())
        .unwrap_or_default();

    let mut accept_quality: Vec<u32> = raw
        .get("accept_quality")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_u64().map(|n| n as u32))
                .collect()
        })
        .unwrap_or_default();
    let mut accept_description: Vec<String> = raw
        .get("accept_description")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    // Bilibili occasionally returns mismatched arrays for unauthenticated users.
    let n = accept_quality.len().min(accept_description.len());
    accept_quality.truncate(n);
    accept_description.truncate(n);
    let current_quality = raw
        .get("quality")
        .and_then(|v| v.as_u64())
        .map(|n| n as u32)
        .unwrap_or(qn);

    let view_fallback = view_meta.is_some();
    let (title, duration, up_name, up_face, up_mid) = if let Some(v) = view_meta {
        (
            v.title,
            v.duration,
            v.owner.name,
            proxy_image(&v.owner.face),
            v.owner.mid,
        )
    } else {
        let dur = raw
            .pointer("/dash/duration")
            .and_then(|v| v.as_i64())
            .or_else(|| {
                raw.get("timelength")
                    .and_then(|v| v.as_i64())
                    .map(|ms| ms / 1000)
            })
            .unwrap_or(0);
        (String::new(), dur, String::new(), String::new(), 0)
    };

    tracing::info!(
        bvid = %bvid,
        cid,
        qn,
        view_fallback,
        ms = t0.elapsed().as_millis() as u64,
        v_tracks = video.len(),
        a_tracks = audio.len(),
        "get_play_info"
    );

    Ok(PlayInfo {
        bvid,
        cid,
        title,
        duration,
        up_name,
        up_face,
        up_mid,
        video,
        audio,
        accept_quality,
        accept_description,
        current_quality,
    })
}

#[tauri::command]
pub async fn get_view_info(state: BiliState<'_>, bvid: String) -> Result<ViewInfo, String> {
    let v = state.view(&bvid).await.map_err(err)?;
    Ok(ViewInfo {
        bvid: v.bvid,
        aid: v.aid,
        cid: v.cid,
        title: v.title,
        desc: v.desc,
        pic: proxy_image(&v.pic),
        duration: v.duration,
        pubdate: v.pubdate,
        tname: v.tname,
        tid: v.tid,
        up_name: v.owner.name,
        up_face: proxy_image(&v.owner.face),
        up_mid: v.owner.mid,
        view: v.stat.view,
        danmaku: v.stat.danmaku,
        reply: v.stat.reply,
        favorite: v.stat.favorite,
        coin: v.stat.coin,
        like: v.stat.like,
        share: v.stat.share,
    })
}

#[tauri::command]
pub async fn get_danmaku(state: BiliState<'_>, cid: i64) -> Result<Vec<Danmaku>, String> {
    state.danmaku(cid).await.map_err(err)
}

#[derive(Serialize, Default)]
pub struct ActionState {
    pub liked: bool,
    pub coined: i64,
    pub favorited: bool,
    pub followed: bool,
}

#[derive(Serialize)]
pub struct TripleResult {
    pub like: bool,
    pub coin: bool,
    pub fav: bool,
}

#[tauri::command]
pub async fn get_action_state(
    state: BiliState<'_>,
    bvid: String,
    mid: i64,
) -> Result<ActionState, String> {
    // Fan out: archive/relation gives like/coin/fav for the video, /x/relation
    // gives the follow bit for the uploader. Run them concurrently — they hit
    // different endpoints and there's no ordering dependency.
    let bili = state.inner().clone();
    let bili2 = bili.clone();
    let bv = bvid.clone();
    let f_archive = async move { bili.archive_relation(&bv).await };
    let f_user = async move {
        if mid > 0 {
            bili2.user_relation(mid).await
        } else {
            Ok(false)
        }
    };
    let (ar, fol) = tokio::join!(f_archive, f_user);
    let ar = ar.map_err(err)?;
    let followed = fol.map_err(err)?;
    Ok(ActionState {
        liked: ar.liked,
        coined: ar.coined,
        favorited: ar.favorited,
        followed,
    })
}

#[tauri::command]
pub async fn like_video(
    state: BiliState<'_>,
    bvid: String,
    like: bool,
) -> Result<(), String> {
    state.like_video(&bvid, like).await.map_err(err)
}

#[tauri::command]
pub async fn coin_video(
    state: BiliState<'_>,
    bvid: String,
    multiply: u8,
    with_like: bool,
) -> Result<(), String> {
    state
        .coin_video(&bvid, multiply, with_like)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn triple_video(state: BiliState<'_>, bvid: String) -> Result<TripleResult, String> {
    let r = state.triple_video(&bvid).await.map_err(err)?;
    Ok(TripleResult {
        like: r.like,
        coin: r.coin,
        fav: r.fav,
    })
}

#[tauri::command]
pub async fn follow_user(
    state: BiliState<'_>,
    mid: i64,
    follow: bool,
) -> Result<(), String> {
    state.relation_modify(mid, follow).await.map_err(err)
}

#[tauri::command]
pub async fn get_comments(
    state: BiliState<'_>,
    bvid: String,
    pn: u32,
    sort: u32,
) -> Result<CommentPage, String> {
    let view = state.view(&bvid).await.map_err(err)?;
    let raw = state.comments(view.aid, pn, 20, sort).await.map_err(err)?;

    let page = raw
        .pointer("/page/num")
        .and_then(|v| v.as_u64())
        .unwrap_or(pn as u64) as u32;
    let size = raw
        .pointer("/page/size")
        .and_then(|v| v.as_u64())
        .unwrap_or(20) as u32;
    let count = raw
        .pointer("/page/count")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let acount = raw
        .pointer("/page/acount")
        .and_then(|v| v.as_i64())
        .unwrap_or(count);

    let replies: Vec<Comment> = raw
        .get("replies")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| comment_from(v, 0)).collect())
        .unwrap_or_default();

    Ok(CommentPage {
        page,
        size,
        count,
        acount,
        replies,
    })
}

/// Map a JSON reply node to our typed Comment. `depth` guards inline-replies
/// recursion: top-level is 0, nested replies are 1, anything deeper is dropped
/// (Bilibili only inlines two levels anyway).
fn comment_from(item: &Value, depth: u8) -> Option<Comment> {
    let rpid = item.get("rpid").and_then(|v| v.as_i64())?;
    let mid = item.get("mid").and_then(|v| v.as_i64()).unwrap_or(0);
    let ctime = item.get("ctime").and_then(|v| v.as_i64()).unwrap_or(0);
    let like = item.get("like").and_then(|v| v.as_i64()).unwrap_or(0);
    let rcount = item.get("rcount").and_then(|v| v.as_i64()).unwrap_or(0);
    let message = item
        .pointer("/content/message")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let member = CommentMember {
        // member.mid arrives as a string in this API; fall back to the top-level mid.
        mid: item
            .pointer("/member/mid")
            .and_then(|v| {
                v.as_str()
                    .and_then(|s| s.parse().ok())
                    .or_else(|| v.as_i64())
            })
            .unwrap_or(mid),
        uname: item
            .pointer("/member/uname")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        face: proxy_image(
            item.pointer("/member/avatar")
                .and_then(|v| v.as_str())
                .unwrap_or(""),
        ),
    };
    let location = item
        .pointer("/reply_control/location")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let replies = if depth == 0 {
        item.get("replies")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| comment_from(v, depth + 1))
                    .collect()
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    Some(Comment {
        rpid,
        mid,
        ctime,
        like,
        rcount,
        message,
        member,
        replies,
        location,
    })
}

/// Wrap an http(s) image URL in our biliimg:// scheme so requests carry the
/// Referer/Cookie headers Bilibili's CDN requires. Empty / data: URLs pass through.
fn proxy_image(url: &str) -> String {
    if url.is_empty() || url.starts_with("data:") || url.starts_with("biliimg:") {
        return url.to_string();
    }
    stream::rewrite_image(url)
}

fn track_from(t: &Value) -> Option<DashTrack> {
    // baseUrl OR base_url depending on bilibili response variant
    let base = t
        .get("baseUrl")
        .or_else(|| t.get("base_url"))
        .and_then(|v| v.as_str())?;
    // SegmentBase is the standard key, but bilibili sometimes flattens it.
    let seg = t.get("SegmentBase").or_else(|| t.get("segment_base"));

    let str_field = |container: Option<&Value>, keys: &[&str]| -> String {
        for k in keys {
            if let Some(v) = container.and_then(|c| c.get(*k)).and_then(|v| v.as_str()) {
                return v.to_string();
            }
        }
        String::new()
    };

    let init_range = if seg.is_some() {
        str_field(
            seg,
            &["initialization", "Initialization", "init_range", "init"],
        )
    } else {
        // Sometimes flattened on the track itself.
        str_field(Some(t), &["initialization", "Initialization"])
    };
    let index_range = if seg.is_some() {
        str_field(seg, &["indexRange", "index_range", "indexrange"])
    } else {
        str_field(Some(t), &["indexRange", "index_range"])
    };

    Some(DashTrack {
        id: t.get("id").and_then(|v| v.as_i64()).unwrap_or(0),
        mime: t
            .get("mimeType")
            .or_else(|| t.get("mime_type"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        codecs: t
            .get("codecs")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        bandwidth: t.get("bandwidth").and_then(|v| v.as_i64()).unwrap_or(0),
        width: t.get("width").and_then(|v| v.as_i64()).unwrap_or(0),
        height: t.get("height").and_then(|v| v.as_i64()).unwrap_or(0),
        frame_rate: t
            .get("frameRate")
            .or_else(|| t.get("frame_rate"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        base_url: stream::rewrite(base),
        init_range,
        index_range,
    })
}

fn nonempty(s: &str) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

fn card_from_rcmd_item(item: &Value) -> Option<VideoCard> {
    // rcmd items have goto="av" for normal videos. Skip ads ("ad", "live", etc.)
    let goto = item.get("goto").and_then(|v| v.as_str()).unwrap_or("");
    if goto != "av" {
        return None;
    }
    let pic = item.get("pic").and_then(|v| v.as_str()).unwrap_or("");
    let up_face = item
        .pointer("/owner/face")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let rcmd_reason = item
        .pointer("/rcmd_reason/content")
        .and_then(|v| v.as_str())
        .and_then(nonempty);
    let tname = item
        .get("tname")
        .and_then(|v| v.as_str())
        .and_then(nonempty);
    let tid = item.get("tid").and_then(|v| v.as_i64());
    Some(VideoCard {
        bvid: item.get("bvid").and_then(|v| v.as_str())?.to_string(),
        aid: item.get("id").and_then(|v| v.as_i64()).unwrap_or(0),
        cid: item.get("cid").and_then(|v| v.as_i64()).unwrap_or(0),
        title: item
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        pic: proxy_image(pic),
        duration: item.get("duration").and_then(|v| v.as_i64()).unwrap_or(0),
        view: item
            .get("stat")
            .and_then(|s| s.get("view"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0),
        up_name: item
            .pointer("/owner/name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        up_face: proxy_image(up_face),
        up_mid: item
            .pointer("/owner/mid")
            .and_then(|v| v.as_i64())
            .unwrap_or(0),
        rcmd_reason,
        tname,
        tid,
    })
}

fn card_from_related_item(item: &Value) -> Option<VideoCard> {
    let pic = item.get("pic").and_then(|v| v.as_str()).unwrap_or("");
    let up_face = item
        .pointer("/owner/face")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let tname = item
        .get("tname")
        .and_then(|v| v.as_str())
        .and_then(nonempty);
    let tid = item.get("tid").and_then(|v| v.as_i64());
    Some(VideoCard {
        bvid: item.get("bvid").and_then(|v| v.as_str())?.to_string(),
        aid: item.get("aid").and_then(|v| v.as_i64()).unwrap_or(0),
        cid: item.get("cid").and_then(|v| v.as_i64()).unwrap_or(0),
        title: item
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        pic: proxy_image(pic),
        duration: item.get("duration").and_then(|v| v.as_i64()).unwrap_or(0),
        view: item
            .get("stat")
            .and_then(|s| s.get("view"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0),
        up_name: item
            .pointer("/owner/name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        up_face: proxy_image(up_face),
        up_mid: item
            .pointer("/owner/mid")
            .and_then(|v| v.as_i64())
            .unwrap_or(0),
        rcmd_reason: None,
        tname,
        tid,
    })
}
