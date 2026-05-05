use crate::types::{DashTrack, PlayInfo};
use js_sys::{Array, Function, Reflect};
use leptos::prelude::*;
use std::collections::HashMap;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{
    HtmlElement, HtmlInputElement, HtmlTextAreaElement, HtmlVideoElement, KeyboardEvent,
};

struct KeydownGuard {
    window: web_sys::Window,
    closure: Closure<dyn FnMut(KeyboardEvent)>,
}

impl Drop for KeydownGuard {
    fn drop(&mut self) {
        let _ = self
            .window
            .remove_event_listener_with_callback("keydown", self.closure.as_ref().unchecked_ref());
    }
}

#[component]
pub fn Player(
    info: PlayInfo,
    #[prop(default = 0.0)] start_at: f64,
    #[prop(into, optional)] on_time: Option<Callback<f64>>,
) -> impl IntoView {
    let video_ref = NodeRef::<leptos::html::Video>::new();
    let info_for_effect = info.clone();
    // Holds the <video> across the component lifetime so on_cleanup can reach
    // it after Leptos has removed the node from the DOM. Without this teardown
    // dash.js keeps firing playbackTimeUpdated against a disposed Callback,
    // which panics the WASM runtime on back-nav from /watch.
    let video_holder = StoredValue::new_local(None::<HtmlVideoElement>);

    Effect::new(move |_| {
        let Some(el) = video_ref.get() else { return };
        let video: HtmlVideoElement = el.unchecked_into();
        video_holder.set_value(Some(video.clone()));

        let mpd = build_mpd(&info_for_effect);
        web_sys::console::log_1(&format!("MPD ({} bytes):\n{}", mpd.len(), mpd).into());

        let url = format!(
            "data:application/dash+xml;charset=utf-8,{}",
            js_sys::encode_uri_component(&mpd)
        );

        if let Err(e) = init_dashjs(&video, &url, start_at, on_time) {
            web_sys::console::error_1(&format!("dash.js init: {e:?}").into());
        }
    });

    on_cleanup(move || {
        video_holder.with_value(|v| {
            if let Some(video) = v {
                teardown_dashjs(video);
            }
        });
    });

    // F-key toggles fullscreen on the <video> element. Returning the listener
    // guard from the Effect ensures it's dropped (and the listener removed)
    // when the component unmounts or the effect re-runs.
    Effect::new(
        move |_prev: Option<Option<KeydownGuard>>| -> Option<KeydownGuard> {
            let el = video_ref.get()?;
            let video: HtmlVideoElement = el.unchecked_into();
            let window = web_sys::window()?;
            let document = window.document()?;

            let video_for_cb = video.clone();
            let doc_for_cb = document.clone();
            let closure = Closure::<dyn FnMut(KeyboardEvent)>::new(move |ev: KeyboardEvent| {
                if !ev.key().eq_ignore_ascii_case("f") {
                    return;
                }
                // Don't hijack F when the user is typing somewhere.
                if let Some(active) = doc_for_cb.active_element() {
                    let typing = active.dyn_ref::<HtmlInputElement>().is_some()
                        || active.dyn_ref::<HtmlTextAreaElement>().is_some()
                        || active
                            .dyn_ref::<HtmlElement>()
                            .map(|e| e.is_content_editable())
                            .unwrap_or(false);
                    if typing {
                        return;
                    }
                }
                ev.prevent_default();
                if doc_for_cb.fullscreen_element().is_some() {
                    let _ = doc_for_cb.exit_fullscreen();
                } else {
                    let _ = video_for_cb.request_fullscreen();
                }
            });

            let _ = window
                .add_event_listener_with_callback("keydown", closure.as_ref().unchecked_ref());

            Some(KeydownGuard { window, closure })
        },
    );

    view! { <video controls autoplay node_ref=video_ref></video> }
}

// Drop any dash.js MediaPlayer attached to this <video>. Safe to call when
// none is attached. Clears the __bili_dash slot so a re-init won't double-free.
fn teardown_dashjs(video: &HtmlVideoElement) {
    let video_js: JsValue = JsValue::from(video.clone());
    let Ok(prev) = Reflect::get(&video_js, &"__bili_dash".into()) else {
        return;
    };
    if prev.is_undefined() || prev.is_null() {
        return;
    }
    if let Ok(reset) = Reflect::get(&prev, &"reset".into()).and_then(|f| f.dyn_into::<Function>()) {
        let _ = reset.call0(&prev);
    }
    if let Ok(destroy) =
        Reflect::get(&prev, &"destroy".into()).and_then(|f| f.dyn_into::<Function>())
    {
        let _ = destroy.call0(&prev);
    }
    let _ = Reflect::set(&video_js, &"__bili_dash".into(), &JsValue::UNDEFINED);
}

