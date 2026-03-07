// ── Shared Tailwind class constants ─────────────────────────
//
// Extracted from individual page files to avoid duplication.
// Page-specific constants remain in their respective files.

pub const GLASS: &str = "bg-white/70 dark:bg-zinc-800/60 backdrop-blur-[12px] border border-black/[.06] dark:border-white/[.08] rounded-xl mb-6 overflow-hidden";
pub const GLASS_HEADER: &str = "px-5 max-md:px-3.5 py-3.5 border-b border-black/[.06] dark:border-white/[.06] flex items-center justify-between gap-3";
pub const GLASS_TITLE: &str = "text-[15px] font-semibold text-zinc-900 dark:text-zinc-100 m-0";
pub const GLASS_BODY: &str = "px-5 max-md:px-3.5 py-4";
pub const MUTED: &str = "text-zinc-500 dark:text-zinc-400";
pub const EMPTY: &str = "text-center py-10 px-4 text-zinc-400 dark:text-zinc-600 text-sm";
pub const SELECT: &str = "py-1 px-2 border border-black/[.06] dark:border-white/[.08] rounded-lg text-xs bg-white/40 dark:bg-zinc-800/40 text-zinc-900 dark:text-zinc-100 outline-none cursor-pointer transition-[border-color,box-shadow] duration-150 focus:border-blue-500 focus:shadow-[0_0_0_2px_rgba(59,130,246,.12)]";
pub const SEARCH_INPUT: &str = "py-2 px-3.5 border border-black/[.08] dark:border-white/10 rounded-lg font-inherit text-sm bg-white/60 dark:bg-zinc-800/60 backdrop-blur-[8px] text-zinc-900 dark:text-zinc-100 outline-none w-full transition-[border-color,box-shadow] duration-150 focus:border-blue-500 focus:shadow-[0_0_0_3px_rgba(59,130,246,.15)] dark:focus:shadow-[0_0_0_3px_rgba(59,130,246,.2)] placeholder:text-zinc-400 dark:placeholder:text-zinc-600";

// ── Sticky header bar ───────────────────────────────────────
pub const HEADER_BAR: &str = "bg-white/70 dark:bg-zinc-800/60 backdrop-blur-[16px] border-b border-black/[.06] dark:border-white/[.06] px-6 max-md:px-4 py-3.5 flex items-center justify-between sticky top-0 z-40";

// ── Breadcrumb navigation ───────────────────────────────────
pub const BREADCRUMB_NAV: &str = "flex items-center gap-1.5 text-sm min-w-0";
pub const BREADCRUMB_LINK: &str = "text-zinc-500 dark:text-zinc-400 hover:text-blue-500 dark:hover:text-blue-400 no-underline truncate max-w-[200px] transition-colors duration-150";
pub const BREADCRUMB_SEP: &str = "text-zinc-300 dark:text-zinc-600 shrink-0 [&_svg]:size-3.5";
pub const BREADCRUMB_CURRENT: &str = "text-zinc-900 dark:text-zinc-100 font-semibold truncate";

/// Variadic class-name joiner. Filters empty segments.
///
/// ```ignore
/// cls!(GLASS, "px-2 py-1", if active { "ring" } else { "" })
/// ```
#[macro_export]
macro_rules! cls {
    ($($segment:expr),* $(,)?) => {{
        [$($segment),*]
            .into_iter()
            .filter(|s: &&str| !s.is_empty())
            .collect::<Vec<&str>>()
            .join(" ")
    }};
}
