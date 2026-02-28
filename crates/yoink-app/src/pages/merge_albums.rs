use leptos::prelude::*;
use lucide_leptos::{ChevronRight, GitMerge, X};

use yoink_shared::{
    MonitoredAlbum, MonitoredArtist, ProviderLink, ServerAction, TrackInfo, provider_display_name,
};

use crate::components::toast::dispatch_with_toast;
use crate::components::{ErrorPanel, MobileMenuButton, Sidebar};
use crate::hooks::{set_page_title, use_sse_version};
use crate::styles::{
    BTN_DANGER, BTN_PRIMARY, BREADCRUMB_CURRENT, BREADCRUMB_LINK, BREADCRUMB_NAV, BREADCRUMB_SEP,
    GLASS, GLASS_BODY, GLASS_HEADER, GLASS_TITLE, HEADER_BAR, MUTED, SELECT, cls,
};

// ── Data structures ─────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MergeCandidate {
    pub album_a: MonitoredAlbum,
    pub album_b: MonitoredAlbum,
    pub links_a: Vec<ProviderLink>,
    pub links_b: Vec<ProviderLink>,
    pub tracks_a: Vec<TrackInfo>,
    pub tracks_b: Vec<TrackInfo>,
    pub merged_tracks: Vec<TrackInfo>,
    pub merged_links: Vec<ProviderLink>,
    pub match_kind: String,
    pub confidence: u8,
    pub explanation: Option<String>,
    /// All suggestion IDs that contributed to this candidate (for dismiss).
    pub suggestion_ids: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MergeAlbumsData {
    pub artist: Option<MonitoredArtist>,
    pub candidates: Vec<MergeCandidate>,
}

// ── Server function ─────────────────────────────────────────

#[server(GetMergeAlbumsData, "/leptos")]
pub async fn get_merge_albums_data(artist_id: String) -> Result<MergeAlbumsData, ServerFnError> {
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

    let mut album_by_id: std::collections::HashMap<String, MonitoredAlbum> =
        std::collections::HashMap::new();
    let mut links_by_album: std::collections::HashMap<String, Vec<ProviderLink>> =
        std::collections::HashMap::new();
    let mut pair_to_album: std::collections::HashMap<(String, String), String> =
        std::collections::HashMap::new();
    let mut tracks_by_album: std::collections::HashMap<String, Vec<TrackInfo>> =
        std::collections::HashMap::new();

    for album in &albums {
        album_by_id.insert(album.id.clone(), album.clone());
        let links = (ctx.fetch_album_links)(album.id.clone())
            .await
            .unwrap_or_default();
        for link in &links {
            pair_to_album.insert(
                (link.provider.clone(), link.external_id.clone()),
                album.id.clone(),
            );
        }
        links_by_album.insert(album.id.clone(), links);
        tracks_by_album.insert(
            album.id.clone(),
            (ctx.fetch_tracks)(album.id.clone()).await.unwrap_or_default(),
        );
    }

    // Deduplicate by ordered album-pair (regardless of which suggestion triggered it).
    let mut dedupe: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::new();
    // Collect suggestion IDs per album pair for dismiss support.
    let mut suggestion_ids_map: std::collections::HashMap<(String, String), Vec<String>> =
        std::collections::HashMap::new();
    // Store first-seen metadata per pair.
    struct PairMeta {
        match_kind: String,
        confidence: u8,
        explanation: Option<String>,
    }
    let mut pair_meta: std::collections::HashMap<(String, String), PairMeta> =
        std::collections::HashMap::new();

    for album in &albums {
        let suggestions = (ctx.fetch_album_match_suggestions)(album.id.clone())
            .await
            .unwrap_or_default();

        for s in suggestions.into_iter().filter(|s| s.status == "pending") {
            let pair = (s.right_provider.clone(), s.right_external_id.clone());
            let Some(other_album_id) = pair_to_album.get(&pair).cloned() else {
                continue;
            };
            if other_album_id == album.id {
                continue;
            }

            let key = if album.id < other_album_id {
                (album.id.clone(), other_album_id.clone())
            } else {
                (other_album_id.clone(), album.id.clone())
            };

            suggestion_ids_map
                .entry(key.clone())
                .or_default()
                .push(s.id.clone());

            pair_meta.entry(key.clone()).or_insert(PairMeta {
                match_kind: s.match_kind.clone(),
                confidence: s.confidence,
                explanation: s.explanation.clone(),
            });

            dedupe.insert(key);
        }
    }

    let mut candidates = Vec::new();

    for key in &dedupe {
        let Some(a) = album_by_id.get(&key.0).cloned() else { continue };
        let Some(b) = album_by_id.get(&key.1).cloned() else { continue };
        let a_links = links_by_album.get(&a.id).cloned().unwrap_or_default();
        let b_links = links_by_album.get(&b.id).cloned().unwrap_or_default();
        let a_tracks = tracks_by_album.get(&a.id).cloned().unwrap_or_default();
        let b_tracks = tracks_by_album.get(&b.id).cloned().unwrap_or_default();
        let merged_tracks = build_merged_track_preview(&a_tracks, &b_tracks);
        let merged_links = build_merged_links_preview(&a_links, &b_links);
        let meta = pair_meta.remove(key).unwrap();
        let suggestion_ids = suggestion_ids_map.remove(key).unwrap_or_default();

        candidates.push(MergeCandidate {
            album_a: a,
            album_b: b,
            links_a: a_links,
            links_b: b_links,
            tracks_a: a_tracks,
            tracks_b: b_tracks,
            merged_tracks,
            merged_links,
            match_kind: meta.match_kind,
            confidence: meta.confidence,
            explanation: meta.explanation,
            suggestion_ids,
        });
    }

    candidates.sort_by(|a, b| {
        b.confidence
            .cmp(&a.confidence)
            .then_with(|| a.album_a.title.cmp(&b.album_a.title))
    });

    Ok(MergeAlbumsData { artist, candidates })
}

