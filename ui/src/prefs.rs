use web_sys::window;

const KEY_QN: &str = "bili.preferred_qn";
const KEY_DM_ON: &str = "bili.danmaku_enabled";
const KEY_DM_OPACITY: &str = "bili.danmaku_opacity";
const KEY_DM_AREA: &str = "bili.danmaku_area";
const KEY_DM_SPEED: &str = "bili.danmaku_speed";
const KEY_DM_SHOW_SCROLL: &str = "bili.danmaku_show_scroll";
const KEY_DM_SHOW_TOP: &str = "bili.danmaku_show_top";
const KEY_DM_SHOW_BOTTOM: &str = "bili.danmaku_show_bottom";
const KEY_DM_FONT_SCALE: &str = "bili.danmaku_font_scale";
const KEY_DM_DENSITY: &str = "bili.danmaku_density";
const KEY_STATS_HUD: &str = "bili.stats_hud_enabled";

const DM_AREA_DEFAULT: f64 = 1.0;
const DM_SPEED_DEFAULT: f64 = 1.0;
const DM_FONT_SCALE_DEFAULT: f64 = 1.0;
const DM_OPACITY_DEFAULT: f64 = 0.85;

fn storage() -> Option<web_sys::Storage> {
    window()?.local_storage().ok().flatten()
}

fn get_f64(key: &str, default: f64, min: f64, max: f64) -> f64 {
    storage()
        .and_then(|s| s.get_item(key).ok().flatten())
        .and_then(|s| s.parse::<f64>().ok())
        .map(|v| v.clamp(min, max))
        .unwrap_or(default)
}

fn set_f64(key: &str, v: f64, min: f64, max: f64) {
    if let Some(s) = storage() {
        let _ = s.set_item(key, &v.clamp(min, max).to_string());
    }
}

fn get_bool(key: &str, default: bool) -> bool {
    storage()
        .and_then(|s| s.get_item(key).ok().flatten())
        .map(|v| v == "1")
        .unwrap_or(default)
}

fn set_bool(key: &str, on: bool) {
    if let Some(s) = storage() {
        let _ = s.set_item(key, if on { "1" } else { "0" });
    }
}

pub fn get_preferred_qn() -> Option<u32> {
    storage()?.get_item(KEY_QN).ok().flatten()?.parse().ok()
}

pub fn set_preferred_qn(qn: u32) {
    if let Some(s) = storage() {
        let _ = s.set_item(KEY_QN, &qn.to_string());
    }
}

pub fn get_danmaku_enabled() -> bool {
    get_bool(KEY_DM_ON, true)
}

pub fn set_danmaku_enabled(on: bool) {
    set_bool(KEY_DM_ON, on);
}

pub fn get_danmaku_opacity() -> f64 {
    get_f64(KEY_DM_OPACITY, DM_OPACITY_DEFAULT, 0.0, 1.0)
}

pub fn set_danmaku_opacity(v: f64) {
    set_f64(KEY_DM_OPACITY, v, 0.0, 1.0);
}

/// Vertical fraction of the player allotted to scrolling danmaku.
/// Valid values: 0.25, 0.5, 0.75, 1.0. Top/bottom-anchored modes ignore this.
pub fn get_danmaku_area() -> f64 {
    get_f64(KEY_DM_AREA, DM_AREA_DEFAULT, 0.25, 1.0)
}

pub fn set_danmaku_area(v: f64) {
    set_f64(KEY_DM_AREA, v, 0.25, 1.0);
}

/// Scroll-speed multiplier. 1.0 = original 10s travel time.
pub fn get_danmaku_speed() -> f64 {
    get_f64(KEY_DM_SPEED, DM_SPEED_DEFAULT, 0.5, 2.0)
}

pub fn set_danmaku_speed(v: f64) {
    set_f64(KEY_DM_SPEED, v, 0.5, 2.0);
}

pub fn get_danmaku_font_scale() -> f64 {
    get_f64(KEY_DM_FONT_SCALE, DM_FONT_SCALE_DEFAULT, 0.75, 1.5)
}

pub fn set_danmaku_font_scale(v: f64) {
    set_f64(KEY_DM_FONT_SCALE, v, 0.75, 1.5);
}

pub fn get_danmaku_show_scroll() -> bool {
    get_bool(KEY_DM_SHOW_SCROLL, true)
}

pub fn set_danmaku_show_scroll(on: bool) {
    set_bool(KEY_DM_SHOW_SCROLL, on);
}

pub fn get_danmaku_show_top() -> bool {
    get_bool(KEY_DM_SHOW_TOP, true)
}

pub fn set_danmaku_show_top(on: bool) {
    set_bool(KEY_DM_SHOW_TOP, on);
}

pub fn get_danmaku_show_bottom() -> bool {
    get_bool(KEY_DM_SHOW_BOTTOM, true)
}

pub fn set_danmaku_show_bottom(on: bool) {
    set_bool(KEY_DM_SHOW_BOTTOM, on);
}

/// Density cap on simultaneously active scrolling danmaku.
/// 0 = unlimited; otherwise the spawn loop drops new entries past the cap.
pub fn get_danmaku_density() -> u32 {
    storage()
        .and_then(|s| s.get_item(KEY_DM_DENSITY).ok().flatten())
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0)
}

pub fn set_danmaku_density(v: u32) {
    if let Some(s) = storage() {
        let _ = s.set_item(KEY_DM_DENSITY, &v.to_string());
    }
}

pub fn get_stats_hud_enabled() -> bool {
    get_bool(KEY_STATS_HUD, false)
}

pub fn set_stats_hud_enabled(on: bool) {
    set_bool(KEY_STATS_HUD, on);
}
