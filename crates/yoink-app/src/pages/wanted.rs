use std::collections::HashMap;

use leptos::prelude::*;
use lucide_leptos::X;

use yoink_shared::{
    album_cover_url, album_profile_url, build_albums_by_artist, build_artist_names,
    build_latest_jobs, status_class, DownloadJob, DownloadStatus, MonitoredAlbum, MonitoredArtist,
    ServerAction,
};

use crate::actions::dispatch_action;

use crate::components::Sidebar;
use crate::hooks::use_sse_version;

// ── Tailwind class constants (matching old design7) ─────────

const GLASS: &str = "bg-white/70 dark:bg-zinc-800/60 backdrop-blur-[12px] border border-black/[.06] dark:border-white/[.08] rounded-xl mb-6 overflow-hidden";
const GLASS_HEADER: &str = "px-5 py-3.5 border-b border-black/[.06] dark:border-white/[.06] flex items-center justify-between gap-3";
const GLASS_TITLE: &str = "text-[15px] font-semibold text-zinc-900 dark:text-zinc-100 m-0";
const MUTED: &str = "text-zinc-500 dark:text-zinc-400";
const EMPTY: &str = "text-center py-10 px-4 text-zinc-400 dark:text-zinc-600 text-sm";
const BTN_DANGER: &str = "inline-flex items-center justify-center gap-1.5 px-3.5 py-1.5 bg-red-500/[.08] dark:bg-red-500/10 backdrop-blur-[8px] border border-red-500/30 dark:border-red-400/30 rounded-lg font-inherit text-[13px] font-medium cursor-pointer text-red-600 dark:text-red-400 no-underline transition-all duration-150 whitespace-nowrap hover:bg-red-500/15 hover:border-red-600 dark:hover:bg-red-500/20 dark:hover:border-red-400";
const ICON_BTN: &str = "inline-flex items-center justify-center size-7 border border-black/[.08] dark:border-white/10 rounded-lg bg-white/50 dark:bg-zinc-800/50 backdrop-blur-[8px] text-zinc-500 dark:text-zinc-400 cursor-pointer transition-all duration-150 p-0 font-inherit text-[13px] hover:bg-blue-500 hover:border-blue-500 hover:text-white hover:shadow-[0_2px_8px_rgba(59,130,246,.3)]";
const WANTED_CARD: &str = "flex items-center gap-3.5 px-5 py-3.5 border-b border-black/[.04] dark:border-white/[.04] transition-[background] duration-[120ms] last:border-b-0 hover:bg-blue-500/[.03] dark:hover:bg-blue-500/[.05]";
const WANTED_THUMB: &str = "size-12 rounded-lg object-cover shrink-0 bg-zinc-200 dark:bg-zinc-800 shadow-[0_2px_8px_rgba(0,0,0,.08)] dark:shadow-[0_2px_8px_rgba(0,0,0,.3)]";
const WANTED_THUMB_FALLBACK: &str = "size-12 rounded-lg inline-flex items-center justify-center bg-zinc-200 dark:bg-zinc-800 text-zinc-400 dark:text-zinc-600 font-bold text-base shrink-0";
const GROUP_HEADER: &str = "text-[13px] font-bold text-blue-500 dark:text-blue-400 px-5 py-2.5 border-b border-black/[.04] dark:border-white/[.04] bg-blue-500/[.03] dark:bg-blue-500/[.05] uppercase tracking-wide";

fn cls(a: &str, b: &str) -> String {
    format!("{a} {b}")
}

// ── DTO for server function response ────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WantedData {
    pub wanted: Vec<MonitoredAlbum>,
    pub artists: Vec<MonitoredArtist>,
    pub jobs: Vec<DownloadJob>,
}

// ── Server function ─────────────────────────────────────────

