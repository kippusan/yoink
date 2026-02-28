use leptos::prelude::*;
use lucide_leptos::{ArrowLeft, ChevronRight};

use yoink_shared::{
    DownloadJob, DownloadStatus, MatchSuggestion, MonitoredAlbum, MonitoredArtist, ProviderLink,
    ServerAction, TrackInfo, album_cover_url, album_type_label, build_latest_jobs,
    provider_display_name, status_class, status_label_text,
};

use crate::components::toast::{dispatch_with_toast, dispatch_with_toast_loading};
use crate::components::{ConfirmDialog, ErrorPanel, MobileMenuButton, Sidebar};
use crate::hooks::{set_page_title, use_sse_version};
use super::provider_icon_svg;
use crate::styles::{
    BTN, BTN_DANGER, BTN_PRIMARY, BREADCRUMB_CURRENT, BREADCRUMB_LINK, BREADCRUMB_NAV,
    BREADCRUMB_SEP, GLASS, GLASS_BODY, GLASS_HEADER, GLASS_TITLE, HEADER_BAR, MUTED, btn_cls, cls,
};

// ── DTO ─────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AlbumDetailData {
    pub album: Option<MonitoredAlbum>,
    pub artist: Option<MonitoredArtist>,
    pub tracks: Vec<TrackInfo>,
    pub jobs: Vec<DownloadJob>,
    pub provider_links: Vec<ProviderLink>,
    pub match_suggestions: Vec<MatchSuggestion>,
}

// ── Server function ─────────────────────────────────────────

