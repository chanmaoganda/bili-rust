use crate::components::header::Header;
use crate::routes::home::Home;
use crate::routes::watch::Watch;
use crate::state::RecommendState;
use leptos::prelude::*;
use leptos_router::components::{Route, Router, Routes};
use leptos_router::path;

#[component]
pub fn App() -> impl IntoView {
    provide_context(RecommendState::new());

    view! {
        <Router>
            <Header />
            <main>
                <Routes fallback=|| view! { <div class="empty">"Not found"</div> }>
                    <Route path=path!("/") view=Home />
                    <Route path=path!("/watch/:bvid") view=Watch />
                </Routes>
            </main>
        </Router>
    }
}
