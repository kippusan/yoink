use leptos::prelude::*;
use lucide_leptos::Bookmark;
use yoink_shared::{MonitoredAlbum, ServerAction, Uuid, album_cover_url};

use super::{SleeveBadge, SleeveBadgeView, fallback_initial};
use crate::components::toast::dispatch_with_toast;

// ── Monitor toggle helper ───────────────────────────────────

/// Props needed to render the monitor (bookmark) toggle on a card.
#[derive(Clone)]
pub struct MonitorToggle {
    pub album_id: Uuid,
    /// Reactive closure returning whether the album is currently monitored.
    pub is_monitored: Signal<bool>,
}

// ── AlbumCard component ─────────────────────────────────────

/// Shared album card used in both the artist-detail discography grid
/// and the library-albums page.
///
/// Always renders using the `.sleeve` CSS class (glow effect extracted
/// from cover art via JS).  Optional features like the status badge
/// overlay, explicit badge, and monitor toggle are controlled via props.
#[component]
pub fn AlbumCard(
    /// The album to render.
    album: MonitoredAlbum,
    /// Navigation target when the card is clicked.
    href: String,

    // ── Cover ───────────────────────────────────────────────
    /// Resolution (px) to request for the cover art URL.
    #[prop(default = 320)]
    cover_resolution: u16,

    // ── Overlays on cover ───────────────────────────────────
    /// Reactive badge overlay on the cover (download / acquired / wanted).
    /// Pass `None` (default) to hide the badge entirely.
    #[prop(optional, into)]
    sleeve_badge: Option<Signal<SleeveBadge>>,

    /// Show the "E" explicit badge in the top-left of the cover.
    #[prop(default = false)]
    show_explicit: bool,

    // ── Below-cover metadata ────────────────────────────────
    /// Primary subtitle shown under the title.
    /// Artist-detail passes `"2024 · Album"`, library passes the artist name.
    #[prop(optional, into)]
    subtitle: Option<String>,

    // ── Interactive controls ────────────────────────────────
    /// When provided, a bookmark icon toggle is rendered beside the title.
    #[prop(optional)]
    monitor_toggle: Option<MonitorToggle>,
) -> impl IntoView {
    let album_id_str = album.id.to_string();
    let album_title = album.title.clone();
    let cover_url = album_cover_url(&album, cover_resolution);
    let is_explicit = album.explicit && show_explicit;

    let fi = fallback_initial(&album_title);

    view! {
        <div class="sleeve" data-album-row data-album-id=album_id_str>
            <a href=href.clone() class="contents">
                <div class="relative w-full pt-[100%] bg-zinc-200 dark:bg-zinc-800 overflow-hidden">
                    // Cover image
                    {match &cover_url {
                        Some(url) => view! {
                            <img class="sleeve-cover" src=url.clone() alt="" loading="lazy" />
                        }.into_any(),
                        None => view! {
                            <div class="absolute inset-0 flex items-center justify-center text-4xl font-extrabold text-zinc-400 dark:text-zinc-600">{fi}</div>
                        }.into_any(),
                    }}
                    // Status badge overlay
                    {move || {
                        sleeve_badge.map(|sig| {
                            let badge = sig.get();
                            view! { <SleeveBadgeView badge=badge /> }
                        })
                    }}
                    // Explicit badge
                    {if is_explicit {
                        view! {
                            <span class="absolute z-3 top-2 left-2 text-[8px] font-bold uppercase tracking-wide px-[5px] py-[2px] rounded-md whitespace-nowrap backdrop-blur-[8px] bg-zinc-700/85 text-zinc-200 dark:bg-zinc-400/85 dark:text-zinc-900">
                                "E"
                            </span>
                        }
                        .into_any()
                    } else {
                        view! { <span></span> }.into_any()
                    }}
                </div>
            </a>
            <div class="px-3 py-2.5 flex items-center gap-1.5">
                <div class="flex-1 min-w-0">
                    <div class="text-[13px] font-semibold text-zinc-900 dark:text-zinc-100 whitespace-nowrap overflow-hidden text-ellipsis">
                        <a
                            href=href
                            title=album_title.clone()
                            class="text-inherit no-underline hover:text-blue-500"
                        >
                            {album_title.clone()}
                        </a>
                    </div>
                    {subtitle
                        .as_ref()
                        .map(|s| {
                            view! {
                                <div class="text-[11px] text-zinc-500 dark:text-zinc-400 whitespace-nowrap overflow-hidden text-ellipsis">
                                    {s.clone()}
                                </div>
                            }
                        })}
                </div>
                // Monitor toggle (bookmark icon)
                {monitor_toggle
                    .as_ref()
                    .map(|mt| {
                        let album_id = mt.album_id;
                        let is_monitored = mt.is_monitored;
                        view! {
                            {move || {
                                let monitored = is_monitored.get();
                                if monitored {
                                    view! {
                                        <button
                                            type="button"
                                            class="shrink-0 flex items-center justify-center w-5 h-5 text-amber-500 dark:text-amber-400 bg-transparent border-none cursor-pointer p-0 transition-colors duration-150 hover:text-amber-600 dark:hover:text-amber-300"
                                            title="Monitored \u{2014} click to unmonitor"
                                            on:click=move |_| {
                                                dispatch_with_toast(
                                                    ServerAction::ToggleAlbumMonitor {
                                                        album_id,
                                                        monitored: false,
                                                    },
                                                    "Album unmonitored",
                                                );
                                            }
                                        >
                                            <Bookmark size=18 fill="currentColor" />
                                        </button>
                                    }
                                    .into_any()
                                } else {
                                    view! {
                                        <button
                                            type="button"
                                            class="shrink-0 flex items-center justify-center w-5 h-5 text-zinc-300 dark:text-zinc-600 bg-transparent border-none cursor-pointer p-0 transition-colors duration-150 hover:text-amber-500 dark:hover:text-amber-400"
                                            title="Not monitored \u{2014} click to monitor"
                                            on:click=move |_| {
                                                dispatch_with_toast(
                                                    ServerAction::ToggleAlbumMonitor {
                                                        album_id,
                                                        monitored: true,
                                                    },
                                                    "Album monitored",
                                                );
                                            }
                                        >
                                            <Bookmark size=18 />
                                        </button>
                                    }
                                    .into_any()
                                }
                            }}
                        }
                    })}
            </div>
        </div>
    }
}
