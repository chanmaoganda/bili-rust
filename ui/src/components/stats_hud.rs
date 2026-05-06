use crate::api;
use crate::types::{DashTrack, DecoderProbe, PlayInfo};
use js_sys::{Function, Reflect};
use leptos::prelude::*;
use leptos::task::spawn_local;
use std::cell::Cell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::HtmlVideoElement;

#[derive(Clone, Default)]
struct Sample {
    decoded: u32,
    dropped: u32,
    last_dropped_per_s: u32,
}

/// Lightweight diagnostic overlay: shows the selected DASH track codec, live
/// frame counters from `HTMLVideoElement.getVideoPlaybackQuality()`, and a host
/// probe of GStreamer hardware decoder factories. Helps the user judge whether
/// the webview is actually hardware-decoding.
#[component]
pub fn StatsHud(video: RwSignal<Option<HtmlVideoElement>>, info: PlayInfo) -> impl IntoView {
    let probe = RwSignal::new(None::<Result<DecoderProbe, String>>);
    let sample = RwSignal::new(Sample::default());

    spawn_local(async move {
        let r = api::get_decoder_probe().await;
        probe.set(Some(r));
    });

    // Sample every 1s. Hold the closure + interval id so we can cancel cleanly.
    let interval_id = StoredValue::new_local(Rc::new(Cell::new(0_i32)));
    let cb_holder = StoredValue::new_local(Rc::new(Cell::new(None::<Closure<dyn FnMut()>>)));

    Effect::new(move |_| {
        let id_holder = interval_id.with_value(|c| c.clone());
        let cb_box = cb_holder.with_value(|c| c.clone());
        if id_holder.get() != 0 {
            return; // already wired
        }
        let prev_dropped = Rc::new(Cell::new(0_u32));
        let cb = Closure::<dyn FnMut()>::new(move || {
            let Some(video_el) = video.get_untracked() else {
                return;
            };
            let video_js: JsValue = video_el.into();
            let Ok(get_q) = Reflect::get(&video_js, &"getVideoPlaybackQuality".into())
                .and_then(|v| v.dyn_into::<Function>())
            else {
                return;
            };
            let Ok(q) = get_q.call0(&video_js) else {
                return;
            };
            let decoded = Reflect::get(&q, &"totalVideoFrames".into())
                .ok()
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0) as u32;
            let dropped = Reflect::get(&q, &"droppedVideoFrames".into())
                .ok()
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0) as u32;
            let last = dropped.saturating_sub(prev_dropped.get());
            prev_dropped.set(dropped);
            sample.set(Sample {
                decoded,
                dropped,
                last_dropped_per_s: last,
            });
        });

        if let Some(window) = web_sys::window() {
            let id = window
                .set_interval_with_callback_and_timeout_and_arguments_0(
                    cb.as_ref().unchecked_ref(),
                    1000,
                )
                .unwrap_or(0);
            id_holder.set(id);
        }
        cb_box.set(Some(cb));
    });

    on_cleanup(move || {
        let id = interval_id.with_value(|c| c.get());
        if id != 0 {
            if let Some(window) = web_sys::window() {
                window.clear_interval_with_handle(id);
            }
        }
        // Drop the closure last so any in-flight tick can't reenter Rust.
        cb_holder.with_value(|c| c.set(None));
    });

    // Resolve the selected track by current_quality (matches build_mpd in player.rs).
    let selected_track: Option<DashTrack> = info
        .video
        .iter()
        .find(|t| t.id == info.current_quality as i64)
        .cloned()
        .or_else(|| info.video.first().cloned());
    let codec = selected_track
        .as_ref()
        .map(|t| t.codecs.clone())
        .unwrap_or_default();
    let dim = selected_track
        .as_ref()
        .map(|t| format!("{}×{} @ {}", t.width, t.height, t.frame_rate))
        .unwrap_or_default();
    let codec_label = codec.clone();

    view! {
        <div class="stats-hud" style="position:absolute;top:8px;right:8px;background:rgba(0,0,0,0.65);color:#eee;font:11px/1.4 monospace;padding:8px 10px;border-radius:6px;max-width:340px;pointer-events:none;z-index:10;white-space:pre-wrap;">
            <div><b>"Codec: "</b>{codec_label}</div>
            <div><b>"Track: "</b>{dim}</div>
            <div>
                <b>"Frames: "</b>
                {move || {
                    let s = sample.get();
                    format!("decoded {} / dropped {} (Δ {}/s)", s.decoded, s.dropped, s.last_dropped_per_s)
                }}
            </div>
            <div>
                <b>"HW decoders: "</b>
                {move || match probe.get() {
                    None => "probing…".to_string(),
                    // Non-Linux: gst-inspect doesn't apply. Show the webview's
                    // own pipeline; the per-frame drop heuristic below judges
                    // whether HW decode is actually engaged.
                    Some(Ok(p)) if p.platform != "linux" => {
                        if p.pipeline.is_empty() {
                            format!("{} (no host probe)", p.platform)
                        } else {
                            p.pipeline.clone()
                        }
                    }
                    Some(Ok(p)) if p.gst_inspect_ok => {
                        if p.hw_decoders.is_empty() {
                            "none".to_string()
                        } else {
                            p.hw_decoders.join(", ")
                        }
                    }
                    Some(Ok(p)) => format!(
                        "gst-inspect-1.0 unavailable: {}",
                        p.error.clone().unwrap_or_else(|| "(no detail)".into())
                    ),
                    Some(Err(e)) => format!("probe error: {e}"),
                }}
            </div>
            <div>
                <b>"Likely: "</b>
                {move || {
                    let s = sample.get();
                    let codec = codec.clone();
                    let frame_rate = selected_track
                        .as_ref()
                        .and_then(|t| t.frame_rate.parse::<f64>().ok())
                        .unwrap_or(30.0);
                    let drop_ratio = if frame_rate > 0.0 {
                        s.last_dropped_per_s as f64 / frame_rate
                    } else {
                        0.0
                    };
                    let low_drops = drop_ratio < 0.05;
                    match probe.get() {
                        None => "(measuring…)".to_string(),
                        // Off-Linux fall back to a drop-rate-only heuristic.
                        // WebView2 / WKWebView default to HW accel for H.264 /
                        // HEVC; sustained drops are the only signal we have
                        // that decode has fallen back to software.
                        Some(Ok(p)) if p.platform != "linux" => {
                            if s.decoded == 0 {
                                "(awaiting frames…)".to_string()
                            } else if low_drops {
                                format!("hardware likely ({})", p.pipeline)
                            } else {
                                "dropping frames — likely software fallback".to_string()
                            }
                        }
                        Some(Ok(p)) if p.gst_inspect_ok => {
                            let matches_codec = match_codec_to_decoder(&codec, &p.hw_decoders);
                            match (matches_codec, low_drops) {
                                (true, true) => "hardware (no drops)".to_string(),
                                (true, false) => "matching HW decoder available, but dropping frames — check pipeline".to_string(),
                                (false, _) => "software (no matching HW decoder for codec)".to_string(),
                            }
                        }
                        Some(_) => "unknown".to_string(),
                    }
                }}
            </div>
            <div style="opacity:0.7;margin-top:4px">
                {move || {
                    let p = probe.get();
                    // The VAAPI env vars only mean anything to GStreamer, so
                    // hide them off-Linux to keep the HUD signal-dense.
                    match p {
                        Some(Ok(p)) if p.platform == "linux" => format!(
                            "LIBVA_DRIVER_NAME={} GST_VAAPI_ALL_DRIVERS={}",
                            p.libva_driver.as_deref().unwrap_or("(unset)"),
                            p.gst_vaapi_all_drivers.as_deref().unwrap_or("(unset)"),
                        ),
                        _ => String::new(),
                    }
                }}
            </div>
        </div>
    }
}

fn match_codec_to_decoder(codecs: &str, hw: &[String]) -> bool {
    let c = codecs.to_ascii_lowercase();
    let needle = if c.starts_with("avc") || c.starts_with("h264") {
        "h264"
    } else if c.starts_with("hev") || c.starts_with("hvc") || c.starts_with("h265") {
        "h265"
    } else if c.starts_with("av01") {
        "av1"
    } else if c.starts_with("vp09") || c.starts_with("vp9") {
        "vp9"
    } else if c.starts_with("vp08") || c.starts_with("vp8") {
        "vp8"
    } else {
        return false;
    };
    hw.iter().any(|d| d.contains(needle))
}
