pub mod actions;
pub mod components;
pub mod hooks;
pub mod pages;
pub mod shell;
pub mod styles;

use leptos::prelude::*;
use leptos_router::{
    components::{Route, Router, Routes},
    path,
};
use leptoaster::{Toaster, provide_toaster};

use hooks::{SseStatus, provide_sse_version, use_sse_status};

/// The top-level Leptos application component.
///
/// This produces only the *body content* — Router + Routes + page views.
/// On the server, `shell::shell()` wraps this inside the full HTML document.
/// On the client, `hydrate_body(App)` hydrates this against `<body>` children.
#[component]
pub fn App() -> impl IntoView {
    provide_toaster();
    provide_sse_version();

    let status = use_sse_status();

    view! {
        <Toaster stacked=true />
        // SSE reconnecting banner
        <Show when=move || status.get() == SseStatus::Reconnecting>
            <div class="fixed top-0 left-0 right-0 z-[9998] bg-amber-500/90 dark:bg-amber-600/90 backdrop-blur-sm text-white text-center text-sm font-medium py-1.5 px-4 shadow-md"
                role="alert"
            >
                "Connection lost \u{2014} reconnecting\u{2026}"
            </div>
        </Show>
        <Router>
            <KeyboardShortcuts />
            <Routes fallback=pages::not_found::NotFoundPage>
                <Route path=path!("/") view=pages::dashboard::DashboardPage />
                <Route path=path!("/artists") view=pages::artists::ArtistsPage />
                <Route path=path!("/artists/:id") view=pages::artist_detail::ArtistDetailPage />
                <Route path=path!("/artists/:artist_id/albums/:album_id") view=pages::album_detail::AlbumDetailPage />
                <Route path=path!("/wanted") view=pages::wanted::WantedPage />
            </Routes>
        </Router>
    }
}

/// Global keyboard shortcuts — renders nothing, just installs a keydown listener.
/// Must be a child of `<Router>` so that `use_navigate()` has router context.
#[component]
fn KeyboardShortcuts() -> impl IntoView {
    #[cfg(feature = "hydrate")]
    {
        use std::cell::Cell;
        use std::rc::Rc;
        use wasm_bindgen::prelude::*;
        use wasm_bindgen::JsCast;

        let navigate = leptos_router::hooks::use_navigate();
        let pending_g: Rc<Cell<bool>> = Rc::new(Cell::new(false));
        let g_timer: Rc<Cell<Option<i32>>> = Rc::new(Cell::new(None));

        let pending_g2 = pending_g.clone();
        let g_timer2 = g_timer.clone();

        let handler = Closure::<dyn Fn(web_sys::KeyboardEvent)>::new(move |ev: web_sys::KeyboardEvent| {
            // Ignore when typing in an input, textarea, select, or contenteditable
            if let Some(target) = ev.target() {
                if let Some(el) = target.dyn_ref::<web_sys::HtmlElement>() {
                    let tag = el.tag_name().to_ascii_lowercase();
                    if tag == "input" || tag == "textarea" || tag == "select" {
                        return;
                    }
                    if el.is_content_editable() {
                        return;
                    }
                }
            }
            // Ignore if any modifier key is held (allow plain keys only)
            if ev.ctrl_key() || ev.meta_key() || ev.alt_key() {
                return;
            }

            let key = ev.key();

            // Handle pending "g" combo
            if pending_g2.get() {
                pending_g2.set(false);
                if let Some(id) = g_timer2.get() {
                    leptos::prelude::window().clear_timeout_with_handle(id);
                    g_timer2.set(None);
                }
                match key.as_str() {
                    "d" => { navigate("/", Default::default()); return; }
                    "a" => { navigate("/artists", Default::default()); return; }
                    "w" => { navigate("/wanted", Default::default()); return; }
                    _ => { return; } // unknown combo, discard
                }
            }

            match key.as_str() {
                // "/" focuses the search input on the Artists page
                "/" => {
                    ev.prevent_default();
                    let doc = leptos::prelude::document();
                    if let Ok(Some(input)) = doc.query_selector("input[aria-label='Search artists']") {
                        if let Some(el) = input.dyn_ref::<web_sys::HtmlElement>() {
                            let _ = el.focus();
                        }
                    }
                }
                // "g" starts a two-key navigation combo
                "g" => {
                    pending_g2.set(true);
                    // Timeout: if no second key within 500ms, cancel
                    let pg = pending_g2.clone();
                    let gt = g_timer2.clone();
                    let cb = Closure::once_into_js(move || {
                        pg.set(false);
                        gt.set(None);
                    });
                    if let Ok(id) = leptos::prelude::window()
                        .set_timeout_with_callback_and_timeout_and_arguments_0(
                            cb.as_ref().unchecked_ref(),
                            500,
                        )
                    {
                        g_timer2.set(Some(id));
                    }
                }
                _ => {}
            }
        });

        let _ = leptos::prelude::document()
            .add_event_listener_with_callback("keydown", handler.as_ref().unchecked_ref());
        // Intentionally leak to keep the listener alive for the app lifetime
        handler.forget();
    }
}
