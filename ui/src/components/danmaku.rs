use crate::types::Danmaku;
use leptos::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{HtmlDivElement, HtmlElement, HtmlVideoElement};

const SCROLL_LANES: usize = 12;
const TB_LANES: usize = 4;
const SCROLL_TRAVEL_S: f64 = 10.0;
const TB_HOLD_S: f64 = 5.0;
const SEEK_THRESHOLD_S: f64 = 2.0;

#[derive(Copy, Clone)]
enum Kind {
    Scroll,
    Top,
    Bottom,
}

struct Active {
    el: HtmlElement,
    kind: Kind,
    spawn_t: f64,
    travel: f64, // container_w + width, cached so we don't read layout each frame
}

struct LoopState {
    cursor: usize,
    last_t: f64,
    active: Vec<Active>,
    lane_free_scroll: [f64; SCROLL_LANES],
    lane_free_top: [f64; TB_LANES],
    lane_free_bot: [f64; TB_LANES],
    raf_id: i32,
}

impl LoopState {
    fn new() -> Self {
        Self {
            cursor: 0,
            last_t: -1.0,
            active: Vec::new(),
            lane_free_scroll: [0.0; SCROLL_LANES],
            lane_free_top: [0.0; TB_LANES],
            lane_free_bot: [0.0; TB_LANES],
            raf_id: 0,
        }
    }

    fn clear_dom(&mut self, layer: &HtmlDivElement) {
        for a in self.active.drain(..) {
            let _ = layer.remove_child(&a.el);
        }
        self.lane_free_scroll = [0.0; SCROLL_LANES];
        self.lane_free_top = [0.0; TB_LANES];
        self.lane_free_bot = [0.0; TB_LANES];
    }
}

#[component]
pub fn DanmakuOverlay(
    video: RwSignal<Option<HtmlVideoElement>>,
    danmakus: Signal<Option<Vec<Danmaku>>>,
    enabled: Signal<bool>,
    opacity: Signal<f64>,
) -> impl IntoView {
    let layer_ref = NodeRef::<leptos::html::Div>::new();

    // Hold state across rAF callbacks. on_cleanup requires Send+Sync, so we
    // park non-Send types in StoredValue::new_local (CSR-only, like player.rs)
    // and clone the inner Rc out when entering JS-callback context.
    let state_sv = StoredValue::new_local(Rc::new(RefCell::new(LoopState::new())));
    let raf_sv = StoredValue::new_local(Rc::new(RefCell::new(None::<Closure<dyn FnMut(f64)>>)));
    let dm_sv = StoredValue::new_local(Rc::new(RefCell::new(Rc::new(Vec::<Danmaku>::new()))));

    Effect::new(move |_| {
        let Some(layer_el) = layer_ref.get() else {
            return;
        };
        let layer: HtmlDivElement = layer_el.unchecked_into();

        let state = state_sv.with_value(|rc| rc.clone());
        let raf_holder = raf_sv.with_value(|rc| rc.clone());
        let dm_holder = dm_sv.with_value(|rc| rc.clone());

        // Track danmaku list — when it changes (cid switch), rebuild snapshot
        // and reset state. `with` reads reactively so this Effect re-runs.
        let next: Vec<Danmaku> = danmakus.with(|d| d.clone()).unwrap_or_default();
        *dm_holder.borrow_mut() = Rc::new(next);
        {
            let mut s = state.borrow_mut();
            s.clear_dom(&layer);
            s.cursor = 0;
            s.last_t = -1.0;
        }

        // Keep a single rAF chain alive for the component lifetime.
        if raf_holder.borrow().is_some() {
            return;
        }

        let video_sig = video;
        let enabled_sig = enabled;
        let opacity_sig = opacity;
        let layer_for_loop = layer.clone();
        let state_for_loop = state.clone();
        let raf_for_loop = raf_holder.clone();
        let dm_for_loop = dm_holder.clone();

        let cb = Closure::<dyn FnMut(f64)>::new(move |_now: f64| {
            let Some(window) = web_sys::window() else {
                return;
            };
            let layer = layer_for_loop.clone();
            let video_el = video_sig.get_untracked();

            tick(
                &state_for_loop,
                &dm_for_loop,
                &layer,
                video_el.as_ref(),
                enabled_sig.get_untracked(),
                opacity_sig.get_untracked(),
            );

            if let Some(c) = raf_for_loop.borrow().as_ref() {
                let id = window
                    .request_animation_frame(c.as_ref().unchecked_ref())
                    .unwrap_or(0);
                state_for_loop.borrow_mut().raf_id = id;
            }
        });

        if let Some(window) = web_sys::window() {
            let id = window
                .request_animation_frame(cb.as_ref().unchecked_ref())
                .unwrap_or(0);
            state.borrow_mut().raf_id = id;
        }
        *raf_holder.borrow_mut() = Some(cb);
    });

    on_cleanup(move || {
        if let Some(window) = web_sys::window() {
            let id = state_sv.with_value(|rc| rc.borrow().raf_id);
            if id != 0 {
                let _ = window.cancel_animation_frame(id);
            }
        }
        // Drop the closure so JS cannot re-enter Rust after teardown.
        raf_sv.with_value(|rc| {
            rc.borrow_mut().take();
        });
    });

    view! { <div class="danmaku-layer" node_ref=layer_ref></div> }
}

