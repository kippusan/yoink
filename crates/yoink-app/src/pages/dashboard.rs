use std::collections::HashMap;

use leptos::prelude::*;

use yoink_shared::{
    DownloadJob, DownloadStatus, MonitoredAlbum, MonitoredArtist, ServerAction, build_artist_names,
    status_class, status_label_text,
};

use crate::components::toast::{dispatch_with_toast, dispatch_with_toast_loading};
use crate::components::{ConfirmDialog, ErrorPanel, Sidebar};
use crate::hooks::{set_page_title, use_sse_version};
use crate::styles::{
    BTN, BTN_DANGER, BTN_PRIMARY, EMPTY, GLASS, GLASS_HEADER, GLASS_TITLE, MUTED, btn_cls, cls,
};

// ── Page-specific Tailwind class constants ──────────────────

const STAT_CARD: &str = "d7-stat-card bg-white/70 dark:bg-zinc-800/60 backdrop-blur-[12px] border border-black/[.06] dark:border-white/[.08] rounded-xl p-4 relative overflow-hidden";
const STAT_LABEL: &str =
    "text-xs uppercase tracking-wide text-zinc-500 dark:text-zinc-400 m-0 mb-1";
const STAT_VALUE: &str = "text-[28px] font-bold text-zinc-900 dark:text-zinc-100 m-0";

