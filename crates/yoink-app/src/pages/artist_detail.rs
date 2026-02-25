use std::collections::HashMap;

use leptos::prelude::*;
use lucide_leptos::{ArrowLeft, ListMusic};

use yoink_shared::{
    DownloadJob, MonitoredAlbum, MonitoredArtist, ServerAction, TrackInfo, album_cover_url,
    album_profile_url, album_type_label, album_type_rank, build_latest_jobs,
    monitored_artist_image_url, monitored_artist_profile_url, status_class, status_label_text,
};

use crate::actions::dispatch_action;
use crate::components::Sidebar;
use crate::hooks::use_sse_version;

// ── Tailwind class constants ────────────────────────────────

const GLASS: &str = "bg-white/70 dark:bg-zinc-800/60 backdrop-blur-[12px] border border-black/[.06] dark:border-white/[.08] rounded-xl mb-6 overflow-hidden";
const GLASS_HEADER: &str = "px-5 py-3.5 border-b border-black/[.06] dark:border-white/[.06] flex items-center justify-between gap-3";
const GLASS_TITLE: &str = "text-[15px] font-semibold text-zinc-900 dark:text-zinc-100 m-0";
const GLASS_BODY: &str = "px-5 py-4";
const MUTED: &str = "text-zinc-500 dark:text-zinc-400";
const EMPTY: &str = "text-center py-10 px-4 text-zinc-400 dark:text-zinc-600 text-sm";
const BTN: &str = "inline-flex items-center justify-center gap-1.5 px-3.5 py-1.5 bg-white/60 dark:bg-zinc-800/60 backdrop-blur-[8px] border border-black/[.08] dark:border-white/10 rounded-lg font-inherit text-[13px] font-medium cursor-pointer text-zinc-600 dark:text-zinc-300 no-underline transition-all duration-150 whitespace-nowrap hover:bg-white/85 hover:border-blue-500/20 dark:hover:bg-zinc-800/85 dark:hover:border-blue-500/30";
const BTN_PRIMARY: &str = "inline-flex items-center justify-center gap-1.5 px-3.5 py-1.5 bg-blue-500 dark:bg-blue-500 backdrop-blur-[8px] border border-blue-500 rounded-lg font-inherit text-[13px] font-medium cursor-pointer text-white no-underline transition-all duration-150 whitespace-nowrap shadow-[0_2px_12px_rgba(59,130,246,.25)] hover:bg-blue-400 hover:border-blue-400 hover:shadow-[0_4px_20px_rgba(59,130,246,.35)]";
const BTN_DANGER: &str = "inline-flex items-center justify-center gap-1.5 px-3.5 py-1.5 bg-red-500/[.08] dark:bg-red-500/10 backdrop-blur-[8px] border border-red-500/30 dark:border-red-400/30 rounded-lg font-inherit text-[13px] font-medium cursor-pointer text-red-600 dark:text-red-400 no-underline transition-all duration-150 whitespace-nowrap hover:bg-red-500/15 hover:border-red-600 dark:hover:bg-red-500/20 dark:hover:border-red-400";

fn cls(a: &str, b: &str) -> String {
    format!("{a} {b}")
}

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

#[server(GetAlbumTracks, "/leptos")]
pub async fn get_album_tracks(album_id: i64) -> Result<Vec<TrackInfo>, ServerFnError> {
    let ctx = use_context::<yoink_shared::ServerContext>()
        .ok_or_else(|| ServerFnError::new("ServerContext not available"))?;

    (ctx.fetch_tracks)(album_id)
        .await
        .map_err(ServerFnError::new)
}

// ── Page component ──────────────────────────────────────────

