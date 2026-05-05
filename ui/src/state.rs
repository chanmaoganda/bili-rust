use crate::types::VideoCard;
use leptos::prelude::*;

// Recommendation feed state lifted to App scope so it survives Home remounts
// (e.g. after navigating back from /watch/:bvid via mouse4 / Alt+Left).
#[derive(Clone, Copy)]
pub struct RecommendState {
    pub cards: RwSignal<Vec<VideoCard>>,
    pub fresh_idx: RwSignal<u32>,
    pub loading: RwSignal<bool>,
    pub error: RwSignal<Option<String>>,
    pub attempted: RwSignal<bool>,
    pub end_reached: RwSignal<bool>,
    pub scroll_y: RwSignal<f64>,
}

impl RecommendState {
    pub fn new() -> Self {
        Self {
            cards: RwSignal::new(Vec::new()),
            fresh_idx: RwSignal::new(0),
            loading: RwSignal::new(false),
            error: RwSignal::new(None),
            attempted: RwSignal::new(false),
            end_reached: RwSignal::new(false),
            scroll_y: RwSignal::new(0.0),
        }
    }

    pub fn reset(&self) {
        self.cards.set(Vec::new());
        self.fresh_idx.set(0);
        self.error.set(None);
        self.attempted.set(false);
        self.end_reached.set(false);
        self.scroll_y.set(0.0);
    }
}