#[cfg(feature = "ssr")]
fn build_merged_track_preview(target: &[TrackInfo], source: &[TrackInfo]) -> Vec<TrackInfo> {
    let mut by_key: std::collections::HashMap<String, TrackInfo> = std::collections::HashMap::new();

    let make_key = |t: &TrackInfo| {
        if let Some(isrc) = &t.isrc
            && !isrc.trim().is_empty()
        {
            return format!("isrc:{}", isrc.trim().to_ascii_uppercase());
        }
        format!(
            "{}:{}:{}",
            t.disc_number,
            t.track_number,
            t.title.to_ascii_lowercase()
        )
    };

    for t in target {
        by_key.insert(make_key(t), t.clone());
    }
    for t in source {
        let key = make_key(t);
        by_key.entry(key).or_insert_with(|| t.clone());
    }

    let mut merged: Vec<TrackInfo> = by_key.into_values().collect();
    merged.sort_by(|a, b| {
        a.disc_number
            .cmp(&b.disc_number)
            .then_with(|| a.track_number.cmp(&b.track_number))
            .then_with(|| a.title.cmp(&b.title))
    });
    merged
}

#[cfg(feature = "ssr")]
fn build_merged_links_preview(target: &[ProviderLink], source: &[ProviderLink]) -> Vec<ProviderLink> {
    let mut map: std::collections::HashMap<(String, String), ProviderLink> =
        std::collections::HashMap::new();

    for l in target {
        map.insert((l.provider.clone(), l.external_id.clone()), l.clone());
    }
    for l in source {
        map.entry((l.provider.clone(), l.external_id.clone()))
            .or_insert_with(|| l.clone());
    }

    let mut out: Vec<ProviderLink> = map.into_values().collect();
    out.sort_by(|a, b| a.provider.cmp(&b.provider).then_with(|| a.external_id.cmp(&b.external_id)));
    out
}

// ── UI helpers ──────────────────────────────────────────────

/// Confidence badge with color coding.
fn confidence_class(confidence: u8) -> &'static str {
    if confidence >= 80 {
        "text-emerald-700 dark:text-emerald-300 bg-emerald-500/10 border-emerald-500/20"
    } else if confidence >= 50 {
        "text-amber-700 dark:text-amber-300 bg-amber-500/10 border-amber-500/20"
    } else {
        "text-red-700 dark:text-red-300 bg-red-500/10 border-red-500/20"
    }
}

fn match_kind_label(kind: &str) -> &str {
    match kind {
        "isrc_exact" => "ISRC match",
        "title_fuzzy" => "Fuzzy title",
        _ => kind,
    }
}