const TABLE: &str = "w-full border-collapse text-[13px] [&_th]:text-left [&_th]:px-3 [&_th]:py-2.5 [&_th]:font-semibold [&_th]:text-xs [&_th]:uppercase [&_th]:tracking-wide [&_th]:text-zinc-500 dark:[&_th]:text-zinc-400 [&_th]:bg-black/[.02] dark:[&_th]:bg-white/[.02] [&_th]:border-b [&_th]:border-black/[.06] dark:[&_th]:border-white/[.06] [&_th]:whitespace-nowrap [&_td]:px-3 [&_td]:py-2 [&_td]:border-b [&_td]:border-black/[.04] dark:[&_td]:border-white/[.04] [&_td]:text-zinc-600 dark:[&_td]:text-zinc-300 [&_td]:align-middle [&_tbody_tr:hover]:bg-blue-500/[.03] dark:[&_tbody_tr:hover]:bg-blue-500/[.05] [&_tbody_tr:last-child_td]:border-b-0";

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
    set_page_title("Dashboard");
    let version = use_sse_version();
    let data = Resource::new(move || version.get(), |_| get_dashboard_data());

    view! {
        <div class="flex min-h-screen">
            <Sidebar active="dashboard" />
            <div class="ml-[220px] max-md:ml-0 flex-1 min-h-screen">
                <Transition fallback=move || view! {
                    <div>
                        <div class="bg-white/70 dark:bg-zinc-800/60 backdrop-blur-[16px] border-b border-black/[.06] dark:border-white/[.06] px-6 max-md:pl-14 py-3.5 flex items-center justify-between sticky top-0 z-40">
                            <h1 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100 m-0">"Dashboard"</h1>
                        </div>
                        // Skeleton stat cards
                        <div class="p-6 max-md:p-4">
                            <div class="grid grid-cols-[repeat(auto-fill,minmax(180px,1fr))] gap-4 mb-6">
                                {(0..5).map(|_| view! {
                                    <div class="bg-white/70 dark:bg-zinc-800/60 rounded-xl p-4 animate-pulse">
                                        <div class="h-3 w-20 bg-zinc-200 dark:bg-zinc-700 rounded mb-3"></div>
                                        <div class="h-8 w-14 bg-zinc-200 dark:bg-zinc-700 rounded"></div>
                                    </div>
                                }).collect_view()}
                            </div>
                            // Skeleton table rows
                            <div class="bg-white/70 dark:bg-zinc-800/60 rounded-xl overflow-hidden border border-black/[.06] dark:border-white/[.08]">
                                <div class="px-5 py-3 border-b border-black/[.06] dark:border-white/[.06]">
                                    <div class="h-4 w-32 bg-zinc-200 dark:bg-zinc-700 rounded animate-pulse"></div>
                                </div>
                                {(0..6).map(|_| view! {
                                    <div class="flex items-center gap-4 px-5 py-3 border-b border-black/[.04] dark:border-white/[.04] animate-pulse">
                                        <div class="h-3.5 w-32 bg-zinc-200 dark:bg-zinc-700 rounded"></div>
                                        <div class="h-3.5 w-20 bg-zinc-200 dark:bg-zinc-700 rounded"></div>
                                        <div class="h-5 w-14 bg-zinc-200 dark:bg-zinc-700 rounded-full"></div>
                                        <div class="h-3.5 w-10 bg-zinc-200 dark:bg-zinc-700 rounded"></div>
                                        <div class="h-5 w-20 bg-zinc-200 dark:bg-zinc-700 rounded-full"></div>
                                        <div class="h-3.5 w-24 bg-zinc-200 dark:bg-zinc-700 rounded"></div>
                                    </div>
                                }).collect_view()}
                            </div>
                        </div>
                    </div>
                }>
                    {move || {
                        data.get().map(|result| match result {
                            Err(e) => view! {
                                <div class="p-6">
                                    <ErrorPanel
                                        message="Failed to load dashboard data."
                                        details=e.to_string()
                                        retry_href="/"
                                    />
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

    let all_jobs: Vec<_> = {
        let mut sorted = data.jobs;
        sorted.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        sorted
    };
    let total_jobs = all_jobs.len();
    let all_jobs = StoredValue::new(all_jobs);
    let (visible_count, set_visible_count) = signal(25usize);

    let show_clear_completed = RwSignal::new(false);

    // Loading state signals for header buttons
    let scan_loading = RwSignal::new(false);
    let retag_loading = RwSignal::new(false);

    view! {
        // Header bar
        <div class="bg-white/70 dark:bg-zinc-800/60 backdrop-blur-[16px] border-b border-black/[.06] dark:border-white/[.06] px-6 max-md:pl-14 py-3.5 flex items-center justify-between sticky top-0 z-40">
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

            // Confirmation dialog
            <ConfirmDialog
                open=show_clear_completed
                title="Clear Completed"
                message="This will remove all completed download records."
                confirm_label="Clear"
                on_confirm=move |_: bool| {
                    dispatch_with_toast(ServerAction::ClearCompleted, "Completed jobs cleared");
                }
            />

            // Recent activity panel
            <div class=GLASS>
                <div class=GLASS_HEADER>
                    <h2 class=GLASS_TITLE>"Recent Activity"</h2>
                    <div class="flex flex-wrap items-center gap-2">
                        <button type="button"
                            class=move || btn_cls(BTN_PRIMARY, "px-2.5 py-0.5 text-xs", scan_loading.get())
                            disabled=move || scan_loading.get()
                            on:click=move |_| {
                                dispatch_with_toast_loading(ServerAction::ScanImportLibrary, "Library scan started", Some(scan_loading));
                            }>
                            {move || if scan_loading.get() { "Scanning\u{2026}" } else { "Scan Drive + Import" }}
                        </button>
                        <button type="button"
                            class=move || btn_cls(BTN, "px-2.5 py-0.5 text-xs", retag_loading.get())
                            disabled=move || retag_loading.get()
                            on:click=move |_| {
                                dispatch_with_toast_loading(ServerAction::RetagLibrary, "Retagging started", Some(retag_loading));
                            }>
                            {move || if retag_loading.get() { "Retagging\u{2026}" } else { "Retag Existing Files" }}
                        </button>
                        {if has_completed {
                            view! {
                                <button type="button" class={cls(BTN, "px-2.5 py-0.5 text-xs")}
                                    on:click=move |_| {
                                        show_clear_completed.set(true);
                                    }>"Clear Completed"</button>
                            }.into_any()
                        } else {
                            view! { <span></span> }.into_any()
                        }}
                    </div>
                </div>
                {if total_jobs == 0 {
                    view! { <div class=EMPTY>"No download jobs yet."</div> }.into_any()
                } else {
                    let names = artist_names.clone();
                    view! {
                        <div class="overflow-x-auto">
                            <table class=TABLE aria-label="Recent download activity">
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
                                    {move || {
                                        let count = visible_count.get();
                                        let names_inner = names.clone();
                                        all_jobs.with_value(|jobs| {
                                            jobs.iter().take(count).map(|job| {
                                                view! { <JobRow job=job.clone() artist_names=names_inner.clone() /> }
                                            }).collect_view()
                                        })
                                    }}
                                </tbody>
                            </table>
                        </div>
                        // Show more / pagination footer
                        <div class="px-5 py-3 border-t border-black/[.04] dark:border-white/[.04] flex items-center justify-between">
                            <span class={cls(MUTED, "text-xs")}>
                                {move || {
                                    let shown = visible_count.get().min(total_jobs);
                                    format!("Showing {shown} of {total_jobs}")
                                }}
                            </span>
                            <Show when=move || visible_count.get() < total_jobs>
                                <button type="button"
                                    class={cls(BTN, "px-2.5 py-0.5 text-xs")}
                                    on:click=move |_| {
                                        set_visible_count.update(|c| *c += 25);
                                    }>
                                    "Show More"
                                </button>
                            </Show>
                        </div>
                    }.into_any()
                }}
            </div>
        </div>
    }
}

/// A single job row in the recent activity table.
#[component]
fn JobRow(
    job: DownloadJob,
    #[allow(unused_variables)] artist_names: HashMap<String, String>,
) -> impl IntoView {
    let sc = status_class(&job.status).to_string();
    let st_label = status_label_text(&job.status, job.completed_tracks, job.total_tracks);
    let progress = format!("{}/{}", job.completed_tracks, job.total_tracks);
    let artist_name = job.artist_name.clone();
    let updated = job.updated_at.format("%Y-%m-%d %H:%M").to_string();
    let is_queued = matches!(job.status, DownloadStatus::Queued);
    let is_failed = matches!(job.status, DownloadStatus::Failed);
    let job_id_val = job.id.clone();
    let album_id_retry = job.album_id.clone();
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
                                dispatch_with_toast(ServerAction::CancelDownload { job_id: job_id_val.clone() }, "Download cancelled");
                            }>"Cancel"</button>
                    }.into_any()
                } else if is_failed {
                    view! {
                        <button type="button" class={cls(BTN, "px-2.5 py-0.5 text-xs")}
                            on:click=move |_| {
                                dispatch_with_toast(ServerAction::RetryDownload { album_id: album_id_retry.clone() }, "Download queued for retry");
                            }>"Retry"</button>
                    }.into_any()
                } else {
                    view! { <span class=MUTED>{"\u{2014}"}</span> }.into_any()
                }}
            </td>
        </tr>
    }
}
