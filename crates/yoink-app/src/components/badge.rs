use leptos::prelude::*;
use yoink_shared::DownloadStatus;

/// Semantic color families for inline badges.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum BadgeVariant {
    #[default]
    Neutral,
    Info,
    Success,
    Warning,
    Danger,
    Accent,
    Explicit,
}

impl BadgeVariant {
    const fn soft_classes(self) -> &'static str {
        match self {
            Self::Neutral => "bg-zinc-500/[.08] text-zinc-500 dark:text-zinc-400",
            Self::Info => "bg-blue-500/[.10] text-blue-600 dark:text-blue-400",
            Self::Success => "bg-emerald-500/[.12] text-emerald-600 dark:text-emerald-400",
            Self::Warning => "bg-amber-500/[.12] text-amber-600 dark:text-amber-300",
            Self::Danger => "bg-red-500/[.10] text-red-600 dark:text-red-400",
            Self::Accent => "bg-violet-500/[.12] text-violet-600 dark:text-violet-300",
            Self::Explicit => "bg-zinc-200 text-zinc-600 dark:bg-zinc-700 dark:text-zinc-300",
        }
    }

    const fn outline_classes(self) -> &'static str {
        match self {
            Self::Neutral => {
                "bg-zinc-500/[.06] border border-zinc-500/10 text-zinc-500 dark:text-zinc-400 hover:bg-zinc-500/[.1]"
            }
            Self::Info => {
                "bg-blue-500/[.08] border border-blue-500/20 text-blue-600 dark:text-blue-400 hover:bg-blue-500/15"
            }
            Self::Success => {
                "bg-emerald-500/[.08] border border-emerald-500/20 text-emerald-700 dark:text-emerald-300 hover:bg-emerald-500/[.14]"
            }
            Self::Warning => {
                "bg-amber-500/[.08] border border-amber-500/20 text-amber-700 dark:text-amber-300 hover:bg-amber-500/[.14]"
            }
            Self::Danger => {
                "bg-red-500/[.08] border border-red-500/20 text-red-700 dark:text-red-300 hover:bg-red-500/[.14]"
            }
            Self::Accent => {
                "bg-violet-500/[.08] border border-violet-500/20 text-violet-700 dark:text-violet-300 hover:bg-violet-500/[.14]"
            }
            Self::Explicit => {
                "bg-zinc-200 border border-zinc-300/60 text-zinc-600 dark:bg-zinc-700 dark:border-zinc-600 dark:text-zinc-300"
            }
        }
    }
}

/// Visual treatment for the badge surface.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum BadgeSurface {
    #[default]
    Soft,
    Outline,
}

impl BadgeSurface {
    const fn classes(self, variant: BadgeVariant) -> &'static str {
        match self {
            Self::Soft => variant.soft_classes(),
            Self::Outline => variant.outline_classes(),
        }
    }
}

/// Shared size presets for inline badges.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum BadgeSize {
    #[default]
    Xs,
    Sm,
    Pill,
}

impl BadgeSize {
    const fn classes(self) -> &'static str {
        match self {
            Self::Xs => "px-1.5 py-px text-[10px] rounded leading-4",
            Self::Sm => "px-2 py-0.5 text-[11px] rounded-md leading-4",
            Self::Pill => "px-2 py-0.5 text-[11px] rounded-full leading-4",
        }
    }
}

const BADGE_BASE: &str = "inline-flex items-center gap-1 whitespace-nowrap shrink-0 align-middle font-medium no-underline transition-colors duration-150";

#[component]
pub fn Badge(
    #[prop(optional)] variant: BadgeVariant,
    #[prop(optional)] surface: BadgeSurface,
    #[prop(optional)] size: BadgeSize,
    #[prop(default = false)] mono: bool,
    #[prop(optional, into)] class: String,
    #[prop(optional, into)] href: Option<String>,
    #[prop(default = false)] new_tab: bool,
    #[prop(optional, into)] title: Option<String>,
    children: Children,
) -> impl IntoView {
    let computed = move || {
        crate::cls!(
            BADGE_BASE,
            size.classes(),
            surface.classes(variant),
            if mono { "font-mono" } else { "" },
            class.as_str()
        )
    };

    let inner = children();

    match href {
        Some(href) if new_tab => view! {
            <a
                href=href
                target="_blank"
                rel="noreferrer"
                class=computed
                title=title
            >
                {inner}
            </a>
        }
        .into_any(),
        Some(href) => view! {
            <a href=href class=computed title=title>
                {inner}
            </a>
        }
        .into_any(),
        None => view! {
            <span class=computed title=title>
                {inner}
            </span>
        }
        .into_any(),
    }
}

pub fn download_status_badge_variant(status: &DownloadStatus) -> BadgeVariant {
    match status {
        DownloadStatus::Queued => BadgeVariant::Warning,
        DownloadStatus::Resolving => BadgeVariant::Accent,
        DownloadStatus::Downloading => BadgeVariant::Info,
        DownloadStatus::Completed => BadgeVariant::Success,
        DownloadStatus::Failed => BadgeVariant::Danger,
    }
}
