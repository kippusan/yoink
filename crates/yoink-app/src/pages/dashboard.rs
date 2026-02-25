use std::collections::HashMap;

use leptos::prelude::*;

use yoink_shared::{
    DownloadJob, DownloadStatus, MonitoredAlbum, MonitoredArtist, ServerAction, build_artist_names,
    status_class, status_label_text,
};

use crate::components::Sidebar;
use crate::components::toast::dispatch_with_toast;
use crate::hooks::use_sse_version;

// ── Tailwind class constants (matching old design7) ─────────

const GLASS: &str = "bg-white/70 dark:bg-zinc-800/60 backdrop-blur-[12px] border border-black/[.06] dark:border-white/[.08] rounded-xl mb-6 overflow-hidden";
const GLASS_HEADER: &str = "px-5 py-3.5 border-b border-black/[.06] dark:border-white/[.06] flex items-center justify-between gap-3";
const GLASS_TITLE: &str = "text-[15px] font-semibold text-zinc-900 dark:text-zinc-100 m-0";
const MUTED: &str = "text-zinc-500 dark:text-zinc-400";
const EMPTY: &str = "text-center py-10 px-4 text-zinc-400 dark:text-zinc-600 text-sm";
const BTN: &str = "inline-flex items-center justify-center gap-1.5 px-3.5 py-1.5 bg-white/60 dark:bg-zinc-800/60 backdrop-blur-[8px] border border-black/[.08] dark:border-white/10 rounded-lg font-inherit text-[13px] font-medium cursor-pointer text-zinc-600 dark:text-zinc-300 no-underline transition-all duration-150 whitespace-nowrap hover:bg-white/85 hover:border-blue-500/20 dark:hover:bg-zinc-800/85 dark:hover:border-blue-500/30";
const BTN_PRIMARY: &str = "inline-flex items-center justify-center gap-1.5 px-3.5 py-1.5 bg-blue-500 dark:bg-blue-500 backdrop-blur-[8px] border border-blue-500 rounded-lg font-inherit text-[13px] font-medium cursor-pointer text-white no-underline transition-all duration-150 whitespace-nowrap shadow-[0_2px_12px_rgba(59,130,246,.25)] hover:bg-blue-400 hover:border-blue-400 hover:shadow-[0_4px_20px_rgba(59,130,246,.35)]";
const BTN_DANGER: &str = "inline-flex items-center justify-center gap-1.5 px-3.5 py-1.5 bg-red-500/[.08] dark:bg-red-500/10 backdrop-blur-[8px] border border-red-500/30 dark:border-red-400/30 rounded-lg font-inherit text-[13px] font-medium cursor-pointer text-red-600 dark:text-red-400 no-underline transition-all duration-150 whitespace-nowrap hover:bg-red-500/15 hover:border-red-600 dark:hover:bg-red-500/20 dark:hover:border-red-400";
const STAT_CARD: &str = "d7-stat-card bg-white/70 dark:bg-zinc-800/60 backdrop-blur-[12px] border border-black/[.06] dark:border-white/[.08] rounded-xl p-4 relative overflow-hidden transition-[transform,box-shadow] duration-150 hover:-translate-y-px hover:shadow-[0_4px_24px_rgba(59,130,246,.08)] dark:hover:shadow-[0_4px_24px_rgba(59,130,246,.12)]";
const STAT_LABEL: &str =
    "text-xs uppercase tracking-wide text-zinc-500 dark:text-zinc-400 m-0 mb-1";
const STAT_VALUE: &str = "text-[28px] font-bold text-zinc-900 dark:text-zinc-100 m-0";

const TABLE: &str = "w-full border-collapse text-[13px] [&_th]:text-left [&_th]:px-3 [&_th]:py-2.5 [&_th]:font-semibold [&_th]:text-xs [&_th]:uppercase [&_th]:tracking-wide [&_th]:text-zinc-500 dark:[&_th]:text-zinc-400 [&_th]:bg-black/[.02] dark:[&_th]:bg-white/[.02] [&_th]:border-b [&_th]:border-black/[.06] dark:[&_th]:border-white/[.06] [&_th]:whitespace-nowrap [&_td]:px-3 [&_td]:py-2 [&_td]:border-b [&_td]:border-black/[.04] dark:[&_td]:border-white/[.04] [&_td]:text-zinc-600 dark:[&_td]:text-zinc-300 [&_td]:align-middle [&_tbody_tr:hover]:bg-blue-500/[.03] dark:[&_tbody_tr:hover]:bg-blue-500/[.05] [&_tbody_tr:last-child_td]:border-b-0";

