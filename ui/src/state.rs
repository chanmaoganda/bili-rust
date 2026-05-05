use crate::types::VideoCard;
use leptos::prelude::*;

// Recommendation feed state lifted to App scope so it survives Home remounts
// (e.g. after navigating back from /watch/:bvid via mouse4 / Alt+Left).
//
// Cursor semantics (matching B站 web client):
//   fresh_idx — per-page scroll cursor; +1 on each infinite-scroll batch.
//   brush     — session-level "换一换" counter; +1 only when user explicitly
//               rotates, otherwise stays put across infinite-scroll pages.
// Mixing the two causes the ranker to treat every scroll page as a full
// refresh, hurting personalization and dedup.
#[derive(Clone, Copy)]
pub struct RecommendState {
    pub cards: RwSignal<Vec<VideoCard>>,
    pub fresh_idx: RwSignal<u32>,
    pub brush: RwSignal<u32>,
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
            brush: RwSignal::new(0),
            loading: RwSignal::new(false),
            error: RwSignal::new(None),
            attempted: RwSignal::new(false),
            end_reached: RwSignal::new(false),
            scroll_y: RwSignal::new(0.0),
        }
    }

    /// Full reset — forget brush, start a brand-new session.
    pub fn reset(&self) {
        self.cards.set(Vec::new());
        self.fresh_idx.set(0);
        self.brush.set(0);
        self.error.set(None);
        self.attempted.set(false);
        self.end_reached.set(false);
        self.scroll_y.set(0.0);
    }

    /// "换一换" — keep the session, ask for a fresh page. brush+1, fresh_idx=0.
    pub fn rotate(&self) {
        self.cards.set(Vec::new());
        self.fresh_idx.set(0);
        self.brush.update(|b| *b += 1);
        self.error.set(None);
        self.end_reached.set(false);
        self.scroll_y.set(0.0);
    }
}
