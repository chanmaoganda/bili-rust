use web_sys::window;

const KEY: &str = "bili.preferred_qn";

pub fn get_preferred_qn() -> Option<u32> {
    let storage = window()?.local_storage().ok().flatten()?;
    storage.get_item(KEY).ok().flatten()?.parse().ok()
}

pub fn set_preferred_qn(qn: u32) {
    let Some(w) = window() else { return };
    let Ok(Some(storage)) = w.local_storage() else {
        return;
    };
    let _ = storage.set_item(KEY, &qn.to_string());
}
