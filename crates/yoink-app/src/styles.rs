// ── Shared Tailwind class constants ─────────────────────────
//
// Extracted from individual page files to avoid duplication.
// Page-specific constants remain in their respective files.

pub const GLASS: &str = "bg-white/70 dark:bg-zinc-800/60 backdrop-blur-[12px] border border-black/[.06] dark:border-white/[.08] rounded-xl mb-6 overflow-hidden";
pub const GLASS_HEADER: &str = "px-5 py-3.5 border-b border-black/[.06] dark:border-white/[.06] flex items-center justify-between gap-3";
pub const GLASS_TITLE: &str = "text-[15px] font-semibold text-zinc-900 dark:text-zinc-100 m-0";
pub const GLASS_BODY: &str = "px-5 py-4";
pub const MUTED: &str = "text-zinc-500 dark:text-zinc-400";
pub const EMPTY: &str = "text-center py-10 px-4 text-zinc-400 dark:text-zinc-600 text-sm";
pub const BTN: &str = "inline-flex items-center justify-center gap-1.5 px-3.5 py-1.5 bg-white/60 dark:bg-zinc-800/60 backdrop-blur-[8px] border border-black/[.08] dark:border-white/10 rounded-lg font-inherit text-[13px] font-medium cursor-pointer text-zinc-600 dark:text-zinc-300 no-underline transition-all duration-150 whitespace-nowrap hover:bg-white/85 hover:border-blue-500/20 dark:hover:bg-zinc-800/85 dark:hover:border-blue-500/30";
pub const BTN_PRIMARY: &str = "inline-flex items-center justify-center gap-1.5 px-3.5 py-1.5 bg-blue-500 dark:bg-blue-500 backdrop-blur-[8px] border border-blue-500 rounded-lg font-inherit text-[13px] font-medium cursor-pointer text-white no-underline transition-all duration-150 whitespace-nowrap shadow-[0_2px_12px_rgba(59,130,246,.25)] hover:bg-blue-400 hover:border-blue-400 hover:shadow-[0_4px_20px_rgba(59,130,246,.35)]";
pub const BTN_DANGER: &str = "inline-flex items-center justify-center gap-1.5 px-3.5 py-1.5 bg-red-500/[.08] dark:bg-red-500/10 backdrop-blur-[8px] border border-red-500/30 dark:border-red-400/30 rounded-lg font-inherit text-[13px] font-medium cursor-pointer text-red-600 dark:text-red-400 no-underline transition-all duration-150 whitespace-nowrap hover:bg-red-500/15 hover:border-red-600 dark:hover:bg-red-500/20 dark:hover:border-red-400";

pub const BTN_LOADING: &str = "opacity-60 pointer-events-none cursor-not-allowed";
pub const SELECT: &str = "py-1 px-2 border border-black/[.06] dark:border-white/[.08] rounded-lg text-xs bg-white/40 dark:bg-zinc-800/40 text-zinc-900 dark:text-zinc-100 outline-none cursor-pointer transition-[border-color,box-shadow] duration-150 focus:border-blue-500 focus:shadow-[0_0_0_2px_rgba(59,130,246,.12)]";

pub fn cls(a: &str, b: &str) -> String {
    format!("{a} {b}")
}

/// Combine a button class with `BTN_LOADING` when loading is true.
pub fn btn_cls(base: &str, extra: &str, loading: bool) -> String {
    if loading {
        format!("{base} {extra} {BTN_LOADING}")
    } else {
        format!("{base} {extra}")
    }
}