fn dashjs_global() -> Result<JsValue, JsValue> {
    let win = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let v = Reflect::get(&win, &"dashjs".into())?;
    if v.is_undefined() || v.is_null() {
        return Err(JsValue::from_str("dashjs not loaded"));
    }
    Ok(v)
}

fn init_dashjs(
    video: &HtmlVideoElement,
    manifest_url: &str,
    start_at: f64,
    on_time: Option<Callback<f64>>,
) -> Result<(), JsValue> {
    // If a previous dash.js instance is still attached to this <video> (Leptos
    // reuses the DOM node across re-renders), tear it down so we don't end up
    // with two MediaPlayers fighting for the same element.
    teardown_dashjs(video);
    // Reset native element state so seeks and bitrate switches don't carry over.
    video.set_src("");
    video.set_current_time(0.0);

    let video_js: JsValue = JsValue::from(video.clone());
    let dashjs = dashjs_global()?;
    let mp_factory: Function = Reflect::get(&dashjs, &"MediaPlayer".into())?.dyn_into()?;
    let mp = mp_factory.call0(&JsValue::UNDEFINED)?;
    let create: Function = Reflect::get(&mp, &"create".into())?.dyn_into()?;
    let player = create.call0(&mp)?;
    Reflect::set(&video_js, &"__bili_dash".into(), &player)?;

    // Wire diagnostic events BEFORE initialize so we don't miss the first ones.
    wire_event(&player, "error", "ERROR")?;
    wire_event(&player, "playbackError", "PLAYBACK_ERROR")?;
    wire_event(&player, "manifestLoaded", "MANIFEST_LOADED")?;
    wire_event(&player, "streamInitialized", "STREAM_INITIALIZED")?;
    wire_event(&player, "canPlay", "CAN_PLAY")?;
    wire_event(&player, "playbackStarted", "PLAYBACK_STARTED")?;
    wire_event(&player, "fragmentLoadingCompleted", "FRAG_OK")?;
    wire_event(&player, "fragmentLoadingFailed", "FRAG_FAIL")?;

    // One-shot seek to the resume position on the FIRST canPlay only.
    // dash.js re-fires canPlay after every user seek; without the flag we'd
    // snap the user back to start_at every time they drag the scrubber.
    if start_at > 0.0 {
        let video_for_seek = video.clone();
        let did_seek = std::rc::Rc::new(std::cell::Cell::new(false));
        let cb = Closure::<dyn FnMut(JsValue)>::new(move |_| {
            if did_seek.get() {
                return;
            }
            did_seek.set(true);
            video_for_seek.set_current_time(start_at);
        });
        let on: Function = Reflect::get(&player, &"on".into())?.dyn_into()?;
        let args = Array::new();
        args.push(&JsValue::from_str("canPlay"));
        args.push(cb.as_ref());
        Reflect::apply(&on, &player, &args)?;
        cb.forget();
    }

    // Forward currentTime updates to the parent so it can resume on quality switch.
    if let Some(cb) = on_time {
        let last = std::rc::Rc::new(std::cell::Cell::new(0.0_f64));
        let last_clone = last.clone();
        let listener = Closure::<dyn FnMut(JsValue)>::new(move |e: JsValue| {
            let t = Reflect::get(&e, &"time".into())
                .ok()
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            // Throttle: only push when the second changes — keeps signal churn down.
            if (t - last_clone.get()).abs() >= 0.5 {
                last_clone.set(t);
                cb.run(t);
            }
        });
        let on: Function = Reflect::get(&player, &"on".into())?.dyn_into()?;
        let args = Array::new();
        args.push(&JsValue::from_str("playbackTimeUpdated"));
        args.push(listener.as_ref());
        Reflect::apply(&on, &player, &args)?;
        listener.forget();
    }

    let init: Function = Reflect::get(&player, &"initialize".into())?.dyn_into()?;
    let args = Array::new();
    args.push(&JsValue::from(video));
    args.push(&JsValue::from_str(manifest_url));
    args.push(&JsValue::from_bool(true));
    Reflect::apply(&init, &player, &args)?;
    Ok(())
}

fn wire_event(player: &JsValue, event: &str, label: &'static str) -> Result<(), JsValue> {
    let on: Function = Reflect::get(player, &"on".into())?.dyn_into()?;
    let cb = Closure::<dyn Fn(JsValue)>::new(move |e: JsValue| {
        web_sys::console::log_2(&format!("[dash:{label}]").into(), &e);
    });
    let args = Array::new();
    args.push(&JsValue::from_str(event));
    args.push(cb.as_ref());
    Reflect::apply(&on, player, &args)?;
    cb.forget();
    Ok(())
}

