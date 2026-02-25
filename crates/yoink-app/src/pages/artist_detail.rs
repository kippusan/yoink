use std::collections::HashMap;

use leptos::prelude::*;
use lucide_leptos::ArrowLeft;

use yoink_shared::{
    DownloadJob, MonitoredAlbum, MonitoredArtist, ServerAction, album_cover_url,
    album_profile_url, album_type_label, album_type_rank, build_latest_jobs,
    monitored_artist_image_url, monitored_artist_profile_url, status_class, status_label_text,
};

use leptoaster::{ToastBuilder, ToastLevel, ToastPosition, expect_toaster};

use crate::actions::dispatch_action;
use crate::components::{ConfirmDialog, ErrorPanel, Sidebar};
use crate::components::toast::{dispatch_with_toast, dispatch_with_toast_loading};
use crate::hooks::{set_page_title, use_sse_version};
use crate::styles::{
    BTN, BTN_DANGER, BTN_PRIMARY, EMPTY, GLASS, GLASS_BODY, GLASS_HEADER, GLASS_TITLE, MUTED,
    SELECT, btn_cls, cls, tidal_icon_svg,
};

// ── DTO ─────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ArtistDetailData {
    pub artist: Option<MonitoredArtist>,
    pub albums: Vec<MonitoredAlbum>,
    pub jobs: Vec<DownloadJob>,
}

// ── Server function ─────────────────────────────────────────

#[server(GetArtistDetail, "/leptos")]
pub async fn get_artist_detail(artist_id: i64) -> Result<ArtistDetailData, ServerFnError> {
    let ctx = use_context::<yoink_shared::ServerContext>()
        .ok_or_else(|| ServerFnError::new("ServerContext not available"))?;

    let artists = ctx.monitored_artists.read().await;
    let artist = artists.iter().find(|a| a.id == artist_id).cloned();
    drop(artists);

    let albums: Vec<MonitoredAlbum> = ctx
        .monitored_albums
        .read()
        .await
        .iter()
        .filter(|a| a.artist_id == artist_id)
        .cloned()
        .collect();

    let jobs = ctx.download_jobs.read().await.clone();

    Ok(ArtistDetailData {
        artist,
        albums,
        jobs,
    })
}

// ── Page component ──────────────────────────────────────────

