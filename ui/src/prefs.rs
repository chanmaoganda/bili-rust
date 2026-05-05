use web_sys::window;

const KEY_QN: &str = "bili.preferred_qn";
const KEY_DM_ON: &str = "bili.danmaku_enabled";
const KEY_DM_OPACITY: &str = "bili.danmaku_opacity";

pub fn get_preferred_qn() -> Option<u32> {
    let storage = window()?.local_storage().ok().flatten()?;
    storage.get_item(KEY_QN).ok().flatten()?.parse().ok()
}

pub fn set_preferred_qn(qn: u32) {
    let Some(w) = window() else { return };
    let Ok(Some(storage)) = w.local_storage() else {
        return;
    };
    let _ = storage.set_item(KEY_QN, &qn.to_string());
}

pub fn get_danmaku_enabled() -> bool {
    let Some(w) = window() else { return true };
    let Ok(Some(storage)) = w.local_storage() else {
        return true;
    };
    storage
        .get_item(KEY_DM_ON)
        .ok()
        .flatten()
        .map(|v| v == "1")
        .unwrap_or(true)
}

pub fn set_danmaku_enabled(on: bool) {
    let Some(w) = window() else { return };
    let Ok(Some(storage)) = w.local_storage() else {
        return;
    };
    let _ = storage.set_item(KEY_DM_ON, if on { "1" } else { "0" });
}

pub fn get_danmaku_opacity() -> f64 {
    let Some(w) = window() else { return 0.85 };
    let Ok(Some(storage)) = w.local_storage() else {
        return 0.85;
    };
    storage
        .get_item(KEY_DM_OPACITY)
        .ok()
        .flatten()
        .and_then(|s| s.parse::<f64>().ok())
        .map(|v| v.clamp(0.2, 1.0))
        .unwrap_or(0.85)
}

pub fn set_danmaku_opacity(v: f64) {
    let Some(w) = window() else { return };
    let Ok(Some(storage)) = w.local_storage() else {
        return;
    };
    let _ = storage.set_item(KEY_DM_OPACITY, &v.clamp(0.2, 1.0).to_string());
}
