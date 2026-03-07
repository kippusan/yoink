use leptos::prelude::*;

// ── Variant ────────────────────────────────────────────────

/// Visual style of a button.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum ButtonVariant {
    /// Glass / neutral outline (default).
    #[default]
    Outline,
    /// Solid blue primary action.
    Primary,
    /// Translucent red — for less-prominent destructive actions.
    Danger,
    /// Solid red — for prominent destructive confirms.
    DangerSolid,
}

impl ButtonVariant {
    /// Tailwind classes for this variant (no sizing).
    pub const fn classes(self) -> &'static str {
        match self {
            Self::Outline => {
                "\
                bg-white/60 dark:bg-zinc-800/60 backdrop-blur-[8px] \
                border border-black/[.08] dark:border-white/10 \
                text-zinc-600 dark:text-zinc-300 \
                hover:bg-white/85 hover:border-blue-500/20 \
                dark:hover:bg-zinc-800/85 dark:hover:border-blue-500/30"
            }
            Self::Primary => {
                "\
                bg-blue-500 dark:bg-blue-500 backdrop-blur-[8px] \
                border border-blue-500 text-white \
                shadow-[0_2px_12px_rgba(59,130,246,.25)] \
                hover:bg-blue-400 hover:border-blue-400 \
                hover:shadow-[0_4px_20px_rgba(59,130,246,.35)]"
            }
            Self::Danger => {
                "\
                bg-red-500/[.08] dark:bg-red-500/10 backdrop-blur-[8px] \
                border border-red-500/30 dark:border-red-400/30 \
                text-red-600 dark:text-red-400 \
                hover:bg-red-500/15 hover:border-red-600 \
                dark:hover:bg-red-500/20 dark:hover:border-red-400"
            }
            Self::DangerSolid => {
                "\
                bg-red-500 backdrop-blur-[8px] \
                border border-red-500 text-white \
                shadow-[0_2px_12px_rgba(239,68,68,.25)] \
                hover:bg-red-400 hover:border-red-400 \
                hover:shadow-[0_4px_20px_rgba(239,68,68,.35)]"
            }
        }
    }
}

// ── Size ───────────────────────────────────────────────────

/// Padding / font-size preset.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum ButtonSize {
    /// `px-2 py-0.5 text-[11px] gap-1`
    Xs,
    /// `px-2.5 py-0.5 text-xs` — the most common size (~22 uses)
    #[default]
    Sm,
    /// `px-3 py-1 text-xs`
    Md,
    /// `px-3.5 py-1.5 text-[13px]` — original BTN default
    Lg,
}

impl ButtonSize {
    /// Tailwind classes for this size.
    pub const fn classes(self) -> &'static str {
        match self {
            Self::Xs => "px-2 py-1 text-[11px] gap-1",
            Self::Sm => "px-2.5 py-1.5 text-xs",
            Self::Md => "px-3 py-2 text-xs",
            Self::Lg => "px-3.5 py-2.5 text-[13px]",
        }
    }
}

// ── Shared base classes ────────────────────────────────────

/// Classes shared by every button regardless of variant / size.
const BTN_BASE: &str = "\
    inline-flex items-center justify-center gap-1.5 rounded-lg \
    font-medium cursor-pointer no-underline \
    transition-all duration-150 whitespace-nowrap \
    disabled:opacity-50 disabled:pointer-events-none disabled:cursor-not-allowed";

// ── Component ──────────────────────────────────────────────

/// A polymorphic button component.
///
/// Renders `<a>` when `href` is provided, otherwise `<button type="button">`.
///
/// Spread HTML attributes (`on:click`, `disabled`, `title`, …) are forwarded
/// to the single top-level element automatically by Leptos.
///
/// ```ignore
/// <Button variant=ButtonVariant::Primary size=ButtonSize::Sm
///     on:click=move |_| do_stuff()
///     {..} disabled=is_disabled
/// >
///     "Click me"
/// </Button>
/// ```
#[component]
pub fn Button(
    /// Visual style.
    #[prop(optional)]
    variant: ButtonVariant,
    /// Padding / font-size preset.
    #[prop(optional)]
    size: ButtonSize,
    /// When true, adds loading opacity + disables pointer events.
    #[prop(optional, into)]
    loading: MaybeProp<bool>,
    /// Extra Tailwind classes appended after the computed classes.
    #[prop(optional, into)]
    class: String,
    /// If set, renders an `<a>` instead of `<button>`.
    #[prop(optional, into)]
    href: Option<String>,
    children: Children,
) -> impl IntoView {
    let computed = move || {
        let is_loading = loading.get().unwrap_or(false);
        let loading_cls = if is_loading {
            "opacity-60 pointer-events-none cursor-not-allowed"
        } else {
            ""
        };
        [BTN_BASE, variant.classes(), size.classes(), loading_cls]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<&str>>()
            .join(" ")
    };

    let inner = children();

    if let Some(href) = href {
        let extra = class;
        view! { <a href=href class=move || format!("{} {extra}", computed())>{inner}</a> }
            .into_any()
    } else {
        let extra = class;
        view! { <button type="button" class=move || format!("{} {extra}", computed())>{inner}</button> }
            .into_any()
    }
}