#[component]
pub fn ArtistDetailPage() -> impl IntoView {
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
                    <div class="bg-white/70 dark:bg-zinc-800/60 backdrop-blur-[16px] border-b border-black/[.06] dark:border-white/[.06] px-6 py-3.5 flex items-center justify-between sticky top-0 z-40">
                        <h1 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100 m-0">"Loading\u{2026}"</h1>
                    </div>
                }>
                    {move || {
                        data.get().map(|result| match result {
                            Err(e) => view! {
                                <div class="p-6">
                                    <div class="text-red-500">{format!("Error: {e}")}</div>
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

    let mut sorted_albums = albums;
    sorted_albums.sort_by(|a, b| {
        album_type_rank(a.album_type.as_deref(), &a.title)
            .cmp(&album_type_rank(b.album_type.as_deref(), &b.title))
            .then_with(|| b.release_date.cmp(&a.release_date))
            .then_with(|| a.title.cmp(&b.title))
    });

    let latest_jobs = build_latest_jobs(jobs);
    let artist_id_val = artist.id;

    view! {
        // Header
        <div class="bg-white/70 dark:bg-zinc-800/60 backdrop-blur-[16px] border-b border-black/[.06] dark:border-white/[.06] px-6 py-3.5 flex items-center justify-between sticky top-0 z-40">
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
                        <div class={cls(MUTED, "text-[13px] mb-2")}>
                            {format!("{album_count} albums \u{00b7} {monitored_count} monitored \u{00b7} {acquired_count} acquired \u{00b7} {wanted_count} wanted")}
                        </div>
                        <div class="flex flex-wrap gap-1.5">
                            <a href=artist_profile target="_blank" rel="noreferrer" class={cls(BTN, "px-2.5 py-0.5 text-xs")}>"View on Tidal"</a>
                            <button type="button" class={cls(BTN, "px-2.5 py-0.5 text-xs")}
                                on:click=move |_| {
                                    leptos::task::spawn_local(async move {
                                        let _ = dispatch_action(ServerAction::SyncArtistAlbums { artist_id: artist_id_val }).await;
                                    });
                                }>"Sync Albums"</button>
                            <button type="button" class={cls(BTN_PRIMARY, "px-2.5 py-0.5 text-xs")}
                                on:click=move |_| {
                                    leptos::task::spawn_local(async move {
                                        let _ = dispatch_action(ServerAction::BulkMonitor { artist_id: artist_id_val, monitored: true }).await;
                                    });
                                }>"Monitor All"</button>
                            <button type="button" class={cls(BTN, "px-2.5 py-0.5 text-xs")}
                                on:click=move |_| {
                                    leptos::task::spawn_local(async move {
                                        let _ = dispatch_action(ServerAction::BulkMonitor { artist_id: artist_id_val, monitored: false }).await;
                                    });
                                }>"Unmonitor All"</button>
                            <button type="button" class={cls(BTN_DANGER, "px-2.5 py-0.5 text-xs")}
                                on:click=move |_| {
                                    let navigate = leptos_router::hooks::use_navigate();
                                    leptos::task::spawn_local(async move {
                                        let _ = dispatch_action(ServerAction::RemoveArtist { artist_id: artist_id_val }).await;
                                        navigate("/artists", Default::default());
                                    });
                                }>"Remove Artist"</button>
                        </div>
                    </div>
                </div>
            </div>

            // Albums grid
            <div class=GLASS>
                <div class=GLASS_HEADER>
                    <h2 class=GLASS_TITLE>"Discography"</h2>
                    <span class={cls(MUTED, "text-xs")}>{format!("{album_count} albums")}</span>
                </div>
                {if sorted_albums.is_empty() {
                    view! { <div class=EMPTY>"No albums synced. Hit Sync Albums to fetch from Tidal."</div> }.into_any()
                } else {

                    // Which album's tracklist is currently open (None = all collapsed).
                    let (expanded_id, set_expanded_id) = signal::<Option<i64>>(None);
                    view! {
                        <div class={cls(GLASS_BODY, "p-4")}>
                            <div class="d7-album-grid">
                                {sorted_albums.into_iter().map(|album| {
                                    view! { <AlbumSleeve album=album latest_jobs=latest_jobs.clone() expanded_id=expanded_id set_expanded_id=set_expanded_id /> }
                                }).collect_view()}
                            </div>
                        </div>
                    }.into_any()
                }}
            </div>
        </div>
    }
}

/// Album sleeve card + full-width tracklist detail row.
///
/// The card is a normal grid cell. When expanded, a tracklist row spans all
/// columns below it via `grid-column: 1 / -1`.
#[component]
fn AlbumSleeve(
    album: MonitoredAlbum,
    latest_jobs: HashMap<i64, DownloadJob>,
    expanded_id: ReadSignal<Option<i64>>,
    set_expanded_id: WriteSignal<Option<i64>>,
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

    let cover_url = album_cover_url(&album, 640);
    let profile_url = album_profile_url(&album);

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

    // Is *this* album the currently expanded one?
    let is_expanded = move || expanded_id.get() == Some(album_id);

    // Tracks fetched once and cached.
    #[cfg(not(feature = "hydrate"))]
    let (tracks, _) = signal::<Option<Result<Vec<TrackInfo>, String>>>(None);

    #[cfg(feature = "hydrate")]
    let (tracks, set_tracks) = signal::<Option<Result<Vec<TrackInfo>, String>>>(None);

    let on_toggle = move |_| {
        if is_expanded() {
            set_expanded_id.set(None);
        } else {
            set_expanded_id.set(Some(album_id));
            // Fetch on first expand only
            if tracks.get_untracked().is_none() {
                #[cfg(feature = "hydrate")]
                {
                    leptos::task::spawn_local(async move {
                        let result = get_album_tracks(album_id).await;
                        set_tracks.set(Some(result.map_err(|e| e.to_string())));
                    });
                }
            }
        }
    };

    let btn_class = move || {
        if is_expanded() {
            "d7-tracklist-btn active"
        } else {
            "d7-tracklist-btn"
        }
    };

    // We return a fragment: the sleeve card (grid cell) + the detail row (full-width).
    view! {
        // ── Album card (normal grid cell) ───────────────────
        <div class="d7-sleeve" data-album-row data-album-id=album_id_str.clone()>
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

            <div class="d7-sleeve-info">
                <div class="d7-sleeve-title">
                    {match profile_url {
                        Some(url) => view! {
                            <a href=url target="_blank" rel="noreferrer">{album_title.clone()}</a>
                        }.into_any(),
                        None => view! {
                            <span>{album_title.clone()}</span>
                        }.into_any(),
                    }}
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
                            leptos::task::spawn_local(async move {
                                let _ = dispatch_action(ServerAction::ToggleAlbumMonitor { album_id, monitored: next }).await;
                            });
                        }>{monitor_label}</button>
                    {if is_acquired {
                        view! {
                            <button type="button" class={cls(BTN_DANGER, "d7-sleeve-action-btn")} title="Delete downloaded files"
                                on:click=move |_| {
                                    leptos::task::spawn_local(async move {
                                        let _ = dispatch_action(ServerAction::RemoveAlbumFiles { album_id }).await;
                                    });
                                }>
                                "Remove Files"
                            </button>
                        }.into_any()
                    } else {
                        view! { <span></span> }.into_any()
                    }}
                    <button type="button" class=btn_class on:click=on_toggle title="Show tracks">
                        <ListMusic size=14 />
                    </button>
                </div>
            </div>
        </div>

        // ── Tracklist detail row (spans all grid columns) ───
        {move || {
            if !is_expanded() {
                return view! { <span class="hidden"></span> }.into_any();
            }
            view! {
                <div class="col-span-full bg-zinc-900/40 dark:bg-zinc-900/60 backdrop-blur-[8px] border border-white/[.06] rounded-xl p-4 -mt-2">
                    {move || match tracks.get() {
                        None => view! {
                            <div class="text-sm text-zinc-400 py-2">"Loading tracks\u{2026}"</div>
                        }.into_any(),
                        Some(Err(ref err)) => view! {
                            <div class="text-sm text-red-400 py-2">{format!("Failed to load tracks: {err}")}</div>
                        }.into_any(),
                        Some(Ok(ref list)) if list.is_empty() => view! {
                            <div class="text-sm text-zinc-400 py-2">"No tracks found"</div>
                        }.into_any(),
                        Some(Ok(ref list)) => view! {
                            <div class="grid grid-cols-[auto_1fr_auto] gap-x-3 gap-y-1 text-sm">
                                {list.iter().map(|t| view! {
                                    <span class="text-zinc-500 tabular-nums text-right">{t.track_number}</span>
                                    <span class="text-zinc-200 truncate">{t.title.clone()}</span>
                                    <span class="text-zinc-500 tabular-nums">{t.duration_display.clone()}</span>
                                }).collect_view()}
                            </div>
                        }.into_any(),
                    }}
                </div>
            }.into_any()
        }}
    }
}