fn tick(
    state: &Rc<RefCell<LoopState>>,
    dm: &Rc<RefCell<Rc<Vec<Danmaku>>>>,
    layer: &HtmlDivElement,
    video: Option<&HtmlVideoElement>,
    enabled: bool,
    opacity: f64,
) {
    if !enabled {
        let mut s = state.borrow_mut();
        if !s.active.is_empty() {
            s.clear_dom(layer);
        }
        return;
    }
    let Some(video) = video else {
        return;
    };

    let t = video.current_time();
    let list = dm.borrow().clone();
    if list.is_empty() {
        return;
    }
    let mut s = state.borrow_mut();

    // Seek detection: large jump (forward or backward) — purge and reseat cursor.
    if (t - s.last_t).abs() > SEEK_THRESHOLD_S {
        s.clear_dom(layer);
        s.cursor = list.partition_point(|d| d.progress < t);
    }
    s.last_t = t;

    // Container width drives both scroll travel and lane height calc.
    let container_w = layer.client_width() as f64;
    let container_h = layer.client_height() as f64;
    if container_w <= 0.0 || container_h <= 0.0 {
        return;
    }

    // Lane height: keep ≤ 28px lanes for default size. Scale with container.
    let lane_h = (container_h / SCROLL_LANES as f64).clamp(20.0, 36.0);

    // Spawn new entries that have come due.
    while s.cursor < list.len() && list[s.cursor].progress <= t + 0.05 {
        let d = list[s.cursor].clone();
        s.cursor += 1;
        spawn_one(&mut s, layer, &d, t, container_w, lane_h, opacity);
    }

    // Update / cull active.
    let mut i = 0;
    while i < s.active.len() {
        let a = &s.active[i];
        let age = t - a.spawn_t;
        let done = match a.kind {
            Kind::Scroll => age >= a.travel / scroll_speed(a.travel),
            Kind::Top | Kind::Bottom => age >= TB_HOLD_S,
        };
        if done {
            let removed = s.active.swap_remove(i);
            let _ = layer.remove_child(&removed.el);
            continue;
        }
        if let Kind::Scroll = a.kind {
            // x = container_w - speed * age; element position-anchored at left=0,
            // so translateX moves it from container_w into negative width.
            let x = container_w - scroll_speed(a.travel) * age;
            let _ =
                a.el.style()
                    .set_property("transform", &format!("translateX({x:.1}px)"));
        }
        i += 1;
    }
}