/// Render a compact provider badge.
#[component]
fn ProviderBadge(link: ProviderLink) -> impl IntoView {
    let label = provider_display_name(&link.provider);
    match link.external_url {
        Some(url) => view! {
            <a
                href=url
                target="_blank"
                rel="noreferrer"
                class="inline-flex items-center px-1.5 py-px text-[10px] font-medium text-blue-600 dark:text-blue-400 bg-blue-500/[.08] border border-blue-500/20 rounded no-underline hover:bg-blue-500/15"
                title=link.external_id
            >
                {label}
            </a>
        }.into_any(),
        None => view! {
            <span class="inline-flex items-center px-1.5 py-px text-[10px] font-medium text-zinc-500 dark:text-zinc-400 bg-zinc-500/[.06] border border-zinc-500/10 rounded" title=link.external_id>
                {label}
            </span>
        }.into_any(),
    }
}

/// Compact album card used in the side-by-side comparison.
#[component]
fn AlbumCard(
    album: MonitoredAlbum,
    links: Vec<ProviderLink>,
    track_count: usize,
) -> impl IntoView {
    view! {
        <div class="flex items-start gap-3 min-w-0">
            {match album.cover_url.clone() {
                Some(url) => view! {
                    <img src=url alt="" class="size-14 rounded-lg object-cover border border-black/[.06] dark:border-white/[.08] shrink-0 bg-zinc-200 dark:bg-zinc-800" />
                }.into_any(),
                None => view! {
                    <div class="size-14 rounded-lg inline-flex items-center justify-center border border-black/[.06] dark:border-white/[.08] bg-zinc-200 dark:bg-zinc-800 text-zinc-400 dark:text-zinc-500 text-[10px] shrink-0">"No Art"</div>
                }.into_any(),
            }}
            <div class="min-w-0 flex-1">
                <div class="text-sm font-semibold text-zinc-900 dark:text-zinc-100 truncate">{album.title}</div>
                {album.release_date.map(|d| view! {
                    <div class="text-xs text-zinc-500 dark:text-zinc-400">{d}</div>
                })}
                <div class="text-xs text-zinc-500 dark:text-zinc-400 mt-0.5">
                    {format!("{} tracks", track_count)}
                    {if album.acquired { " · acquired" } else { "" }}
                    {if album.monitored { " · monitored" } else { "" }}
                </div>
                <div class="flex flex-wrap gap-1 mt-1.5">
                    {links.into_iter().map(|link| view! { <ProviderBadge link=link /> }).collect_view()}
                </div>
            </div>
        </div>
    }
}

// ── Merge card component ────────────────────────────────────

