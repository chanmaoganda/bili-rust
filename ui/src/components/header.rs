use crate::api;
use leptos::prelude::*;
use leptos_router::components::A;

#[component]
pub fn Header() -> impl IntoView {
    let user = LocalResource::new(|| async { api::get_user_info().await });

    view! {
        <header class="app">
            <A href="/">
                <span class="brand">"Bili (Rust)"</span>
            </A>
            <div class="me">
                {move || {
                    user.get()
                        .map(|res| match res {
                            Ok(u) if u.is_login => {
                                view! {
                                    <img src=u.face alt="" />
                                    <span>{u.uname}</span>
                                }
                                    .into_any()
                            }
                            Ok(_) => view! { <span>"not logged in"</span> }.into_any(),
                            Err(e) => view! { <span class="error">{e}</span> }.into_any(),
                        })
                }}
            </div>
        </header>
    }
}