fn scroll_speed(travel: f64) -> f64 {
    travel / SCROLL_TRAVEL_S
}

fn spawn_one(
    s: &mut LoopState,
    layer: &HtmlDivElement,
    d: &Danmaku,
    t: f64,
    container_w: f64,
    lane_h: f64,
    opacity: f64,
) {
    let kind = match d.mode {
        4 => Kind::Bottom,
        5 => Kind::Top,
        _ => Kind::Scroll,
    };

    // Pick a free lane; drop on saturation (matches Bilibili behavior).
    let lane = match kind {
        Kind::Scroll => find_free_lane(&s.lane_free_scroll, t),
        Kind::Top => find_free_lane(&s.lane_free_top, t),
        Kind::Bottom => find_free_lane(&s.lane_free_bot, t),
    };
    let Some(lane) = lane else { return };

    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(document) = window.document() else {
        return;
    };
    let Ok(span_node) = document.create_element("span") else {
        return;
    };
    let span: HtmlElement = match span_node.dyn_into::<HtmlElement>() {
        Ok(s) => s,
        Err(_) => return,
    };
    span.set_text_content(Some(&d.text));

    let style = span.style();
    let font_px = font_size_px(d.size);
    let _ = style.set_property("font-size", &format!("{font_px}px"));
    if d.color != 0xFFFFFF {
        let _ = style.set_property("color", &format!("#{:06X}", d.color));
    }
    let _ = style.set_property("opacity", &format!("{opacity:.3}"));

    // Lane positioning. Scroll/top lanes count from the top of the container;
    // bottom lanes count up from the bottom edge. Real Bilibili lets scroll
    // and top/bottom regions overlap visually, which we mirror here.
    match kind {
        Kind::Scroll | Kind::Top => {
            let top_px = lane as f64 * lane_h;
            let _ = style.set_property("top", &format!("{top_px:.1}px"));
        }
        Kind::Bottom => {
            let bot_px = lane as f64 * lane_h;
            let _ = style.set_property("bottom", &format!("{bot_px:.1}px"));
        }
    }
    let _ = style.set_property("left", "0");

    if let Kind::Scroll = kind {
        // Start fully off-screen on the right edge.
        let _ = style.set_property("transform", &format!("translateX({container_w:.1}px)"));
    } else {
        // Centered horizontally for top/bottom.
        let _ = style.set_property("left", "50%");
        let _ = style.set_property("transform", "translateX(-50%)");
    }

    if layer.append_child(&span).is_err() {
        return;
    }

    // Now the element is in the DOM, measure it and set the lane's "free at" time.
    let width = span.offset_width() as f64;

    match kind {
        Kind::Scroll => {
            let travel = container_w + width;
            let speed = scroll_speed(travel);
            // Lane is occupied until the entry's tail clears the right edge.
            // Entry tail clears when speed * dt = width  →  dt = width / speed.
            // Free at t + dt + small gap to keep density readable.
            let dt = width / speed;
            s.lane_free_scroll[lane] = t + dt + 0.15;
            s.active.push(Active {
                el: span,
                kind,
                spawn_t: t,
                travel,
            });
        }
        Kind::Top => {
            s.lane_free_top[lane] = t + TB_HOLD_S;
            s.active.push(Active {
                el: span,
                kind,
                spawn_t: t,
                travel: 0.0,
            });
        }
        Kind::Bottom => {
            s.lane_free_bot[lane] = t + TB_HOLD_S;
            s.active.push(Active {
                el: span,
                kind,
                spawn_t: t,
                travel: 0.0,
            });
        }
    }
}

fn find_free_lane(lanes: &[f64], t: f64) -> Option<usize> {
    lanes.iter().position(|&free_at| free_at <= t)
}

fn font_size_px(size: u16) -> u16 {
    match size {
        s if s <= 18 => 18,
        s if s >= 36 => 28,
        _ => 22,
    }
}
