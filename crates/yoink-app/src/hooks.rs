use leptos::prelude::*;

/// A monotonically increasing version counter driven by SSE notifications.
///
/// On the client (WASM), opens an `EventSource` to `/api/events` and bumps the
/// counter each time the server sends an `update` event.  On the server (SSR),
/// returns a static `0` — the real value is picked up after hydration.
///
/// Use this as the *source* signal for a `Resource` so the resource automatically
/// refetches whenever the backend state changes:
///
/// ```rust,ignore
/// let version = use_sse_version();
/// let data = Resource::new(move || version.get(), |_| get_my_data());
/// ```
pub fn use_sse_version() -> ReadSignal<u64> {
    #[cfg(not(feature = "hydrate"))]
    let (version, _) = signal(0u64);

    #[cfg(feature = "hydrate")]
    let (version, set_version) = signal(0u64);

    #[cfg(feature = "hydrate")]
    {
        use leptos::prelude::on_cleanup;

        // Fire once after the component mounts on the client.
        let cleanup = setup_event_source(set_version);
        on_cleanup(cleanup);
    }

    version
}

/// Wire up an `EventSource` and return a closure that tears it down.
#[cfg(feature = "hydrate")]
fn setup_event_source(set_version: WriteSignal<u64>) -> impl FnOnce() + 'static {
    use wasm_bindgen::prelude::*;
    use web_sys::EventSource;

    let es = EventSource::new("/api/events").expect("EventSource::new failed");

    // Listen for the named "update" event (not the generic "message" event).
    let cb = Closure::<dyn Fn(web_sys::Event)>::new(move |_evt: web_sys::Event| {
        set_version.update(|v| *v += 1);
    });
    es.add_event_listener_with_callback("update", cb.as_ref().unchecked_ref())
        .expect("addEventListener failed");
    cb.forget(); // leak intentionally — lives for the lifetime of the EventSource

    // Return a cleanup function that closes the connection.
    move || {
        es.close();
    }
}
