use crate::components::header::Header;
use crate::components::nav_shortcuts::NavShortcuts;
use crate::routes::followees::Followees;
use crate::routes::history::History;
use crate::routes::home::Home;
use crate::routes::login::Login;
use crate::routes::space::Space;
use crate::routes::toview::Toview;
use crate::routes::watch::Watch;
use crate::state::{FolloweesState, LoginVersion, RecommendState};
use leptos::prelude::*;
use leptos_router::components::{Route, Router, Routes};
use leptos_router::path;

#[component]
pub fn App() -> impl IntoView {
    provide_context(RecommendState::new());
    provide_context(LoginVersion::new());
    provide_context(FolloweesState::new());

    view! {
        <Router>
            <Header />
            <NavShortcuts />
            <main>
                <Routes fallback=|| view! { <div class="empty">"Not found"</div> }>
                    <Route path=path!("/") view=Home />
                    <Route path=path!("/watch/:bvid") view=Watch />
                    <Route path=path!("/login") view=Login />
                    <Route path=path!("/space/:mid") view=Space />
                    <Route path=path!("/history") view=History />
                    <Route path=path!("/toview") view=Toview />
                    <Route path=path!("/followees") view=Followees />
                </Routes>
            </main>
        </Router>
    }
}
