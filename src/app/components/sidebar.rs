use leptos::prelude::*;

// ── SVG icons ───────────────────────────────────────────────

const ICON_HOUSE: &str = r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 9l9-7 9 7v11a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/><polyline points="9 22 9 12 15 12 15 22"/></svg>"#;

const ICON_MIC: &str = r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 1a3 3 0 0 0-3 3v8a3 3 0 0 0 6 0V4a3 3 0 0 0-3-3z"/><path d="M19 10v2a7 7 0 0 1-14 0v-2"/><line x1="12" y1="19" x2="12" y2="23"/><line x1="8" y1="23" x2="16" y2="23"/></svg>"#;

const ICON_HEART: &str = r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20.84 4.61a5.5 5.5 0 0 0-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 0 0-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 0 0 0-7.78z"/></svg>"#;

const ICON_MUSIC: &str = r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/></svg>"#;

const ICON_SUN_MOON: &str = r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="5"/><line x1="12" y1="1" x2="12" y2="3"/><line x1="12" y1="21" x2="12" y2="23"/><line x1="4.22" y1="4.22" x2="5.64" y2="5.64"/><line x1="18.36" y1="18.36" x2="19.78" y2="19.78"/><line x1="1" y1="12" x2="3" y2="12"/><line x1="21" y1="12" x2="23" y2="12"/><line x1="4.22" y1="19.78" x2="5.64" y2="18.36"/><line x1="18.36" y1="5.64" x2="19.78" y2="4.22"/></svg>"#;

// ── Tailwind class constants ────────────────────────────────

const NAV_BASE: &str = "flex items-center gap-3 py-2.5 px-4 text-zinc-400/90 no-underline text-sm font-medium border-l-3 border-transparent transition-[background,color,border-color] duration-150 hover:bg-white/[.04] hover:text-zinc-200 [&_svg]:size-[18px] [&_svg]:shrink-0";
const NAV_ACTIVE: &str = "flex items-center gap-3 py-2.5 px-4 text-zinc-100 no-underline text-sm font-medium border-l-3 border-blue-500 bg-blue-500/[.08] transition-[background,color,border-color] duration-150 hover:bg-white/[.04] hover:text-zinc-200 [&_svg]:size-[18px] [&_svg]:shrink-0";

// ── Theme toggle helpers (client-only) ──────────────────────

/// Read the current dark-mode state from the DOM.
/// On the server this always returns `true` (the bootstrap script defaults to dark).
fn read_is_dark() -> bool {
    #[cfg(feature = "hydrate")]
    {
        leptos::prelude::document()
            .document_element()
            .map(|el| el.class_list().contains("dark"))
            .unwrap_or(true)
    }
    #[cfg(not(feature = "hydrate"))]
    {
        true
    }
}

/// Toggle dark mode: flip the `dark` class on `<html>` and persist to localStorage.
#[cfg(feature = "hydrate")]
fn toggle_dark_mode() -> bool {
    let doc_el = leptos::prelude::document()
        .document_element()
        .expect("missing <html>");
    let class_list = doc_el.class_list();
    let _ = class_list.toggle("dark");
    let is_dark = class_list.contains("dark");

    if let Ok(Some(storage)) = leptos::prelude::window().local_storage() {
        let _ = storage.set_item("theme", if is_dark { "dark" } else { "light" });
    }
    is_dark
}

/// Navigation sidebar — shared across all Leptos-rendered pages.
#[component]
pub fn Sidebar(#[prop(into)] active: String) -> impl IntoView {
    let dashboard_class = if active == "dashboard" {
        NAV_ACTIVE
    } else {
        NAV_BASE
    };
    let artists_class = if active == "artists" {
        NAV_ACTIVE
    } else {
        NAV_BASE
    };
    let wanted_class = if active == "wanted" {
        NAV_ACTIVE
    } else {
        NAV_BASE
    };

    // Theme state — initialised from DOM on the client (after hydration),
    // defaults to `true` (dark) during SSR to match the bootstrap script.
    let (is_dark, set_is_dark) = signal(read_is_dark());

    // Sync on mount: re-read from DOM after hydration in case the bootstrap
    // script set a different state than the SSR default.
    #[cfg(feature = "hydrate")]
    {
        Effect::new(move |_| {
            set_is_dark.set(read_is_dark());
        });
    }

    let theme_label = move || if is_dark.get() { "Dark" } else { "Light" };

    let on_toggle = move |_| {
        #[cfg(feature = "hydrate")]
        {
            let new_dark = toggle_dark_mode();
            set_is_dark.set(new_dark);
        }
    };

    view! {
        <aside class="fixed inset-y-0 left-0 w-[220px] bg-[rgba(10,10,15,.92)] backdrop-blur-[20px] border-r border-white/[.06] flex flex-col z-50 overflow-y-auto max-md:hidden">
            <div class="px-4 pt-5 pb-3 flex items-center gap-2.5 border-b border-white/[.06]">
                <div class="size-8 rounded-lg bg-linear-to-br from-blue-500 to-blue-400 flex items-center justify-center shrink-0 shadow-[0_0_16px_rgba(59,130,246,.3)] [&_svg]:size-[18px] [&_svg]:fill-white" inner_html=ICON_MUSIC></div>
                <span class="text-lg font-bold text-zinc-100 tracking-wide">"yoink"</span>
            </div>
            <nav class="flex-1 py-2">
                <a href="/" class=dashboard_class>
                    <span inner_html=ICON_HOUSE></span>
                    "Dashboard"
                </a>
                <a href="/artists" class=artists_class>
                    <span inner_html=ICON_MIC></span>
                    "Artists"
                </a>
                <a href="/wanted" class=wanted_class>
                    <span inner_html=ICON_HEART></span>
                    "Wanted"
                </a>
            </nav>
            <div class="px-4 py-3 border-t border-white/[.06]">
                <button type="button"
                    class="flex items-center gap-2.5 w-full bg-transparent border-none text-zinc-400/90 font-inherit text-[13px] cursor-pointer py-2 px-1 rounded-md transition-[background,color] duration-150 hover:bg-white/[.04] hover:text-zinc-200 [&_svg]:size-4 [&_svg]:shrink-0"
                    on:click=on_toggle
                >
                    <span inner_html=ICON_SUN_MOON></span>
                    <span>{theme_label}</span>
                </button>
            </div>
        </aside>
    }
}