#[component]
pub fn ArtistDetailPage() -> impl IntoView {
    set_page_title("Artist");
    let params = leptos_router::hooks::use_params_map();
    let artist_id = move || {
        params
            .read()
            .get("id")
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0)
    };

    let version = use_sse_version();
    let data = Resource::new(
        move || (artist_id(), version.get()),
        |(id, _)| get_artist_detail(id),
    );

    view! {
        <div class="flex min-h-screen">
            <Sidebar active="artists" />
            <div class="ml-[220px] max-md:ml-0 flex-1 min-h-screen">
                <Transition fallback=move || view! {
                    <div>
                        <div class="bg-white/70 dark:bg-zinc-800/60 backdrop-blur-[16px] border-b border-black/[.06] dark:border-white/[.06] px-6 max-md:pl-14 py-3.5 flex items-center justify-between sticky top-0 z-40">
                            <div class="h-5 w-36 bg-zinc-200 dark:bg-zinc-700 rounded animate-pulse"></div>
                        </div>
                        <div class="p-6 max-md:p-4">
                            // Skeleton artist header card
                            <div class="mb-5 bg-white/70 dark:bg-zinc-800/60 rounded-xl border border-black/[.06] dark:border-white/[.08] p-5">
                                <div class="flex flex-wrap items-center gap-5 animate-pulse">
                                    <div class="size-20 rounded-full bg-zinc-200 dark:bg-zinc-700 shrink-0"></div>
                                    <div class="flex-1 min-w-0">
                                        <div class="h-6 w-40 bg-zinc-200 dark:bg-zinc-700 rounded mb-3"></div>
                                        <div class="h-3.5 w-64 bg-zinc-200 dark:bg-zinc-700 rounded mb-3"></div>
                                        <div class="flex flex-wrap gap-1.5">
                                            {(0..4).map(|_| view! {
                                                <div class="h-7 w-20 bg-zinc-200 dark:bg-zinc-700 rounded-lg"></div>
                                            }).collect_view()}
                                        </div>
                                    </div>
                                </div>
                            </div>
                            // Skeleton album grid
                            <div class="bg-white/70 dark:bg-zinc-800/60 rounded-xl border border-black/[.06] dark:border-white/[.08] overflow-hidden">
                                <div class="px-5 py-3 border-b border-black/[.06] dark:border-white/[.06]">
                                    <div class="h-4 w-24 bg-zinc-200 dark:bg-zinc-700 rounded animate-pulse"></div>
                                </div>
                                <div class="p-4">
                                    <div class="d7-album-grid">
                                        {(0..6).map(|_| view! {
                                            <div class="rounded-xl overflow-hidden border border-black/[.04] dark:border-white/[.04] animate-pulse">
                                                <div class="w-full" style="padding-top:100%;background:var(--tw-color-zinc-200,.oklch(.923 0 0))">
                                                </div>
                                                <div class="p-3">
                                                    <div class="h-3.5 w-24 bg-zinc-200 dark:bg-zinc-700 rounded mb-2"></div>
                                                    <div class="h-3 w-16 bg-zinc-200 dark:bg-zinc-700 rounded"></div>
                                                </div>
                                            </div>
                                        }).collect_view()}
                                    </div>
                                </div>
                            </div>
                        </div>
                    </div>
                }>
                    {move || {
                        data.get().map(|result| match result {
                            Err(e) => view! {
                                <div class="p-6">
                                    <ErrorPanel
                                        message="Failed to load artist details."
                                        details=e.to_string()
                                    />
                                </div>
                            }.into_any(),
                            Ok(data) => match data.artist {
                                None => view! {
                                    <div class="p-6">
                                        <div class="text-zinc-500">"Artist not found."</div>
                                        <a href="/artists" class={cls(BTN, "mt-4 inline-flex items-center gap-1.5")}>
                                            <ArrowLeft size=14 />
                                            "All Artists"
                                        </a>
                                    </div>
                                }.into_any(),
                                Some(artist) => {
                                    view! { <ArtistDetailContent artist=artist albums=data.albums jobs=data.jobs /> }.into_any()
                                }
                            }
                        })
                    }}
                </Transition>
            </div>
        </div>
    }
}