#[server(GetWantedData, "/leptos")]
pub async fn get_wanted_data() -> Result<WantedData, ServerFnError> {
    // ServerContext was provided via provide_context in main.rs
    let ctx = use_context::<yoink_shared::ServerContext>()
        .ok_or_else(|| ServerFnError::new("ServerContext not available"))?;

    let artists = ctx.monitored_artists.read().await.clone();
    let jobs = ctx.download_jobs.read().await.clone();
    let wanted: Vec<MonitoredAlbum> = ctx
        .monitored_albums
        .read()
        .await
        .iter()
        .filter(|a| a.wanted)
        .cloned()
        .collect();

    Ok(WantedData {
        wanted,
        artists,
        jobs,
    })
}

// ── Page component ──────────────────────────────────────────

/// Wanted page — shows albums that are wanted but not yet acquired.
#[component]
pub fn WantedPage() -> impl IntoView {
    let version = use_sse_version();
    let wanted_data = Resource::new(move || version.get(), |_| get_wanted_data());

    view! {
        <div class="flex min-h-screen">
            <Sidebar active="wanted" />
            <div class="ml-[220px] max-md:ml-0 flex-1 min-h-screen">
                <Transition fallback=move || view! {
                    <div class="bg-white/70 dark:bg-zinc-800/60 backdrop-blur-[16px] border-b border-black/[.06] dark:border-white/[.06] px-6 py-3.5 flex items-center justify-between sticky top-0 z-40">
                        <h1 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100 m-0">"Wanted"</h1>
                        <span class={cls(MUTED, "text-[13px]")}>"Loading\u{2026}"</span>
                    </div>
                }>
                    {move || {
                        wanted_data.get().map(|result| match result {
                            Err(e) => view! {
                                <div class="p-6">
                                    <div class="text-red-500">{format!("Error loading wanted data: {e}")}</div>
                                </div>
                            }.into_any(),
                            Ok(data) => {
                                view! { <WantedContent data=data /> }.into_any()
                            }
                        })
                    }}
                </Transition>
            </div>
        </div>
    }
}

/// Inner content rendered once data is loaded.
#[component]
fn WantedContent(data: WantedData) -> impl IntoView {
    let artist_names = build_artist_names(&data.artists);
    let latest_jobs = build_latest_jobs(data.jobs);
    let albums_by_artist = build_albums_by_artist(data.wanted);

    let mut artist_order: Vec<(i64, String)> = albums_by_artist
        .keys()
        .map(|&aid| {
            let name = artist_names
                .get(&aid)
                .cloned()
                .unwrap_or_else(|| format!("Unknown ({aid})"));
            (aid, name)
        })
        .collect();
    artist_order.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));

    let total_wanted: usize = albums_by_artist.values().map(|v| v.len()).sum();
    let total_str = format!("{total_wanted} albums");

    view! {
        // Header bar
        <div class="bg-white/70 dark:bg-zinc-800/60 backdrop-blur-[16px] border-b border-black/[.06] dark:border-white/[.06] px-6 py-3.5 flex items-center justify-between sticky top-0 z-40">
            <h1 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100 m-0">"Wanted"</h1>
            <span class={cls(MUTED, "text-[13px]")}>{total_str.clone()}</span>
        </div>

        // Content
        <div class="p-6 max-md:p-4">
            <div class=GLASS>
                <div class=GLASS_HEADER>
                    <h2 class=GLASS_TITLE>"Missing Albums"</h2>
                    <span class={cls(MUTED, "text-xs")}>{total_str}</span>
                </div>

                {if total_wanted == 0 {
                    view! { <div class=EMPTY>"All albums acquired. Nothing wanted."</div> }.into_any()
                } else {
                    view! {
                        <div>
                            {artist_order.into_iter().map(|(artist_id, artist_name)| {
                                let group_albums = albums_by_artist
                                    .get(&artist_id)
                                    .cloned()
                                    .unwrap_or_default();
                                let jobs_ref = latest_jobs.clone();
                                view! {
                                    <div class=GROUP_HEADER>{artist_name}</div>
                                    {group_albums.into_iter().map(move |album| {
                                        let jobs_inner = jobs_ref.clone();
                                        view! { <WantedRow album=album latest_jobs=jobs_inner /> }
                                    }).collect_view()}
                                }
                            }).collect_view()}
                        </div>
                    }.into_any()
                }}
            </div>
        </div>
    }
}

