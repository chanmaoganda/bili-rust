use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct UserInfo {
    pub is_login: bool,
    pub mid: i64,
    pub uname: String,
    pub face: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
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
    #[serde(default)]
    pub rcmd_reason: Option<String>,
    #[serde(default)]
    pub tname: Option<String>,
    #[serde(default)]
    pub tid: Option<i64>,
    /// Unix seconds of last watch (history only).
    #[serde(default)]
    pub view_at: Option<i64>,
    /// Unix seconds when added to Watch Later (toview only).
    #[serde(default)]
    pub add_at: Option<i64>,
    /// Seconds watched into the video; -1 means finished.
    #[serde(default)]
    pub progress: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct HistoryPage {
    pub list: Vec<VideoCard>,
    pub cursor_max: i64,
    pub cursor_view_at: i64,
    pub has_more: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct ToviewPage {
    pub count: i32,
    pub list: Vec<VideoCard>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
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
    #[serde(default)]
    pub accept_quality: Vec<u32>,
    #[serde(default)]
    pub accept_description: Vec<String>,
    #[serde(default)]
    pub current_quality: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
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

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct DashTrack {
    pub id: i64,
    pub mime: String,
    pub codecs: String,
    pub bandwidth: i64,
    pub width: i64,
    pub height: i64,
    pub frame_rate: String,
    pub base_url: String,
    pub init_range: String,
    pub index_range: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Default)]
pub struct DecoderProbe {
    #[serde(default)]
    pub hw_decoders: Vec<String>,
    #[serde(default)]
    pub libva_driver: Option<String>,
    #[serde(default)]
    pub gst_vaapi_all_drivers: Option<String>,
    #[serde(default)]
    pub webkit_disable_compositing: Option<String>,
    #[serde(default)]
    pub gst_inspect_ok: bool,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Danmaku {
    pub progress: f64,
    pub mode: u8,
    pub size: u16,
    pub color: u32,
    pub text: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct CommentMember {
    pub mid: i64,
    pub uname: String,
    pub face: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Comment {
    pub rpid: i64,
    pub mid: i64,
    pub ctime: i64,
    pub like: i64,
    pub rcount: i64,
    pub message: String,
    pub member: CommentMember,
    #[serde(default)]
    pub replies: Vec<Comment>,
    #[serde(default)]
    pub location: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Default)]
pub struct ActionState {
    #[serde(default)]
    pub liked: bool,
    #[serde(default)]
    pub coined: i64,
    #[serde(default)]
    pub favorited: bool,
    #[serde(default)]
    pub followed: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Default)]
pub struct TripleResult {
    #[serde(default)]
    pub like: bool,
    #[serde(default)]
    pub coin: bool,
    #[serde(default)]
    pub fav: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct CommentPage {
    pub page: u32,
    pub size: u32,
    pub count: i64,
    pub acount: i64,
    pub replies: Vec<Comment>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct SpaceInfo {
    pub mid: i64,
    pub name: String,
    pub face: String,
    pub sign: String,
    pub level: i64,
    #[serde(default)]
    pub top_photo: String,
    pub following: i64,
    pub follower: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct SpaceVideoPage {
    pub list: Vec<VideoCard>,
    pub page: u32,
    pub size: u32,
    pub count: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct QrStart {
    pub url: String,
    pub qrcode_key: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct QrPoll {
    pub status: String,
}

/// Format a Unix-seconds timestamp as a relative Chinese label.
pub fn fmt_ctime(ctime: i64) -> String {
    if ctime <= 0 {
        return String::new();
    }
    let now = (js_sys::Date::now() / 1000.0) as i64;
    let diff = now - ctime;
    if diff < 60 {
        "刚刚".to_string()
    } else if diff < 3600 {
        format!("{}分钟前", diff / 60)
    } else if diff < 86_400 {
        format!("{}小时前", diff / 3600)
    } else if diff < 86_400 * 30 {
        format!("{}天前", diff / 86_400)
    } else {
        let d = js_sys::Date::new(&((ctime as f64) * 1000.0).into());
        let y = d.get_full_year();
        let m = d.get_month() + 1;
        let day = d.get_date();
        format!("{y}-{m:02}-{day:02}")
    }
}

pub fn fmt_views(n: i64) -> String {
    if n >= 10_000 {
        format!("{:.1}万", n as f64 / 10_000.0)
    } else {
        n.to_string()
    }
}

pub fn fmt_duration(s: i64) -> String {
    let m = s / 60;
    let r = s % 60;
    if m >= 60 {
        let h = m / 60;
        let mm = m % 60;
        format!("{h}:{mm:02}:{r:02}")
    } else {
        format!("{m}:{r:02}")
    }
}