fn cls(a: &str, b: &str) -> String {
    format!("{a} {b}")
}

// ── DTO for server function response ────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DashboardData {
    pub artists: Vec<MonitoredArtist>,
    pub albums: Vec<MonitoredAlbum>,
    pub jobs: Vec<DownloadJob>,
}

// ── Server function ─────────────────────────────────────────

#[server(GetDashboardData, "/leptos")]
pub async fn get_dashboard_data() -> Result<DashboardData, ServerFnError> {
    let ctx = use_context::<yoink_shared::ServerContext>()
        .ok_or_else(|| ServerFnError::new("ServerContext not available"))?;

    let artists = ctx.monitored_artists.read().await.clone();
    let albums = ctx.monitored_albums.read().await.clone();
    let jobs = ctx.download_jobs.read().await.clone();

    Ok(DashboardData {
        artists,
        albums,
        jobs,
    })
}

// ── Page component ──────────────────────────────────────────

#[component]
pub fn DashboardPage() -> impl IntoView {
    let version = use_sse_version();
    let data = Resource::new(move || version.get(), |_| get_dashboard_data());

    view! {
        <div class="flex min-h-screen">
            <Sidebar active="dashboard" />
            <div class="ml-[220px] max-md:ml-0 flex-1 min-h-screen">
                <Transition fallback=move || view! {
                    <div class="bg-white/70 dark:bg-zinc-800/60 backdrop-blur-[16px] border-b border-black/[.06] dark:border-white/[.06] px-6 py-3.5 flex items-center justify-between sticky top-0 z-40">
                        <h1 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100 m-0">"Dashboard"</h1>
                        <span class={cls(MUTED, "text-[13px]")}>"Loading\u{2026}"</span>
                    </div>
                }>
                    {move || {
                        data.get().map(|result| match result {
                            Err(e) => view! {
                                <div class="p-6">
                                    <div class="text-red-500">{format!("Error loading dashboard: {e}")}</div>
                                </div>
                            }.into_any(),
                            Ok(data) => {
                                view! { <DashboardContent data=data /> }.into_any()
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
fn DashboardContent(data: DashboardData) -> impl IntoView {
    let artist_names = build_artist_names(&data.artists);
    let monitored_count = data.artists.len();
    let monitored_albums = data.albums.iter().filter(|a| a.monitored).count();
    let wanted_albums = data.albums.iter().filter(|a| a.wanted).count();
    let acquired_albums = data.albums.iter().filter(|a| a.acquired).count();
    let queued_jobs = data
        .jobs
        .iter()
        .filter(|j| {
            matches!(
                j.status,
                DownloadStatus::Queued | DownloadStatus::Resolving | DownloadStatus::Downloading
            )
        })
        .count();

    let has_completed = data
        .jobs
        .iter()
        .any(|j| matches!(j.status, DownloadStatus::Completed));

    let recent_jobs: Vec<_> = {
        let mut sorted = data.jobs;
        sorted.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        sorted.into_iter().take(25).collect()
    };

    view! {
        // Header bar
        <div class="bg-white/70 dark:bg-zinc-800/60 backdrop-blur-[16px] border-b border-black/[.06] dark:border-white/[.06] px-6 py-3.5 flex items-center justify-between sticky top-0 z-40">
            <h1 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100 m-0">"Dashboard"</h1>
        </div>

        // Content
        <div class="p-6 max-md:p-4">
            // Stat cards
            <div class="grid grid-cols-[repeat(auto-fill,minmax(180px,1fr))] gap-4 mb-6">
                <div class=STAT_CARD>
                    <p class=STAT_LABEL>"Artists"</p>
                    <p class=STAT_VALUE>{monitored_count}</p>
                </div>
                <div class=STAT_CARD>
                    <p class=STAT_LABEL>"Monitored Albums"</p>
                    <p class=STAT_VALUE>{monitored_albums}</p>
                </div>
                <div class=STAT_CARD>
                    <p class=STAT_LABEL>"Wanted"</p>
                    <p class=STAT_VALUE>{wanted_albums}</p>
                </div>
                <div class=STAT_CARD>
                    <p class=STAT_LABEL>"Acquired"</p>
                    <p class=STAT_VALUE>{acquired_albums}</p>
                </div>
                <div class=STAT_CARD>
                    <p class=STAT_LABEL>"Active Jobs"</p>
                    <p class=STAT_VALUE>{queued_jobs}</p>
                </div>
            </div>

            // Recent activity panel
            <div class=GLASS>
                <div class=GLASS_HEADER>
                    <h2 class=GLASS_TITLE>"Recent Activity"</h2>
                    <div class="flex flex-wrap items-center gap-2">
                        <button type="button" class={cls(BTN_PRIMARY, "px-2.5 py-0.5 text-xs")}
                            on:click=move |_| {
                                dispatch_with_toast(ServerAction::ScanImportLibrary, "Library scan started");
                            }>"Scan Drive + Import"</button>
                        <button type="button" class={cls(BTN, "px-2.5 py-0.5 text-xs")}
                            on:click=move |_| {
                                dispatch_with_toast(ServerAction::RetagLibrary, "Retagging started");
                            }>"Retag Existing Files"</button>
                        {if has_completed {
                            view! {
                                <button type="button" class={cls(BTN, "px-2.5 py-0.5 text-xs")}
                                    on:click=move |_| {
                                        dispatch_with_toast(ServerAction::ClearCompleted, "Completed jobs cleared");
                                    }>"Clear Completed"</button>
                            }.into_any()
                        } else {
                            view! { <span></span> }.into_any()
                        }}
                    </div>
                </div>
                {if recent_jobs.is_empty() {
                    view! { <div class=EMPTY>"No download jobs yet."</div> }.into_any()
                } else {
                    view! {
                        <table class=TABLE>
                            <thead>
                                <tr>
                                    <th>"Album"</th>
                                    <th>"Artist"</th>
                                    <th>"Quality"</th>
                                    <th>"Progress"</th>
                                    <th>"Status"</th>
                                    <th>"Updated"</th>
                                    <th>"Actions"</th>
                                </tr>
                            </thead>
                            <tbody>
                                {recent_jobs.into_iter().map(|job| {
                                    view! { <JobRow job=job artist_names=artist_names.clone() /> }
                                }).collect_view()}
                            </tbody>
                        </table>
                    }.into_any()
                }}
            </div>
        </div>
    }
}

/// A single job row in the recent activity table.
#[component]
fn JobRow(job: DownloadJob, artist_names: HashMap<i64, String>) -> impl IntoView {
    let sc = status_class(&job.status).to_string();
    let st_label = status_label_text(&job.status, job.completed_tracks, job.total_tracks);
    let progress = format!("{}/{}", job.completed_tracks, job.total_tracks);
    let artist_name = artist_names
        .get(&job.artist_id)
        .cloned()
        .unwrap_or_else(|| format!("#{}", job.artist_id));
    let updated = job.updated_at.format("%Y-%m-%d %H:%M").to_string();
    let is_queued = matches!(job.status, DownloadStatus::Queued);
    let is_failed = matches!(job.status, DownloadStatus::Failed);
    let job_id_val = job.id;
    let album_id_val = job.album_id;
    let error_msg = job.error.clone().unwrap_or_default();

    view! {
        <tr>
            <td>
                <div>{job.album_title}</div>
                {if is_failed && !error_msg.is_empty() {
                    view! { <small class="text-[11px] text-red-600 dark:text-red-400">{error_msg}</small> }.into_any()
                } else {
                    view! { <span></span> }.into_any()
                }}
            </td>
            <td>{artist_name}</td>
            <td><span class="pill d7-pill-muted">{job.quality}</span></td>
            <td>{progress}</td>
            <td><span class=sc>{st_label}</span></td>
            <td class=MUTED>{updated}</td>
            <td>
                {if is_queued {
                    view! {
                        <button type="button" class={cls(BTN_DANGER, "px-2.5 py-0.5 text-xs")}
                            on:click=move |_| {
                                dispatch_with_toast(ServerAction::CancelDownload { job_id: job_id_val }, "Download cancelled");
                            }>"Cancel"</button>
                    }.into_any()
                } else if is_failed {
                    view! {
                        <button type="button" class={cls(BTN, "px-2.5 py-0.5 text-xs")}
                            on:click=move |_| {
                                dispatch_with_toast(ServerAction::RetryDownload { album_id: album_id_val }, "Download queued for retry");
                            }>"Retry"</button>
                    }.into_any()
                } else {
                    view! { <span class=MUTED>{"\u{2014}"}</span> }.into_any()
                }}
            </td>
        </tr>
    }
}