#[component]
fn MergeCandidateCard(candidate: MergeCandidate) -> impl IntoView {
    let conf_cls = confidence_class(candidate.confidence);
    let kind_label = match_kind_label(&candidate.match_kind).to_string();

    // Title options for the dropdown.
    let title_a = candidate.album_a.title.clone();
    let title_b = candidate.album_b.title.clone();
    let titles_differ = title_a != title_b;

    // Cover options for the dropdown.
    let cover_a = candidate.album_a.cover_url.clone();
    let cover_b = candidate.album_b.cover_url.clone();

    // Signals for user selection — default to album A's metadata.
    let (selected_title, set_selected_title) = signal(title_a.clone());
    let (selected_cover, set_selected_cover) = signal(cover_a.clone());

    // IDs for the merge action.
    let id_a = candidate.album_a.id.clone();
    let id_b = candidate.album_b.id.clone();
    let merge_target_id = id_a.clone();
    let merge_source_id = id_b.clone();
    let suggestion_ids = candidate.suggestion_ids.clone();

    // Summary stats.
    let merged_link_count = candidate.merged_links.len();
    let merged_track_count = candidate.merged_tracks.len();

    view! {
        <div class={cls(GLASS, "overflow-hidden")}>
            // ── Header: confidence + match kind ────────────────
            <div class=GLASS_HEADER>
                <div class="flex items-center gap-2">
                    <span class={format!("inline-flex items-center px-2 py-0.5 text-xs font-semibold border rounded-md {}", conf_cls)}>
                        {format!("{}%", candidate.confidence)}
                    </span>
                    <span class="text-sm font-medium text-zinc-900 dark:text-zinc-100">
                        {kind_label}
                    </span>
                    {candidate.explanation.map(|e| view! {
                        <span class="text-xs text-zinc-500 dark:text-zinc-400">{format!("— {}", e)}</span>
                    })}
                </div>
            </div>

            <div class={cls(GLASS_BODY, "space-y-4")}>
                // ── Side-by-side album cards ───────────────────
                <div class="grid grid-cols-1 md:grid-cols-[1fr_auto_1fr] gap-3 items-stretch">
                    <div class="p-3 rounded-lg border border-black/[.04] dark:border-white/[.04] bg-zinc-50/50 dark:bg-zinc-900/30">
                        <AlbumCard
                            album=candidate.album_a
                            links=candidate.links_a
                            track_count=candidate.tracks_a.len()
                        />
                    </div>
                    <div class="hidden md:flex items-center justify-center text-zinc-400 dark:text-zinc-500 text-lg font-light select-none">
                        "+"
                    </div>
                    <div class="p-3 rounded-lg border border-black/[.04] dark:border-white/[.04] bg-zinc-50/50 dark:bg-zinc-900/30">
                        <AlbumCard
                            album=candidate.album_b
                            links=candidate.links_b
                            track_count=candidate.tracks_b.len()
                        />
                    </div>
                </div>

                // ── Merge result configuration ─────────────────
                <div class="p-3 rounded-lg border border-blue-500/15 bg-blue-500/[.04]">
                    <div class="flex flex-wrap items-center gap-x-4 gap-y-2">
                        // Title selector
                        {if titles_differ {
                            let ta = title_a.clone();
                            let tb = title_b.clone();
                            view! {
                                <label class="flex items-center gap-2 text-xs text-zinc-600 dark:text-zinc-300">
                                    <span class="font-medium">"Title:"</span>
                                    <select
                                        class=SELECT
                                        on:change=move |ev| {
                                            use leptos::prelude::event_target_value;
                                            set_selected_title.set(event_target_value(&ev));
                                        }
                                    >
                                        <option value={ta.clone()} selected=true>{ta.clone()}</option>
                                        <option value={tb.clone()}>{tb.clone()}</option>
                                    </select>
                                </label>
                            }.into_any()
                        } else {
                            view! {
                                <span class="text-xs text-zinc-600 dark:text-zinc-300">
                                    <span class="font-medium">"Title: "</span>
                                    {title_a.clone()}
                                </span>
                            }.into_any()
                        }}

                        // Cover selector (only when both have covers and they differ)
                        {if cover_a.is_some() && cover_b.is_some() && cover_a != cover_b {
                            let ca = cover_a.clone().unwrap();
                            let cb = cover_b.clone().unwrap();
                            let ca2 = ca.clone();
                            let cb2 = cb.clone();
                            view! {
                                <label class="flex items-center gap-2 text-xs text-zinc-600 dark:text-zinc-300">
                                    <span class="font-medium">"Cover:"</span>
                                    <select
                                        class=SELECT
                                        on:change=move |ev| {
                                            use leptos::prelude::event_target_value;
                                            let val = event_target_value(&ev);
                                            set_selected_cover.set(if val == "b" { Some(cb2.clone()) } else { Some(ca2.clone()) });
                                        }
                                    >
                                        <option value="a" selected=true>"Album 1 cover"</option>
                                        <option value="b">"Album 2 cover"</option>
                                    </select>
                                </label>
                            }.into_any()
                        } else {
                            view! { <span></span> }.into_any()
                        }}

                        // Summary
                        <span class="text-xs text-zinc-500 dark:text-zinc-400">
                            {format!("{} provider links · {} tracks after merge", merged_link_count, merged_track_count)}
                        </span>

                        // Merged provider badges
                        <div class="flex flex-wrap gap-1">
                            {candidate.merged_links.into_iter().map(|link| view! { <ProviderBadge link=link /> }).collect_view()}
                        </div>
                    </div>
                </div>

                // ── Collapsible track list ─────────────────────
                <details class="group">
                    <summary class="cursor-pointer text-xs font-medium text-zinc-500 dark:text-zinc-400 hover:text-zinc-700 dark:hover:text-zinc-200 select-none transition-colors">
                        {format!("Show merged track list ({} tracks)", merged_track_count)}
                    </summary>
                    <div class="mt-2 grid grid-cols-1 sm:grid-cols-2 gap-x-4 gap-y-0.5 text-xs text-zinc-600 dark:text-zinc-300 max-h-56 overflow-auto">
                        {candidate.merged_tracks.into_iter().map(|t| view! {
                            <div class="truncate">{
                            if let Some(ref v) = t.version {
                                if !v.is_empty() {
                                    format!("{}-{:02}. {} ({})", t.disc_number, t.track_number, t.title, v)
                                } else {
                                    format!("{}-{:02}. {}", t.disc_number, t.track_number, t.title)
                                }
                            } else {
                                format!("{}-{:02}. {}", t.disc_number, t.track_number, t.title)
                            }
                        }</div>
                        }).collect_view()}
                    </div>
                </details>

                // ── Actions ────────────────────────────────────
                <div class="flex items-center justify-end gap-2 pt-1 border-t border-black/[.04] dark:border-white/[.04]">
                    // Dismiss all related suggestions
                    {
                        let sids = suggestion_ids.clone();
                        view! {
                            <button
                                type="button"
                                class={cls(BTN_DANGER, "px-3 py-1 text-xs")}
                                on:click=move |_| {
                                    for sid in &sids {
                                        dispatch_with_toast(
                                            ServerAction::DismissMatchSuggestion { suggestion_id: sid.clone() },
                                            "Match dismissed",
                                        );
                                    }
                                }
                            >
                                <X size=12 />
                                "Not a match"
                            </button>
                        }
                    }
                    // Merge
                    {
                        let target_id = merge_target_id;
                        let source_id = merge_source_id;
                        view! {
                            <button
                                type="button"
                                class={cls(BTN_PRIMARY, "px-3 py-1 text-xs")}
                                on:click=move |_| {
                                    let title = selected_title.get();
                                    let cover = selected_cover.get();
                                    dispatch_with_toast(
                                        ServerAction::MergeAlbums {
                                            target_album_id: target_id.clone(),
                                            source_album_id: source_id.clone(),
                                            result_title: Some(title),
                                            result_cover_url: cover,
                                        },
                                        "Albums merged",
                                    );
                                }
                            >
                                <GitMerge size=12 />
                                "Merge"
                            </button>
                        }
                    }
                </div>
            </div>
        </div>
    }
}

