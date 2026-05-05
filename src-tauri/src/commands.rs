use crate::api::Bili;
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
    pub init_range: String,    // "start-end"
    pub index_range: String,   // "start-end"
}

#[tauri::command]
pub async fn get_user_info(state: BiliState<'_>) -> Result<UserInfo, String> {
    let nav = state.nav().await.map_err(err)?;
    Ok(UserInfo {
        is_login: nav.data.is_login,
        mid: nav.data.mid,
        uname: nav.data.uname,
        face: nav.data.face,
    })
}

#[tauri::command]
pub async fn get_rcmd(state: BiliState<'_>, fresh_idx: u32) -> Result<Vec<VideoCard>, String> {
    let items = state.rcmd(fresh_idx, 12).await.map_err(err)?;
    Ok(items.iter().filter_map(card_from_rcmd_item).collect())
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
    let dash = raw.get("dash").ok_or_else(|| "no dash in playurl response".to_string())?;

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

    let view_fallback = view_meta.is_some();
    let (title, duration, up_name, up_face, up_mid) = if let Some(v) = view_meta {
        (v.title, v.duration, v.owner.name, v.owner.face, v.owner.mid)
    } else {
        let dur = raw
            .pointer("/dash/duration")
            .and_then(|v| v.as_i64())
            .or_else(|| raw.get("timelength").and_then(|v| v.as_i64()).map(|ms| ms / 1000))
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
    })
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
        str_field(seg, &["initialization", "Initialization", "init_range", "init"])
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
        codecs: t.get("codecs").and_then(|v| v.as_str()).unwrap_or("").to_string(),
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

fn card_from_rcmd_item(item: &Value) -> Option<VideoCard> {
    // rcmd items have goto="av" for normal videos. Skip ads ("ad", "live", etc.)
    let goto = item.get("goto").and_then(|v| v.as_str()).unwrap_or("");
    if goto != "av" {
        return None;
    }
    Some(VideoCard {
        bvid: item.get("bvid").and_then(|v| v.as_str())?.to_string(),
        aid: item.get("id").and_then(|v| v.as_i64()).unwrap_or(0),
        cid: item.get("cid").and_then(|v| v.as_i64()).unwrap_or(0),
        title: item.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        pic: item.get("pic").and_then(|v| v.as_str()).unwrap_or("").to_string(),
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
        up_face: item
            .pointer("/owner/face")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        up_mid: item.pointer("/owner/mid").and_then(|v| v.as_i64()).unwrap_or(0),
    })
}

fn card_from_related_item(item: &Value) -> Option<VideoCard> {
    Some(VideoCard {
        bvid: item.get("bvid").and_then(|v| v.as_str())?.to_string(),
        aid: item.get("aid").and_then(|v| v.as_i64()).unwrap_or(0),
        cid: item.get("cid").and_then(|v| v.as_i64()).unwrap_or(0),
        title: item.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        pic: item.get("pic").and_then(|v| v.as_str()).unwrap_or("").to_string(),
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
        up_face: item
            .pointer("/owner/face")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        up_mid: item.pointer("/owner/mid").and_then(|v| v.as_i64()).unwrap_or(0),
    })
}