/// Build a static-profile MPD. Bilibili's playurl returns every quality up to
/// the requested qn, and dash.js's ABR will otherwise pick the lowest one
/// regardless of what the user selected. Filter to just the chosen quality so
/// the player has exactly one option for video.
fn build_mpd(info: &PlayInfo) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    let dur = info.duration.max(1);
    write!(
        s,
        r#"<?xml version="1.0" encoding="UTF-8"?>
<MPD xmlns="urn:mpeg:dash:schema:mpd:2011"
     profiles="urn:mpeg:dash:profile:isoff-on-demand:2011"
     type="static" mediaPresentationDuration="PT{dur}S" minBufferTime="PT4S">
  <Period>"#,
        dur = dur
    )
    .ok();

    let target_qn = info.current_quality as i64;
    let mut video_tracks: Vec<DashTrack> = if target_qn != 0 {
        info.video
            .iter()
            .filter(|t| t.id == target_qn)
            .cloned()
            .collect()
    } else {
        Vec::new()
    };
    if video_tracks.is_empty() {
        // Fallback: include everything so we at least play if the requested
        // qn isn't represented in the track list.
        video_tracks = info.video.clone();
    }

    // Group video reps by mime type — DASH AdaptationSet must be one mimeType.
    for (mime, reps) in group_by_mime(&video_tracks) {
        write!(
            s,
            r#"
    <AdaptationSet contentType="video" mimeType="{mime}" segmentAlignment="true" startWithSAP="1">"#,
            mime = xml_escape(&mime),
        )
        .ok();
        for t in reps {
            if t.init_range.is_empty() || t.index_range.is_empty() {
                continue;
            }
            write!(
                s,
                r#"
      <Representation id="v{id}" codecs="{codecs}" bandwidth="{bw}" width="{w}" height="{h}" frameRate="{fr}">
        <BaseURL>{url}</BaseURL>
        <SegmentBase indexRange="{idx}" indexRangeExact="true">
          <Initialization range="{init}"/>
        </SegmentBase>
      </Representation>"#,
                id = t.id,
                codecs = xml_escape(&t.codecs),
                bw = t.bandwidth,
                w = t.width,
                h = t.height,
                fr = xml_escape(&t.frame_rate),
                url = xml_escape(&t.base_url),
                idx = xml_escape(&t.index_range),
                init = xml_escape(&t.init_range),
            )
            .ok();
        }
        s.push_str("\n    </AdaptationSet>");
    }

    for (mime, reps) in group_by_mime(&info.audio) {
        write!(
            s,
            r#"
    <AdaptationSet contentType="audio" mimeType="{mime}" segmentAlignment="true" startWithSAP="1">"#,
            mime = xml_escape(&mime),
        )
        .ok();
        for t in reps {
            if t.init_range.is_empty() || t.index_range.is_empty() {
                continue;
            }
            write!(
                s,
                r#"
      <Representation id="a{id}" codecs="{codecs}" bandwidth="{bw}">
        <BaseURL>{url}</BaseURL>
        <SegmentBase indexRange="{idx}" indexRangeExact="true">
          <Initialization range="{init}"/>
        </SegmentBase>
      </Representation>"#,
                id = t.id,
                codecs = xml_escape(&t.codecs),
                bw = t.bandwidth,
                url = xml_escape(&t.base_url),
                idx = xml_escape(&t.index_range),
                init = xml_escape(&t.init_range),
            )
            .ok();
        }
        s.push_str("\n    </AdaptationSet>");
    }

    s.push_str("\n  </Period>\n</MPD>\n");
    s
}

fn group_by_mime(tracks: &[DashTrack]) -> Vec<(String, Vec<DashTrack>)> {
    let mut map: HashMap<String, Vec<DashTrack>> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for t in tracks {
        let mime = if t.mime.is_empty() {
            // Bilibili sometimes omits mimeType; infer from codecs.
            if t.codecs.starts_with("avc")
                || t.codecs.starts_with("hev")
                || t.codecs.starts_with("hvc")
                || t.codecs.starts_with("av01")
            {
                "video/mp4".to_string()
            } else {
                "audio/mp4".to_string()
            }
        } else {
            t.mime.clone()
        };
        if !map.contains_key(&mime) {
            order.push(mime.clone());
        }
        map.entry(mime).or_default().push(t.clone());
    }
    order
        .into_iter()
        .map(|m| {
            let mut v = map.remove(&m).unwrap();
            v.sort_by_key(|t| t.bandwidth); // ascending — dash.js starts low
            (m, v)
        })
        .collect()
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