#[component]
fn ArtistDetailContent(
    artist: MonitoredArtist,
    albums: Vec<MonitoredAlbum>,
    jobs: Vec<DownloadJob>,
) -> impl IntoView {
    set_page_title(&artist.name);
    let artist_img = monitored_artist_image_url(&artist, 320);
    let artist_profile = monitored_artist_profile_url(&artist);
    let fallback_initial = artist
        .name
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_string());

    let album_count = albums.len();
    let monitored_count = albums.iter().filter(|a| a.monitored).count();
    let acquired_count = albums.iter().filter(|a| a.acquired).count();
    let wanted_count = albums.iter().filter(|a| a.wanted).count();

    let albums_stored = StoredValue::new(albums);
    let (album_sort, set_album_sort) = signal("type".to_string());

    let latest_jobs = build_latest_jobs(jobs);
    let artist_id_val = artist.id;

    // Confirmation dialog signals
    let show_unmonitor_all = RwSignal::new(false);
    let show_remove_artist = RwSignal::new(false);

    // Loading state signals for async buttons
    let sync_loading = RwSignal::new(false);
    let monitor_all_loading = RwSignal::new(false);
    let removing_artist = RwSignal::new(false);

    view! {
        // Header
        <div class="bg-white/70 dark:bg-zinc-800/60 backdrop-blur-[16px] border-b border-black/[.06] dark:border-white/[.06] px-6 max-md:pl-14 py-3.5 flex items-center justify-between sticky top-0 z-40">
            <h1 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100 m-0">{artist.name.clone()}</h1>
            <a href="/artists" class={cls(BTN, "px-2.5 py-0.5 text-xs no-underline inline-flex items-center gap-1.5")}>
                <ArrowLeft size=14 />
                "All Artists"
            </a>
        </div>

        <div class="p-6 max-md:p-4">
            // Artist header card
            <div class={cls(GLASS, "mb-5")}>
                <div class={cls(GLASS_BODY, "flex flex-wrap items-center gap-5")}>
                    {match artist_img {
                        Some(url) => view! {
                            <img class="size-20 rounded-full object-cover border-2 border-blue-500/20 dark:border-blue-500/30 shrink-0 bg-zinc-200 dark:bg-zinc-800" src=url alt="" />
                        }.into_any(),
                        None => view! {
                            <div class="size-20 rounded-full inline-flex items-center justify-center bg-zinc-200 dark:bg-zinc-800 text-zinc-500 dark:text-zinc-400 font-bold text-[32px] border-2 border-blue-500/20 dark:border-blue-500/30 shrink-0">{fallback_initial}</div>
                        }.into_any(),
                    }}
                    <div class="flex-1 min-w-0">
                        <div class="text-[22px] font-bold mb-1">{artist.name.clone()}</div>
                        <div class={cls(MUTED, "text-[13px] mb-2 flex flex-wrap items-center gap-2")}>
                            <span>{format!("{album_count} albums \u{00b7} {monitored_count} monitored \u{00b7} {acquired_count} acquired \u{00b7} {wanted_count} wanted")}</span>
                            <span class="pill d7-pill-muted">{artist.quality_profile.clone()}</span>
                        </div>
                        <div class="flex flex-wrap gap-1.5">
                            <a href=artist_profile target="_blank" rel="noreferrer" class={cls(BTN, "px-2.5 py-0.5 text-xs")}>
                                <span class="inline-block size-3.5 shrink-0" inner_html=tidal_icon_svg()></span>
                                "Tidal"
                            </a>
                            <button type="button"
                                class=move || btn_cls(BTN, "px-2.5 py-0.5 text-xs", sync_loading.get())
                                disabled=move || sync_loading.get()
                                on:click=move |_| {
                                    dispatch_with_toast_loading(ServerAction::SyncArtistAlbums { artist_id: artist_id_val }, "Album sync started", Some(sync_loading));
                                }>
                                {move || if sync_loading.get() { "Syncing\u{2026}" } else { "Sync Albums" }}
                            </button>
                            <button type="button"
                                class=move || btn_cls(BTN_PRIMARY, "px-2.5 py-0.5 text-xs", monitor_all_loading.get())
                                disabled=move || monitor_all_loading.get()
                                on:click=move |_| {
                                    dispatch_with_toast_loading(ServerAction::BulkMonitor { artist_id: artist_id_val, monitored: true }, "All albums monitored", Some(monitor_all_loading));
                                }>
                                {move || if monitor_all_loading.get() { "Monitoring\u{2026}" } else { "Monitor All" }}
                            </button>
                            <button type="button" class={cls(BTN, "px-2.5 py-0.5 text-xs")}
                                on:click=move |_| {
                                    show_unmonitor_all.set(true);
                                }>"Unmonitor All"</button>
                            <button type="button" class={cls(BTN_DANGER, "px-2.5 py-0.5 text-xs")}
                                on:click=move |_| {
                                    show_remove_artist.set(true);
                                }>"Remove Artist"</button>
                        </div>
                    </div>
                </div>
            </div>

            // Confirmation dialogs
            <ConfirmDialog
                open=show_unmonitor_all
                title="Unmonitor All Albums"
                message="This will unmonitor all albums for this artist."
                confirm_label="Unmonitor All"
                on_confirm=move |_: bool| {
                    dispatch_with_toast(ServerAction::BulkMonitor { artist_id: artist_id_val, monitored: false }, "All albums unmonitored");
                }
            />
            <ConfirmDialog
                open=show_remove_artist
                title="Remove Artist"
                message="This will remove the artist and all associated data. This cannot be undone."
                confirm_label="Remove"
                danger=true
                checkbox_label="Also remove downloaded files from disk"
                on_confirm=move |remove_files: bool| {
                    removing_artist.set(true);
                    let navigate = leptos_router::hooks::use_navigate();
                    let toaster = expect_toaster();
                    leptos::task::spawn_local(async move {
                        match dispatch_action(ServerAction::RemoveArtist { artist_id: artist_id_val, remove_files }).await {
                            Ok(()) => {
                                toaster.toast(
                                    ToastBuilder::new("Artist removed")
                                        .with_level(ToastLevel::Success)
                                        .with_position(ToastPosition::BottomRight)
                                        .with_expiry(Some(4_000)),
                                );
                                navigate("/artists", Default::default());
                            }
                            Err(e) => {
                                toaster.toast(
                                    ToastBuilder::new(&format!("Error: {e}"))
                                        .with_level(ToastLevel::Error)
                                        .with_position(ToastPosition::BottomRight)
                                        .with_expiry(Some(8_000)),
                                );
                            }
                        }
                        removing_artist.set(false);
                    });
                }
            />

            // Albums grid with sort (#9)
            <div class=GLASS>
                <div class=GLASS_HEADER>
                    <h2 class=GLASS_TITLE>"Discography"</h2>
                    <div class="flex items-center gap-2">
                        {if album_count > 0 {
                            view! {
                                <select
                                    class=SELECT
                                    aria-label="Sort albums"
                                    on:change=move |ev| {
                                        set_album_sort.set(event_target_value(&ev));
                                    }
                                >
                                    <option value="type" selected=true>"By Type"</option>
                                    <option value="az">"A \u{2013} Z"</option>
                                    <option value="newest">"Newest First"</option>
                                    <option value="oldest">"Oldest First"</option>
                                </select>
                            }.into_any()
                        } else {
                            view! { <span></span> }.into_any()
                        }}
                        <span class={cls(MUTED, "text-xs")}>{format!("{album_count} albums")}</span>
                    </div>
                </div>
                {move || {
                    albums_stored.with_value(|all| {
                        if all.is_empty() {
                            return view! { <div class=EMPTY>"No albums synced. Hit Sync Albums to fetch from Tidal."</div> }.into_any();
                        }

                        let sort_key = album_sort.get();
                        let mut sorted = all.clone();
                        match sort_key.as_str() {
                            "az" => sorted.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase())),
                            "newest" => sorted.sort_by(|a, b| b.release_date.cmp(&a.release_date).then_with(|| a.title.cmp(&b.title))),
                            "oldest" => sorted.sort_by(|a, b| a.release_date.cmp(&b.release_date).then_with(|| a.title.cmp(&b.title))),
                            _ /* "type" */ => sorted.sort_by(|a, b| {
                                album_type_rank(a.album_type.as_deref(), &a.title)
                                    .cmp(&album_type_rank(b.album_type.as_deref(), &b.title))
                                    .then_with(|| b.release_date.cmp(&a.release_date))
                                    .then_with(|| a.title.cmp(&b.title))
                            }),
                        }

                        let jobs = latest_jobs.clone();

                        view! {
                            <div class={cls(GLASS_BODY, "p-4")}>
                                <div class="d7-album-grid">
                                    {sorted.into_iter().map(|album| {
                                        view! { <AlbumSleeve album=album latest_jobs=jobs.clone() artist_id=artist_id_val /> }
                                    }).collect_view()}
                                </div>
                            </div>
                        }.into_any()
                    })
                }}
            </div>
        </div>
    }
}

