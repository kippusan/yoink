use leptos::portal::Portal;
use leptos::prelude::*;
use lucide_leptos::{Check, ChevronDown};
use yoink_shared::Quality;

// ── Helpers ────────────────────────────────────────────────

/// Human-friendly label for a `Quality` value.
pub fn quality_label(quality: Quality) -> &'static str {
    match quality {
        Quality::HiRes => "Hi-Res Lossless",
        Quality::Lossless => "Lossless",
        Quality::High => "High",
        Quality::Low => "Low",
    }
}

/// All quality variants in display order (highest to lowest).
const QUALITY_OPTIONS: [Quality; 4] = [
    Quality::HiRes,
    Quality::Lossless,
    Quality::High,
    Quality::Low,
];

// ── Style constants ────────────────────────────────────────

const TRIGGER: &str = "\
    flex w-fit items-center justify-between gap-2 rounded-lg \
    border border-black/[.06] dark:border-white/[.08] \
    bg-white/40 dark:bg-zinc-800/40 \
    px-2.5 py-1.5 text-xs whitespace-nowrap \
    text-zinc-900 dark:text-zinc-100 \
    cursor-pointer select-none outline-none \
    transition-[color,border-color,box-shadow] duration-150 \
    hover:border-blue-500/30 dark:hover:border-blue-500/40 \
    focus-visible:border-blue-500 focus-visible:ring-[3px] focus-visible:ring-blue-500/15 \
    disabled:cursor-not-allowed disabled:opacity-50";

const CONTENT: &str = "\
    fixed z-[9999] w-max \
    rounded-lg \
    border border-black/[.06] dark:border-white/[.08] \
    bg-white dark:bg-zinc-800 \
    text-zinc-900 dark:text-zinc-100 \
    shadow-lg dark:shadow-[0_8px_32px_rgba(0,0,0,.4)] \
    p-1";

const ITEM: &str = "\
    relative flex w-full cursor-pointer items-center \
    rounded-md py-1.5 pr-8 pl-2 text-xs whitespace-nowrap \
    outline-none select-none border-none bg-transparent \
    text-zinc-900 dark:text-zinc-100 \
    transition-colors duration-100 \
    hover:bg-blue-500/[.08] dark:hover:bg-blue-500/[.12] \
    hover:text-blue-600 dark:hover:text-blue-400";

const ITEM_ACTIVE: &str = "\
    relative flex w-full cursor-pointer items-center \
    rounded-md py-1.5 pr-8 pl-2 text-xs whitespace-nowrap \
    outline-none select-none border-none bg-transparent \
    transition-colors duration-100 \
    bg-blue-500/[.06] dark:bg-blue-500/[.10] \
    text-zinc-900 dark:text-zinc-100 \
    hover:bg-blue-500/[.08] dark:hover:bg-blue-500/[.12] \
    hover:text-blue-600 dark:hover:text-blue-400";

const INDICATOR: &str =
    "absolute right-2 flex size-3.5 items-center justify-center text-blue-500 dark:text-blue-400";

// ── Component ──────────────────────────────────────────────

/// A shadcn/ui-inspired select dropdown for quality overrides.
///
/// The dropdown is rendered via `<Portal>` into `<body>` so it
/// escapes any ancestor `overflow-hidden` / stacking contexts.
#[component]
pub fn QualitySelect(
    /// Current quality override — `None` means "use default".
    selected: Option<Quality>,
    /// The quality that applies when `selected` is `None`.
    default_quality: Quality,
    /// Label prefix for the default option, e.g. `"Use default"` or `"Album default"`.
    #[prop(default = "Use default")]
    default_label_prefix: &'static str,
    /// Called when the user picks a different option.
    on_change: Callback<Option<Quality>>,
) -> impl IntoView {
    let open = RwSignal::new(false);
    let pos_style = RwSignal::new(String::new());
    let trigger_ref = NodeRef::<leptos::html::Button>::new();

    // Display text for the trigger
    let display_text = match selected {
        Some(q) => quality_label(q).to_string(),
        None => format!(
            "{} ({})",
            default_label_prefix,
            quality_label(default_quality)
        ),
    };

    let do_open = move || {
        // Measure trigger rect and position the dropdown below it.
        #[cfg(feature = "hydrate")]
        if let Some(el) = trigger_ref.get() {
            let element: &web_sys::Element = el.as_ref();
            let rect = element.get_bounding_client_rect();
            pos_style.set(format!(
                "top:{:.0}px;left:{:.0}px;",
                rect.bottom() + 4.0,
                rect.left(),
            ));
        }
        open.set(true);
    };

    view! {
        // ── Trigger ────────────────────────────────
        <button
            type="button"
            class=TRIGGER
            node_ref=trigger_ref
            on:click=move |_| {
                if open.get_untracked() { open.set(false) } else { do_open() }
            }
            on:keydown=move |ev| {
                let key = ev.key();
                match key.as_str() {
                    "Escape" => open.set(false),
                    "Enter" | " " | "ArrowDown" => {
                        ev.prevent_default();
                        if !open.get_untracked() { do_open(); }
                    }
                    _ => {}
                }
            }
        >
            <span class="line-clamp-1">{display_text}</span>
            <span
                class="shrink-0 opacity-50 transition-transform duration-150 flex items-center"
                style=move || if open.get() { "transform:rotate(180deg)" } else { "" }
            >
                <ChevronDown size=14 />
            </span>
        </button>

        // ── Portal: backdrop + dropdown rendered at <body> level ──
        <Portal>
            <Show when=move || open.get()>
                // Backdrop
                <div
                    class="fixed inset-0 z-[9998]"
                    on:click=move |_| open.set(false)
                ></div>
                // Dropdown
                <div
                    class=CONTENT
                    style=move || format!(
                        "{}animation:quality-select-in 120ms ease-out",
                        pos_style.get(),
                    )
                >
                    // Default option
                    <button
                        type="button"
                        class=move || if selected.is_none() { ITEM_ACTIVE } else { ITEM }
                        on:click=move |_| {
                            on_change.run(None);
                            open.set(false);
                        }
                        on:keydown=move |ev| {
                            if ev.key() == "Escape" { open.set(false); }
                        }
                    >
                        {format!("{} ({})", default_label_prefix, quality_label(default_quality))}
                        {if selected.is_none() {
                            view! {
                                <span class=INDICATOR>
                                    <Check size=14 />
                                </span>
                            }.into_any()
                        } else {
                            view! { <span></span> }.into_any()
                        }}
                    </button>

                    // Separator
                    <div class="my-1 -mx-1 h-px bg-black/[.06] dark:bg-white/[.06]"></div>

                    // Quality variants
                    {QUALITY_OPTIONS.into_iter().map(|q| {
                        let is_selected = selected == Some(q);
                        let label = quality_label(q);
                        view! {
                            <button
                                type="button"
                                class=move || if is_selected { ITEM_ACTIVE } else { ITEM }
                                on:click=move |_| {
                                    on_change.run(Some(q));
                                    open.set(false);
                                }
                                on:keydown=move |ev| {
                                    if ev.key() == "Escape" { open.set(false); }
                                }
                            >
                                {label}
                                {if is_selected {
                                    view! {
                                        <span class=INDICATOR>
                                            <Check size=14 />
                                        </span>
                                    }.into_any()
                                } else {
                                    view! { <span></span> }.into_any()
                                }}
                            </button>
                        }
                    }).collect_view()}
                </div>
            </Show>
        </Portal>
    }
}
