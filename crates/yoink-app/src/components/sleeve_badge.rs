use leptos::prelude::*;

/// The single most important state to display on an album sleeve card.
///
/// Variants are listed in priority order (highest first).  The consumer is
/// responsible for computing the correct variant — the component simply
/// renders the appropriate badge or nothing at all.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SleeveBadge {
    Downloading { completed: usize, total: usize },
    Queued,
    Failed,
    Acquired,
    Wanted,
    None,
}

/// Shared Tailwind base classes for every badge variant.
const BADGE_BASE: &str = "absolute z-3 top-2 right-2 text-[9px] font-bold uppercase tracking-wide px-2 py-0.5 rounded-md whitespace-nowrap backdrop-blur-[8px]";

#[component]
pub fn SleeveBadgeView(badge: SleeveBadge) -> impl IntoView {
    match badge {
        SleeveBadge::Downloading { completed, total } => {
            let label = if total > 0 {
                format!("Downloading {completed}/{total}")
            } else {
                "Downloading".to_string()
            };
            view! {
                <span class=format!(
                    "{BADGE_BASE} bg-blue-500/85 text-white shadow-[0_2px_10px_rgba(59,130,246,.35)]",
                )>{label}</span>
            }
            .into_any()
        }
        SleeveBadge::Queued => view! {
            <span class=format!(
                "{BADGE_BASE} bg-amber-500/85 text-white shadow-[0_2px_10px_rgba(245,158,11,.35)]",
            )>"Queued"</span>
        }
        .into_any(),
        SleeveBadge::Failed => view! {
            <span class=format!(
                "{BADGE_BASE} bg-red-500/85 text-white shadow-[0_2px_10px_rgba(239,68,68,.35)]",
            )>"Failed"</span>
        }
        .into_any(),
        SleeveBadge::Acquired => {
            // Uses the per-sleeve --glow-rgb CSS variable set by JS from
            // the album cover's dominant colour.
            view! {
                <span
                    class=BADGE_BASE
                    style="background: rgba(var(--glow-rgb),.72); color: #fff; box-shadow: 0 2px 10px rgba(var(--glow-rgb),.28);"
                >
                    "Acquired"
                </span>
            }
            .into_any()
        }
        SleeveBadge::Wanted => {
            view! {
                <span
                    class=BADGE_BASE
                    style="background: rgba(var(--glow-rgb),.9); color: #fff; box-shadow: 0 2px 10px rgba(var(--glow-rgb),.35);"
                >
                    "Wanted"
                </span>
            }
            .into_any()
        }
        SleeveBadge::None => view! { <span></span> }.into_any(),
    }
}
