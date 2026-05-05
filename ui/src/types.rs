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
