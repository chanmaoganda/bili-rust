use crate::api;
use crate::state::LoginVersion;
use leptos::prelude::*;
use leptos_router::components::A;

#[component]
pub fn Header() -> impl IntoView {
    let version = use_context::<LoginVersion>().expect("LoginVersion context missing");
    // Track LoginVersion inside the fetcher so a successful login bumps the
    // epoch and forces a re-fetch — otherwise the header would keep showing
    // "登录" until a full app restart.
    let user = LocalResource::new(move || {
        let _ = version.get();
        async { api::get_user_info().await }
    });

    view! {
        <header class="app">
            <A href="/">
                <span class="brand">"bili-rust"</span>
            </A>
            <nav class="top-nav">
                <A href="/history"><span class="nav-link">"历史"</span></A>
                <A href="/toview"><span class="nav-link">"稍后再看"</span></A>
            </nav>
            <div class="me">
                {move || {
                    user.get()
                        .map(|res| match res {
                            Ok(u) if u.is_login => {
                                let href = format!("/space/{}", u.mid);
                                view! {
                                    <A href=href>
                                        <span class="me-link">
                                            <img src=u.face alt="" />
                                            <span>{u.uname}</span>
                                        </span>
                                    </A>
                                }
                                    .into_any()
                            }
                            Ok(_) => {
                                view! { <A href="/login"><span class="login-link">"登录"</span></A> }
                                    .into_any()
                            }
                            Err(e) => view! { <span class="error">{e}</span> }.into_any(),
                        })
                }}
            </div>
        </header>
    }
}