/// A single wanted album row.
#[component]
fn WantedRow(album: MonitoredAlbum, latest_jobs: HashMap<i64, DownloadJob>) -> impl IntoView {
    let album_title = album.title.clone();
    let release_date = album
        .release_date
        .clone()
        .unwrap_or_else(|| "\u{2014}".to_string());
    let is_explicit = album.explicit;

    let cover_url = album_cover_url(&album, 160);
    let profile_url = album_profile_url(&album);
    let fallback_initial = album_title
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_string());

    let latest_job = latest_jobs.get(&album.id).cloned();
    let job_status = latest_job.as_ref().map(|j| j.status.clone());
    let job_error = latest_job.as_ref().and_then(|j| j.error.clone());
    let is_failed = matches!(job_status, Some(DownloadStatus::Failed));

    let sc = match &job_status {
        Some(s) => status_class(s).to_string(),
        None => "pill".to_string(),
    };
    let status_text = match &job_status {
        Some(s) => s.as_str().to_string(),
        None => "not queued".to_string(),
    };

    let error_class = if is_failed {
        "text-[11px] text-red-600 dark:text-red-400"
    } else {
        "text-[11px] text-red-600 dark:text-red-400 hidden"
    };
    let error_text = job_error.unwrap_or_else(|| "Download failed".to_string());
    let explicit_label = if is_explicit { " [E]" } else { "" };
    let meta_text = format!("{release_date}{explicit_label}");

    let album_id_val = album.id;
    let album_id_str = album.id.to_string();

    view! {
        <div class=WANTED_CARD data-album-id=album_id_str>
            // Cover thumbnail
            {match cover_url {
                Some(url) => view! {
                    <img class=WANTED_THUMB src=url alt="" />
                }.into_any(),
                None => view! {
                    <div class=WANTED_THUMB_FALLBACK>{fallback_initial}</div>
                }.into_any(),
            }}

            // Album info
            <div class="flex-1 min-w-0">
                <div class="text-sm font-semibold text-zinc-900 dark:text-zinc-100 whitespace-nowrap overflow-hidden text-ellipsis">
                    {match profile_url {
                        Some(url) => view! {
                            <a href=url target="_blank" rel="noreferrer" class="text-inherit no-underline hover:text-blue-500">{album_title.clone()}</a>
                        }.into_any(),
                        None => view! {
                            <span>{album_title.clone()}</span>
                        }.into_any(),
                    }}
                </div>
                <div class="text-xs text-zinc-500 dark:text-zinc-400">{meta_text}</div>
                <small class=error_class>{error_text}</small>
            </div>

            // Status pill
            <span class=sc>{status_text}</span>

            // Actions
            <div class="flex gap-1.5 shrink-0 items-center">
                {if is_failed {
                    view! {
                        <button type="button" class={cls(BTN_DANGER, "px-2.5 py-0.5 text-xs")}
                            on:click=move |_| {
                                leptos::task::spawn_local(async move {
                                    let _ = dispatch_action(ServerAction::RetryDownload { album_id: album_id_val }).await;
                                });
                            }>"Retry"</button>
                    }.into_any()
                } else {
                    view! { <span></span> }.into_any()
                }}
                <button type="button" class=ICON_BTN title="Unmonitor"
                    on:click=move |_| {
                        leptos::task::spawn_local(async move {
                            let _ = dispatch_action(ServerAction::ToggleAlbumMonitor { album_id: album_id_val, monitored: false }).await;
                        });
                    }>
                    <X size=14 />
                </button>
            </div>
        </div>
    }
}