// ── Page component ──────────────────────────────────────────

#[component]
pub fn MergeAlbumsPage() -> impl IntoView {
    set_page_title("Merge Albums");

    let params = leptos_router::hooks::use_params_map();
    let artist_id = move || params.read().get("id").unwrap_or_default();
    let version = use_sse_version();

    let data = Resource::new(
        move || (artist_id(), version.get()),
        |(id, _)| get_merge_albums_data(id),
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
                                <span class=BREADCRUMB_CURRENT>"Merge Albums"</span>
                            </nav>
                        </div>
                        <div class="p-6 max-md:p-4">
                            // Skeleton intro card
                            <div class="mb-5 bg-white/70 dark:bg-zinc-800/60 rounded-xl border border-black/[.06] dark:border-white/[.08] overflow-hidden">
                                <div class="px-5 py-3.5 border-b border-black/[.06] dark:border-white/[.06]">
                                    <div class="h-4 w-32 bg-zinc-200 dark:bg-zinc-700 rounded animate-pulse"></div>
                                </div>
                                <div class="px-5 py-4">
                                    <div class="h-3.5 w-full max-w-md bg-zinc-200 dark:bg-zinc-700 rounded animate-pulse"></div>
                                </div>
                            </div>
                            // Skeleton candidate cards
                            {(0..2).map(|_| view! {
                                <div class="mb-4 bg-white/70 dark:bg-zinc-800/60 rounded-xl border border-black/[.06] dark:border-white/[.08] overflow-hidden animate-pulse">
                                    <div class="px-5 py-3.5 border-b border-black/[.06] dark:border-white/[.06] flex items-center gap-2">
                                        <div class="h-5 w-12 bg-zinc-200 dark:bg-zinc-700 rounded"></div>
                                        <div class="h-4 w-24 bg-zinc-200 dark:bg-zinc-700 rounded"></div>
                                    </div>
                                    <div class="px-5 py-4">
                                        <div class="grid grid-cols-1 md:grid-cols-[1fr_auto_1fr] gap-3">
                                            <div class="p-3 rounded-lg border border-black/[.04] dark:border-white/[.04]">
                                                <div class="flex items-start gap-3">
                                                    <div class="size-14 rounded-lg bg-zinc-200 dark:bg-zinc-700 shrink-0"></div>
                                                    <div class="flex-1">
                                                        <div class="h-4 w-28 bg-zinc-200 dark:bg-zinc-700 rounded mb-2"></div>
                                                        <div class="h-3 w-16 bg-zinc-200 dark:bg-zinc-700 rounded"></div>
                                                    </div>
                                                </div>
                                            </div>
                                            <div class="hidden md:flex items-center justify-center text-zinc-300 dark:text-zinc-600">"+"</div>
                                            <div class="p-3 rounded-lg border border-black/[.04] dark:border-white/[.04]">
                                                <div class="flex items-start gap-3">
                                                    <div class="size-14 rounded-lg bg-zinc-200 dark:bg-zinc-700 shrink-0"></div>
                                                    <div class="flex-1">
                                                        <div class="h-4 w-28 bg-zinc-200 dark:bg-zinc-700 rounded mb-2"></div>
                                                        <div class="h-3 w-16 bg-zinc-200 dark:bg-zinc-700 rounded"></div>
                                                    </div>
                                                </div>
                                            </div>
                                        </div>
                                    </div>
                                </div>
                            }).collect_view()}
                        </div>
                    </div>
                }>
                    {move || data.get().map(|result| match result {
                        Err(e) => view! {
                            <div>
                                <div class=HEADER_BAR>
                                    <nav class=BREADCRUMB_NAV aria-label="Breadcrumb"><MobileMenuButton />
                                        <a href="/artists" class=BREADCRUMB_LINK>"Artists"</a>
                                        <span class=BREADCRUMB_SEP><ChevronRight /></span>
                                        <span class=BREADCRUMB_CURRENT>"Merge Albums"</span>
                                    </nav>
                                </div>
                                <div class="p-6 max-md:p-4">
                                    <ErrorPanel message="Failed to load merge candidates." details=e.to_string() />
                                </div>
                            </div>
                        }.into_any(),
                        Ok(data) => {
                            let artist_name = data
                                .artist
                                .as_ref()
                                .map(|a| a.name.clone())
                                .unwrap_or_else(|| "Artist".to_string());
                            let artist_id_back = data
                                .artist
                                .as_ref()
                                .map(|a| a.id.clone())
                                .unwrap_or_default();
                            let artist_link = format!("/artists/{}", artist_id_back);
                            let count = data.candidates.len();

                            view! {
                                // Sticky header with breadcrumb
                                <div class=HEADER_BAR>
                                    <nav class=BREADCRUMB_NAV aria-label="Breadcrumb"><MobileMenuButton />
                                        <a href="/artists" class=BREADCRUMB_LINK>"Artists"</a>
                                        <span class=BREADCRUMB_SEP><ChevronRight /></span>
                                        <a href=artist_link class=BREADCRUMB_LINK>{artist_name.clone()}</a>
                                        <span class=BREADCRUMB_SEP><ChevronRight /></span>
                                        <span class=BREADCRUMB_CURRENT>"Merge Albums"</span>
                                    </nav>
                                </div>

                                <div class="p-6 max-md:p-4">
                                    // Intro card
                                    <div class={cls(GLASS, "mb-5")}>
                                        <div class=GLASS_HEADER>
                                            <h2 class=GLASS_TITLE>"Merge Albums"</h2>
                                            {if count > 0 {
                                                view! {
                                                    <span class={cls(MUTED, "text-xs")}>
                                                        {format!("{} candidate{}", count, if count == 1 { "" } else { "s" })}
                                                    </span>
                                                }.into_any()
                                            } else {
                                                view! { <span></span> }.into_any()
                                            }}
                                        </div>
                                        <div class=GLASS_BODY>
                                            <p class={cls(MUTED, "text-sm m-0")}>
                                                "Albums that appear to be the same release across different providers. Review and merge them into a single entry, or dismiss false matches."
                                            </p>
                                        </div>
                                    </div>

                                    {if data.candidates.is_empty() {
                                        view! {
                                            <div class={cls(GLASS, "p-8 text-center text-sm text-zinc-400 dark:text-zinc-500")}>
                                                "No duplicate albums detected \u{2014} nothing to merge."
                                            </div>
                                        }.into_any()
                                    } else {
                                        view! {
                                            <div class="flex flex-col gap-4">
                                                {data.candidates.into_iter().map(|candidate| {
                                                    view! { <MergeCandidateCard candidate=candidate /> }
                                                }).collect_view()}
                                            </div>
                                        }.into_any()
                                    }}
                                </div>
                            }.into_any()
                        }
                    })}
                </Transition>
            </div>
        </div>
    }
}