/// Album sleeve card in the discography grid.
///
/// The title links to the album detail page.
#[component]
fn AlbumSleeve(
    album: MonitoredAlbum,
    latest_jobs: HashMap<i64, DownloadJob>,
    artist_id: i64,
) -> impl IntoView {
    let album_id = album.id;
    let album_id_str = album.id.to_string();
    let album_title = album.title.clone();
    let release_date = album
        .release_date
        .clone()
        .unwrap_or_else(|| "\u{2014}".to_string());
    let at = album_type_label(album.album_type.as_deref(), &album.title);
    let is_explicit = album.explicit;
    let is_monitored = album.monitored;
    let is_wanted = album.wanted;
    let is_acquired = album.acquired;

    let show_remove_files = RwSignal::new(false);

    let cover_url = album_cover_url(&album, 640);
    let tidal_btn_url = album_profile_url(&album);
    let detail_url = format!("/artists/{artist_id}/albums/{album_id}");

    let latest_job = latest_jobs.get(&album.id).cloned();
    let job_status = latest_job.as_ref().map(|j| j.status.clone());
    let job_progress = latest_job
        .as_ref()
        .map(|j| (j.completed_tracks, j.total_tracks));

    let wanted_pill_text = if is_wanted { "Wanted" } else { "Not Wanted" };

    let status_pill_class = match &job_status {
        Some(s) => status_class(s).to_string(),
        None => "pill".to_string(),
    };
    let status_pill_text = match &job_status {
        Some(s) => status_label_text(
            s,
            job_progress.map(|(c, _)| c).unwrap_or(0),
            job_progress.map(|(_, t)| t).unwrap_or(0),
        ),
        None => "\u{2014}".to_string(),
    };

    let fallback_initial = album_title
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_string());

    let monitor_title = if is_monitored {
        "Unmonitor album"
    } else {
        "Monitor album"
    };
    let monitor_label = if is_monitored { "Unmonitor" } else { "Monitor" };

    view! {
        // ── Album card (grid cell) ──────────────────────────
        <div class="d7-sleeve" data-album-row data-album-id=album_id_str.clone()>
            <a href=detail_url.clone() class="contents">
                <div class="d7-sleeve-cover-wrap">
                    {match cover_url {
                        Some(url) => view! {
                            <img class="d7-sleeve-cover" src=url alt="" loading="lazy" />
                        }.into_any(),
                        None => view! {
                            <div class="d7-sleeve-fallback">{fallback_initial}</div>
                        }.into_any(),
                    }}
                    {if is_wanted && !is_acquired {
                        view! { <span class="d7-badge d7-badge-wanted">"Wanted"</span> }.into_any()
                    } else if is_acquired {
                        view! { <span class="d7-badge d7-badge-acquired">"Acquired"</span> }.into_any()
                    } else {
                        view! { <span></span> }.into_any()
                    }}
                    {if is_explicit {
                        view! { <span class="d7-badge d7-badge-explicit">"E"</span> }.into_any()
                    } else {
                        view! { <span></span> }.into_any()
                    }}
                </div>
            </a>

            <div class="d7-sleeve-info">
                <div class="d7-sleeve-title">
                    <a href=detail_url>{album_title.clone()}</a>
                </div>
                <div class="d7-sleeve-sub">{format!("{release_date} \u{00b7} {at}")}</div>

                <div class="d7-sleeve-status">
                    <span class=status_pill_class data-job-status>{status_pill_text}</span>
                    <span class={cls(MUTED, "text-[10px]")} data-wanted-pill>{wanted_pill_text}</span>
                </div>

                <div class="d7-sleeve-actions">
                    <button type="button" class={cls(BTN, "d7-sleeve-action-btn")} title=monitor_title
                        on:click=move |_| {
                            let next = !is_monitored;
                            let msg = if next { "Album monitored" } else { "Album unmonitored" };
                            dispatch_with_toast(ServerAction::ToggleAlbumMonitor { album_id, monitored: next }, msg);
                        }>{monitor_label}</button>
                    {if is_acquired {
                        view! {
                            <button type="button" class={cls(BTN_DANGER, "d7-sleeve-action-btn")} title="Delete downloaded files"
                                on:click=move |_| {
                                    show_remove_files.set(true);
                                }>
                                "Remove Files"
                            </button>
                        }.into_any()
                    } else {
                        view! { <span></span> }.into_any()
                    }}
                    {match tidal_btn_url {
                        Some(url) => view! {
                            <a href=url target="_blank" rel="noreferrer"
                                class={cls(BTN, "d7-sleeve-action-btn")}
                                title="Open on Tidal" aria-label="Open on Tidal">
                                <span class="inline-block size-3" inner_html=tidal_icon_svg()></span>
                            </a>
                        }.into_any(),
                        None => view! { <span></span> }.into_any(),
                    }}
                </div>
            </div>
        </div>

        // ── Confirmation dialog for removing album files ────
        <ConfirmDialog
            open=show_remove_files
            title="Remove Files"
            message=format!("This will delete all downloaded files for \u{201c}{album_title}\u{201d} from disk.")
            confirm_label="Remove Files"
            danger=true
            checkbox_label="Also unmonitor this album"
            on_confirm=move |unmonitor: bool| {
                let msg = if unmonitor { "Album files removed and unmonitored" } else { "Album files removed" };
                dispatch_with_toast(ServerAction::RemoveAlbumFiles { album_id, unmonitor }, msg);
            }
        />
    }
}
