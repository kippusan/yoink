pub mod components;
pub mod pages;
pub mod shell;

use leptos::prelude::*;
use leptos_router::{
    components::{Route, Router, Routes},
    path,
};

/// The top-level Leptos application component.
///
/// This produces only the *body content* — Router + Routes + page views.
/// On the server, `shell::shell()` wraps this inside the full HTML document.
/// On the client, `hydrate_body(App)` hydrates this against `<body>` children.
#[component]
pub fn App() -> impl IntoView {
    view! {
        <Router>
            <Routes fallback=|| "Not found.">
                <Route path=path!("/") view=pages::dashboard::DashboardPage />
                <Route path=path!("/artists") view=pages::artists::ArtistsPage />
                <Route path=path!("/artists/:id") view=pages::artist_detail::ArtistDetailPage />
                <Route path=path!("/wanted") view=pages::wanted::WantedPage />
            </Routes>
        </Router>
    }
}
