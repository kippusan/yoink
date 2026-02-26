use leptos::prelude::*;

/// Set the document's `<title>` on the client side.
///
/// On SSR this is a no-op — the shell always renders `<title>yoink</title>`.
/// Once hydrated, the title updates immediately.
pub fn set_page_title(title: &str) {
    #[cfg(feature = "hydrate")]
    {
        let full = if title.is_empty() {
            "yoink".to_string()
        } else {
            format!("{title} \u{2014} yoink")
        };
        if let Ok(doc) = leptos::prelude::document().query_selector("title")
            && let Some(el) = doc
        {
            el.set_text_content(Some(&full));
        }
    }
    #[cfg(not(feature = "hydrate"))]
    {
        let _ = title;
    }
}

/// SSE connection status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SseStatus {
    Connected,
    Reconnecting,
}

/// Provide SSE version + status signals via Leptos context.
/// Call once from the top-level `App` component.
pub fn provide_sse_version() {
    let (version, status) = create_sse_signals();
    provide_context(SseVersion(version));
    provide_context(SseConnectionStatus(status));
}

/// Read the SSE version signal from context. Panics if `provide_sse_version` wasn't called.
pub fn use_sse_version() -> ReadSignal<u64> {
    expect_context::<SseVersion>().0
}

/// Read the SSE connection status from context. Panics if `provide_sse_version` wasn't called.
pub fn use_sse_status() -> ReadSignal<SseStatus> {
    expect_context::<SseConnectionStatus>().0
}

#[derive(Clone, Copy)]
struct SseVersion(ReadSignal<u64>);

#[derive(Clone, Copy)]
struct SseConnectionStatus(ReadSignal<SseStatus>);

fn create_sse_signals() -> (ReadSignal<u64>, ReadSignal<SseStatus>) {
    #[cfg(not(feature = "hydrate"))]
    let (version, _) = signal(0u64);

    #[cfg(feature = "hydrate")]
    let (version, set_version) = signal(0u64);

    #[cfg(not(feature = "hydrate"))]
    let (status, _) = signal(SseStatus::Connected);

    #[cfg(feature = "hydrate")]
    let (status, set_status) = signal(SseStatus::Connected);

    #[cfg(feature = "hydrate")]
    {
        setup_event_source(set_version, set_status);
    }

    (version, status)
}

/// Wire up an `EventSource` with auto-reconnect.
///
/// We intentionally do NOT register on_cleanup here because the SSE connection
/// is meant to live for the entire app lifetime (provided once at the root).
/// The browser will close it when the page unloads.
#[cfg(feature = "hydrate")]
fn setup_event_source(set_version: WriteSignal<u64>, set_status: WriteSignal<SseStatus>) {
    use wasm_bindgen::prelude::*;

    // We use a simple recursive approach: connect(), and on error schedule
    // a reconnect after 3 seconds.
    fn connect(set_version: WriteSignal<u64>, set_status: WriteSignal<SseStatus>) {
        let es = web_sys::EventSource::new("/api/events").expect("EventSource::new failed");

        // on open -> connected
        {
            let cb = Closure::<dyn Fn(web_sys::Event)>::new(move |_: web_sys::Event| {
                set_status.set(SseStatus::Connected);
            });
            es.set_onopen(Some(cb.as_ref().unchecked_ref()));
            cb.forget();
        }

        // on "update" event -> bump version
        {
            let cb = Closure::<dyn Fn(web_sys::Event)>::new(move |_: web_sys::Event| {
                set_version.update(|v| *v += 1);
            });
            es.add_event_listener_with_callback("update", cb.as_ref().unchecked_ref())
                .expect("addEventListener failed");
            cb.forget();
        }

        // on error -> set reconnecting, close this ES, schedule reconnect after 3s
        {
            let cb = Closure::<dyn Fn(web_sys::Event)>::new(move |_: web_sys::Event| {
                set_status.set(SseStatus::Reconnecting);

                // The EventSource auto-closes on certain errors, but we close explicitly
                // to be safe. We can't hold a reference to `es` here without Rc, so
                // we rely on the browser's EventSource reconnect being disabled by
                // our error handler. Instead, schedule our own reconnect.
                let reconnect_cb = Closure::once_into_js(move || {
                    connect(set_version, set_status);
                });
                let _ = leptos::prelude::window()
                    .set_timeout_with_callback_and_timeout_and_arguments_0(
                        reconnect_cb.as_ref().unchecked_ref(),
                        3_000,
                    );
            });
            es.set_onerror(Some(cb.as_ref().unchecked_ref()));
            cb.forget();
        }

        // Leak the EventSource intentionally — it lives for the app lifetime.
        // The browser's built-in EventSource reconnect will also fire, but our
        // error handler ensures we update status correctly.
        std::mem::forget(es);
    }

    connect(set_version, set_status);
}