#[server(GetAlbumDetail, "/leptos")]
pub async fn get_album_detail(album_id: String) -> Result<AlbumDetailData, ServerFnError> {
    let ctx = use_context::<yoink_shared::ServerContext>()
        .ok_or_else(|| ServerFnError::new("ServerContext not available"))?;

    let albums = ctx.monitored_albums.read().await;
    let album = albums.iter().find(|a| a.id == album_id).cloned();
    drop(albums);

    let artist = if let Some(ref a) = album {
        let artists = ctx.monitored_artists.read().await;
        artists.iter().find(|ar| ar.id == a.artist_id).cloned()
    } else {
        None
    };

    let tracks = (ctx.fetch_tracks)(album_id.clone())
        .await
        .unwrap_or_default();

    let jobs = ctx.download_jobs.read().await.clone();

    let provider_links = if album.is_some() {
        (ctx.fetch_album_links)(album_id.clone())
            .await
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    let match_suggestions = if album.is_some() {
        (ctx.fetch_album_match_suggestions)(album_id)
            .await
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    Ok(AlbumDetailData {
        album,
        artist,
        tracks,
        jobs,
        provider_links,
        match_suggestions,
    })
}

// ── Page component ──────────────────────────────────────────

#[component]
pub fn AlbumDetailPage() -> impl IntoView {
    set_page_title("Album");
    let params = leptos_router::hooks::use_params_map();
    let album_id = move || params.read().get("album_id").unwrap_or_default();
    let artist_id = move || params.read().get("artist_id").unwrap_or_default();

    let version = use_sse_version();
    let data = Resource::new(
        move || (album_id(), version.get()),
        |(id, _)| get_album_detail(id),
    );

    view! {
        <div class="flex min-h-screen">
            <Sidebar active="artists" />
            <div class="ml-[220px] max-md:ml-0 flex-1 min-h-screen">
                <Transition fallback=move || view! {
                    <div>
                        <div class=HEADER_BAR>
                            <nav class=BREADCRUMB_NAV aria-label="Breadcrumb"><MobileMenuButton />
                                <a href="/artists" class=BREADCRUMB_LINK>"Artists"</a>
                                <span class=BREADCRUMB_SEP><ChevronRight /></span>
                                <div class="h-4 w-20 bg-zinc-200 dark:bg-zinc-700 rounded animate-pulse"></div>
                                <span class=BREADCRUMB_SEP><ChevronRight /></span>
                                <div class="h-4 w-28 bg-zinc-200 dark:bg-zinc-700 rounded animate-pulse"></div>
                            </nav>
                        </div>
                        <div class="p-6 max-md:p-4">
                            // Skeleton hero card
                            <div class="mb-5 bg-white/70 dark:bg-zinc-800/60 rounded-xl border border-black/[.06] dark:border-white/[.08] p-5">
                                <div class="flex flex-col md:flex-row gap-6 animate-pulse">
                                    <div class="size-60 max-md:size-48 max-md:mx-auto rounded-xl bg-zinc-200 dark:bg-zinc-700 shrink-0"></div>
                                    <div class="flex-1 min-w-0">
                                        <div class="h-4 w-32 bg-zinc-200 dark:bg-zinc-700 rounded mb-3"></div>
                                        <div class="h-7 w-56 bg-zinc-200 dark:bg-zinc-700 rounded mb-3"></div>
                                        <div class="h-3.5 w-40 bg-zinc-200 dark:bg-zinc-700 rounded mb-4"></div>
                                        <div class="flex flex-wrap gap-1.5 mb-4">
                                            {(0..4).map(|_| view! {
                                                <div class="h-7 w-20 bg-zinc-200 dark:bg-zinc-700 rounded-lg"></div>
                                            }).collect_view()}
                                        </div>
                                        <div class="flex flex-wrap gap-1.5">
                                            {(0..3).map(|_| view! {
                                                <div class="h-8 w-24 bg-zinc-200 dark:bg-zinc-700 rounded-lg"></div>
                                            }).collect_view()}
                                        </div>
                                    </div>
                                </div>
                            </div>
                            // Skeleton tracklist
                            <div class="bg-white/70 dark:bg-zinc-800/60 rounded-xl border border-black/[.06] dark:border-white/[.08] overflow-hidden">
                                <div class="px-5 py-3 border-b border-black/[.06] dark:border-white/[.06]">
                                    <div class="h-4 w-24 bg-zinc-200 dark:bg-zinc-700 rounded animate-pulse"></div>
                                </div>
                                <div class="p-5">
                                    {(0..8).map(|_| view! {
                                        <div class="flex gap-3 mb-2.5 animate-pulse">
                                            <div class="h-3.5 w-6 bg-zinc-200 dark:bg-zinc-700 rounded"></div>
                                            <div class="h-3.5 flex-1 bg-zinc-200 dark:bg-zinc-700 rounded"></div>
                                            <div class="h-3.5 w-10 bg-zinc-200 dark:bg-zinc-700 rounded"></div>
                                        </div>
                                    }).collect_view()}
                                </div>
                            </div>
                        </div>
                    </div>
                }>
                    {move || {
                        let aid = artist_id();
                        data.get().map(|result| match result {
                            Err(e) => view! {
                                <div class="p-6">
                                    <ErrorPanel
                                        message="Failed to load album details."
                                        details=e.to_string()
                                    />
                                </div>
                            }.into_any(),
                            Ok(data) => match data.album {
                                None => {
                                    let back_href = format!("/artists/{}", aid);
                                    view! {
                                        <div>
                                            <div class=HEADER_BAR>
                                                <nav class=BREADCRUMB_NAV aria-label="Breadcrumb"><MobileMenuButton />
                                                    <a href="/artists" class=BREADCRUMB_LINK>"Artists"</a>
                                                    <span class=BREADCRUMB_SEP><ChevronRight /></span>
                                                    <a href=back_href class=BREADCRUMB_LINK>"Artist"</a>
                                                    <span class=BREADCRUMB_SEP><ChevronRight /></span>
                                                    <span class=BREADCRUMB_CURRENT>"Not Found"</span>
                                                </nav>
                                            </div>
                                            <div class="p-6">
                                                <div class="text-zinc-500">"Album not found."</div>
                                                <a href="/artists" class={cls(BTN, "mt-4 inline-flex items-center gap-1.5")}>
                                                    <ArrowLeft size=14 />
                                                    "All Artists"
                                                </a>
                                            </div>
                                        </div>
                                    }.into_any()
                                }
                                Some(album) => {
                                    view! {
                                        <AlbumDetailContent
                                            album=album
                                            artist=data.artist
                                            tracks=data.tracks
                                            jobs=data.jobs
                                            provider_links=data.provider_links
                                            match_suggestions=data.match_suggestions
                                            artist_id_param=aid
                                        />
                                    }.into_any()
                                }
                            }
                        })
                    }}
                </Transition>
            </div>
        </div>
    }
}

// ── Detail content ──────────────────────────────────────────

#[component]
fn AlbumDetailContent(
    album: MonitoredAlbum,
    artist: Option<MonitoredArtist>,
    tracks: Vec<TrackInfo>,
    jobs: Vec<DownloadJob>,
    provider_links: Vec<ProviderLink>,
    match_suggestions: Vec<MatchSuggestion>,
    artist_id_param: String,
) -> impl IntoView {
    set_page_title(&album.title);

    let album_id_retry = album.id.clone();
    let album_id_retry2 = album.id.clone();
    let album_id_monitor = album.id.clone();
    let album_id_remove_confirm = album.id.clone();
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

    let latest_jobs = build_latest_jobs(jobs);
    let latest_job = latest_jobs.get(&album.id).cloned();
    let job_status = latest_job.as_ref().map(|j| j.status.clone());
    let job_progress = latest_job
        .as_ref()
        .map(|j| (j.completed_tracks, j.total_tracks));
    let job_error = latest_job.as_ref().and_then(|j| j.error.clone());
    let job_quality = latest_job.as_ref().map(|j| j.quality.clone());

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

    let artist_name = artist
        .as_ref()
        .map(|a| a.name.clone())
        .unwrap_or_else(|| "Unknown Artist".to_string());
    let artist_link = format!("/artists/{artist_id_param}");

    let fallback_initial = album_title
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_string());

    let total_duration_secs: u32 = tracks.iter().map(|t| t.duration_secs).sum();
    let total_mins = total_duration_secs / 60;
    let total_secs = total_duration_secs % 60;
    let track_count = tracks.len();
    let duration_display = if total_mins >= 60 {
        let hrs = total_mins / 60;
        let mins = total_mins % 60;
        format!("{hrs} hr {mins} min")
    } else {
        format!("{total_mins} min {total_secs:02} sec")
    };

    // Confirmation dialog signals
    let show_remove_files = RwSignal::new(false);

    // Loading state signals
    let download_loading = RwSignal::new(false);

    // Can we show a download / retry button?
    let can_download = is_wanted && !is_acquired;
    let can_retry = matches!(job_status, Some(DownloadStatus::Failed));

    let monitor_title = if is_monitored {
        "Unmonitor album"
    } else {
        "Monitor album"
    };
    let monitor_label = if is_monitored { "Unmonitor" } else { "Monitor" };

    view! {
        // ── Sticky header with breadcrumb ───────────────────
        <div class=HEADER_BAR>
            <nav class=BREADCRUMB_NAV aria-label="Breadcrumb"><MobileMenuButton />
                <a href="/artists" class=BREADCRUMB_LINK>"Artists"</a>
                <span class=BREADCRUMB_SEP><ChevronRight /></span>
                <a href=artist_link.clone() class=BREADCRUMB_LINK>
                    {artist_name.clone()}
                </a>
                <span class=BREADCRUMB_SEP><ChevronRight /></span>
                <span class=BREADCRUMB_CURRENT>{album_title.clone()}</span>
            </nav>
        </div>

        <div class="p-6 max-md:p-4">
            // ── Hero card ───────────────────────────────────
            <div class={cls(GLASS, "mb-5")}>
                <div class={cls(GLASS_BODY, "p-5 md:p-6")}>
                    // Top row: cover art + title/meta side by side
                    <div class="flex gap-5 mb-4">
                        // Cover art — compact square, no glow effects
                        <div class="shrink-0 w-28 h-28 md:w-40 md:h-40 rounded-lg overflow-hidden bg-zinc-200 dark:bg-zinc-800">
                            {match cover_url.clone() {
                                Some(url) => view! {
                                    <img class="w-full h-full object-cover" src=url alt="" />
                                }.into_any(),
                                None => view! {
                                    <div class="w-full h-full flex items-center justify-center text-3xl font-bold text-zinc-400 dark:text-zinc-600">{fallback_initial}</div>
                                }.into_any(),
                            }}
                        </div>

                        // Core identity: type, title, artist, date
                        <div class="flex-1 min-w-0 flex flex-col justify-center">
                            // Album type + explicit
                            <div class="flex items-center gap-2 mb-1">
                                <span class="text-[11px] font-semibold uppercase tracking-wider text-zinc-400 dark:text-zinc-500">{at}</span>
                                {if is_explicit {
                                    view! { <span class="text-[10px] px-1.5 py-0 rounded bg-zinc-200/80 dark:bg-zinc-700/80 text-zinc-500 dark:text-zinc-400 font-medium">"Explicit"</span> }.into_any()
                                } else {
                                    view! { <span></span> }.into_any()
                                }}
                            </div>

                            // Title — wraps on narrow screens
                            <h1 class="text-xl md:text-2xl font-bold text-zinc-900 dark:text-zinc-100 m-0 mb-1.5 leading-snug break-words">{album_title.clone()}</h1>

                            // Artist · date · tracks
                            <div class={cls(MUTED, "text-sm flex flex-wrap items-center gap-1.5")}>
                                <a href=artist_link.clone() class="text-zinc-600 dark:text-zinc-300 hover:text-blue-500 dark:hover:text-blue-400 no-underline font-medium">
                                    {artist_name.clone()}
                                </a>
                                <span>"\u{00b7}"</span>
                                <span>{release_date.clone()}</span>
                                <span>"\u{00b7}"</span>
                                <span>{format!("{track_count} tracks, {duration_display}")}</span>
                            </div>

                            // Provider links (inline under meta on wider screens)
                            {if !provider_links.is_empty() {
                                view! {
                                    <div class="flex flex-wrap items-center gap-1.5 mt-2">
                                        <span class="text-[11px] text-zinc-400 dark:text-zinc-500">"Available on"</span>
                                        {provider_links.iter().map(|link| {
                                            let display = provider_display_name(&link.provider);
                                            let external_url = link.external_url.clone();
                                            let icon_svg = provider_icon_svg(&link.provider);
                                            match external_url {
                                                Some(url) => view! {
                                                    <a href=url class="inline-flex items-center gap-1 px-2 py-0.5 text-[11px] font-medium text-blue-600 dark:text-blue-400 bg-blue-500/[.08] border border-blue-500/20 rounded-md no-underline hover:bg-blue-500/15" target="_blank" rel="noreferrer">
                                                        <span class="shrink-0 [&>svg]:size-3 text-blue-500/60 dark:text-blue-400/60" inner_html=icon_svg></span>
                                                        {display}
                                                    </a>
                                                }.into_any(),
                                                None => view! {
                                                    <span class="inline-flex items-center gap-1 px-2 py-0.5 text-[11px] font-medium text-zinc-500 dark:text-zinc-400 bg-zinc-500/[.08] border border-zinc-500/20 rounded-md">
                                                        <span class="shrink-0 [&>svg]:size-3 text-zinc-400/60 dark:text-zinc-500/60" inner_html=icon_svg></span>
                                                        {display}
                                                    </span>
                                                }.into_any(),
                                            }
                                        }).collect_view()}
                                    </div>
                                }.into_any()
                            } else {
                                view! { <span></span> }.into_any()
                            }}
                        </div>
                    </div>

                    // Potential matches (full width below the top row)
                    {if match_suggestions.iter().any(|m| m.status == "pending") {
                        view! {
                            <div class="mb-4 rounded-lg border border-amber-500/20 bg-amber-500/[.06] px-3 py-2.5">
                                <div class="text-[11px] uppercase tracking-wider text-amber-700 dark:text-amber-300 mb-2 font-semibold">
                                    "Potential Matches"
                                </div>
                                <div class="flex flex-col gap-2">
                                    {match_suggestions.iter().filter(|m| m.status == "pending").map(|m| {
                                        let accept_id = m.id.clone();
                                        let dismiss_id = m.id.clone();
                                        let display_provider = provider_display_name(&m.right_provider);
                                        let kind = if m.match_kind == "isrc_exact" { "ISRC" } else { "Fuzzy" };
                                        let display_name = m
                                            .external_name
                                            .clone()
                                            .unwrap_or_else(|| "Unknown album match".to_string());
                                        let explanation = m.explanation.clone().unwrap_or_default();
                                        view! {
                                            <div class="flex flex-wrap items-center gap-2 text-xs text-zinc-700 dark:text-zinc-300">
                                                <span class="inline-flex items-center px-1.5 py-0.5 rounded-md bg-white/70 dark:bg-zinc-800/70 border border-black/[.06] dark:border-white/[.08]">
                                                    {format!("{} {}%", kind, m.confidence)}
                                                </span>
                                                <span>{format!("{}: {}", display_provider, display_name)}</span>
                                                <span class="text-zinc-500 dark:text-zinc-400">{explanation}</span>
                                                <button
                                                    type="button"
                                                    class={cls(BTN_PRIMARY, "px-2 py-0.5 text-[11px]")}
                                                    on:click=move |_| {
                                                        dispatch_with_toast(
                                                            ServerAction::AcceptMatchSuggestion { suggestion_id: accept_id.clone() },
                                                            "Match accepted",
                                                        );
                                                    }
                                                >
                                                    "Accept"
                                                </button>
                                                <button
                                                    type="button"
                                                    class={cls(BTN, "px-2 py-0.5 text-[11px]")}
                                                    on:click=move |_| {
                                                        dispatch_with_toast(
                                                            ServerAction::DismissMatchSuggestion { suggestion_id: dismiss_id.clone() },
                                                            "Match dismissed",
                                                        );
                                                    }
                                                >
                                                    "Dismiss"
                                                </button>
                                            </div>
                                        }
                                    }).collect_view()}
                                </div>
                            </div>
                        }.into_any()
                    } else {
                        view! { <span></span> }.into_any()
                    }}

                    // Status pills + actions row
                    <div class="flex flex-wrap items-center gap-2 mb-3">
                        {if is_wanted && !is_acquired {
                            view! { <span class="pill" style="background:rgba(245,158,11,.12);color:#f59e0b">"Wanted"</span> }.into_any()
                        } else if is_acquired {
                            view! { <span class="pill" style="background:rgba(34,197,94,.12);color:#22c55e">"Acquired"</span> }.into_any()
                        } else {
                            view! { <span class="pill">"Not Wanted"</span> }.into_any()
                        }}
                        {if is_monitored {
                            view! { <span class="pill" style="background:rgba(59,130,246,.12);color:#3b82f6">"Monitored"</span> }.into_any()
                        } else {
                            view! { <span class="pill">"Unmonitored"</span> }.into_any()
                        }}
                        <span class=status_pill_class>{status_pill_text}</span>
                        {match &job_quality {
                            Some(q) => view! { <span class="pill d7-pill-muted">{q.clone()}</span> }.into_any(),
                            None => view! { <span></span> }.into_any(),
                        }}
                    </div>

                    // Error message if job failed
                    {match &job_error {
                        Some(err) => view! {
                            <div class="mb-3 text-sm text-red-600 dark:text-red-400 bg-red-500/[.06] dark:bg-red-500/10 border border-red-500/20 rounded-lg px-3 py-2">
                                {err.clone()}
                            </div>
                        }.into_any(),
                        None => view! { <span></span> }.into_any(),
                    }}

                    // Action buttons
                    <div class="flex flex-wrap gap-1.5">
                        {if can_retry {
                            let aid = album_id_retry.clone();
                            view! {
                                <button type="button"
                                    class=move || btn_cls(BTN_PRIMARY, "px-3 py-1.5 text-xs", download_loading.get())
                                    disabled=move || download_loading.get()
                                    on:click={
                                        let aid = aid.clone();
                                        move |_| {
                                            dispatch_with_toast_loading(ServerAction::RetryDownload { album_id: aid.clone() }, "Download requeued", Some(download_loading));
                                        }
                                    }>
                                    {move || if download_loading.get() { "Retrying\u{2026}" } else { "Retry Download" }}
                                </button>
                            }.into_any()
                        } else if can_download {
                            let aid = album_id_retry2.clone();
                            view! {
                                <button type="button"
                                    class=move || btn_cls(BTN_PRIMARY, "px-3 py-1.5 text-xs", download_loading.get())
                                    disabled=move || download_loading.get()
                                    on:click={
                                        let aid = aid.clone();
                                        move |_| {
                                            dispatch_with_toast_loading(ServerAction::RetryDownload { album_id: aid.clone() }, "Download started", Some(download_loading));
                                        }
                                    }>
                                    {move || if download_loading.get() { "Starting\u{2026}" } else { "Download" }}
                                </button>
                            }.into_any()
                        } else {
                            view! { <span></span> }.into_any()
                        }}

                        <button type="button" class={cls(BTN, "px-3 py-1.5 text-xs")} title=monitor_title
                            on:click={
                                let aid = album_id_monitor.clone();
                                move |_| {
                                    let next = !is_monitored;
                                    let msg = if next { "Album monitored" } else { "Album unmonitored" };
                                    dispatch_with_toast(ServerAction::ToggleAlbumMonitor { album_id: aid.clone(), monitored: next }, msg);
                                }
                            }>{monitor_label}</button>

                        {if is_acquired {
                            view! {
                                <button type="button" class={cls(BTN_DANGER, "px-3 py-1.5 text-xs")} title="Delete downloaded files"
                                    on:click=move |_| {
                                        show_remove_files.set(true);
                                    }>
                                    "Remove Files"
                                </button>
                            }.into_any()
                        } else {
                            view! { <span></span> }.into_any()
                        }}
                    </div>
                </div>
            </div>

            // ── Tracklist card ───────────────────────────────
            <div class=GLASS>
                <div class=GLASS_HEADER>
                    <h2 class=GLASS_TITLE>"Tracklist"</h2>
                    <span class={cls(MUTED, "text-xs")}>{format!("{track_count} tracks \u{00b7} {duration_display}")}</span>
                </div>
                {if tracks.is_empty() {
                    view! {
                        <div class="text-center py-10 px-4 text-zinc-400 dark:text-zinc-600 text-sm">"No tracks available."</div>
                    }.into_any()
                } else {
                    view! {
                        <div class={cls(GLASS_BODY, "p-0!")}>
                            <div class="divide-y divide-black/[.04] dark:divide-white/[.04]">
                                {
                                    let has_multiple_discs = tracks.iter().any(|t| t.disc_number > 1);
                                    tracks.iter().map(|t| {
                                        let num = t.track_number;
                                        let disc = t.disc_number;
                                        let title = t.title.clone();
                                        let version = t.version.clone();
                                        let dur = t.duration_display.clone();
                                        let explicit = t.explicit;
                                        let isrc = t.isrc.clone();
                                        let track_num_display = if has_multiple_discs {
                                            format!("{disc}-{num}")
                                        } else {
                                            num.to_string()
                                        };
                                        view! {
                                            <div class="flex items-center gap-3 px-5 py-2.5 transition-colors duration-100 hover:bg-blue-500/[.03] dark:hover:bg-blue-500/[.05]">
                                                <span class="w-8 text-right text-xs tabular-nums text-zinc-400 dark:text-zinc-500 shrink-0">{track_num_display}</span>
                                                <div class="flex-1 min-w-0">
                                                    <div class="flex items-center gap-1.5 truncate">
                                                        <span class="text-sm text-zinc-800 dark:text-zinc-200 truncate">{title}</span>
                                                        {match version {
                                                            Some(v) if !v.is_empty() => view! {
                                                                <span class="text-xs text-zinc-400 dark:text-zinc-500 shrink-0">{format!("({v})")}</span>
                                                            }.into_any(),
                                                            _ => view! { <span></span> }.into_any(),
                                                        }}
                                                        {if explicit {
                                                            view! {
                                                                <span class="inline-flex items-center justify-center px-1 py-px text-[9px] font-bold leading-none tracking-wide uppercase rounded bg-zinc-200 text-zinc-500 dark:bg-zinc-700 dark:text-zinc-400 shrink-0">"E"</span>
                                                            }.into_any()
                                                        } else {
                                                            view! { <span></span> }.into_any()
                                                        }}
                                                    </div>
                                                    {match isrc {
                                                        Some(code) if !code.is_empty() => view! {
                                                            <span class="block text-[10px] font-mono text-zinc-400/70 dark:text-zinc-600 leading-tight mt-0.5">{code}</span>
                                                        }.into_any(),
                                                        _ => view! { <span></span> }.into_any(),
                                                    }}
                                                </div>
                                                <span class="text-xs tabular-nums text-zinc-400 dark:text-zinc-500 shrink-0">{dur}</span>
                                            </div>
                                        }
                                    }).collect_view()
                                }
                            </div>
                        </div>
                    }.into_any()
                }}
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
            on_confirm={
                let aid = album_id_remove_confirm.clone();
                move |unmonitor: bool| {
                    let msg = if unmonitor { "Album files removed and unmonitored" } else { "Album files removed" };
                    dispatch_with_toast(ServerAction::RemoveAlbumFiles { album_id: aid.clone(), unmonitor }, msg);
                }
            }
        />
    }
}
