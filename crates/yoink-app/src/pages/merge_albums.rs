use leptos::prelude::*;
use lucide_leptos::ArrowLeft;

use yoink_shared::{
    MonitoredAlbum, MonitoredArtist, ProviderLink, ServerAction, TrackInfo, provider_display_name,
};

use crate::components::toast::dispatch_with_toast;
use crate::components::{ErrorPanel, Sidebar};
use crate::hooks::{set_page_title, use_sse_version};
use crate::styles::{BTN, BTN_PRIMARY, GLASS, GLASS_BODY, GLASS_HEADER, GLASS_TITLE, cls};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MergeCandidate {
    pub target_album: MonitoredAlbum,
    pub source_album: MonitoredAlbum,
    pub target_links: Vec<ProviderLink>,
    pub source_links: Vec<ProviderLink>,
    pub match_provider: String,
    pub match_external_id: String,
    pub confidence: u8,
    pub explanation: Option<String>,
    pub target_tracks: Vec<TrackInfo>,
    pub source_tracks: Vec<TrackInfo>,
    pub merged_tracks: Vec<TrackInfo>,
    pub result_title: String,
    pub result_cover_url: Option<String>,
    pub result_links: Vec<ProviderLink>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MergeAlbumsData {
    pub artist: Option<MonitoredArtist>,
    pub candidates: Vec<MergeCandidate>,
}

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

    let mut dedupe: std::collections::HashSet<(String, String, String, String)> =
        std::collections::HashSet::new();
    let mut candidates = Vec::new();

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
                (
                    album.id.clone(),
                    other_album_id.clone(),
                    pair.0.clone(),
                    pair.1.clone(),
                )
            } else {
                (
                    other_album_id.clone(),
                    album.id.clone(),
                    pair.0.clone(),
                    pair.1.clone(),
                )
            };
            if !dedupe.insert(key) {
                continue;
            }

            let Some(a) = album_by_id.get(&album.id).cloned() else {
                continue;
            };
            let Some(b) = album_by_id.get(&other_album_id).cloned() else {
                continue;
            };
            let a_links = links_by_album.get(&a.id).cloned().unwrap_or_default();
            let b_links = links_by_album.get(&b.id).cloned().unwrap_or_default();

            let score = |alb: &MonitoredAlbum, links: &[ProviderLink]| -> i32 {
                let mut score = (links.len() as i32) * 10;
                if alb.acquired {
                    score += 5;
                }
                if alb.monitored {
                    score += 2;
                }
                score
            };

            let (target_album, source_album, target_links, source_links) =
                if score(&a, &a_links) >= score(&b, &b_links) {
                    (a, b, a_links, b_links)
                } else {
                    (b, a, b_links, a_links)
                };

            let target_tracks = tracks_by_album
                .get(&target_album.id)
                .cloned()
                .unwrap_or_default();
            let source_tracks = tracks_by_album
                .get(&source_album.id)
                .cloned()
                .unwrap_or_default();
            let merged_tracks = build_merged_track_preview(&target_tracks, &source_tracks);
            let result_links = build_merged_links_preview(&target_links, &source_links);

            candidates.push(MergeCandidate {
                result_title: target_album.title.clone(),
                result_cover_url: target_album.cover_url.clone(),
                target_album,
                source_album,
                target_links,
                source_links,
                result_links,
                match_provider: pair.0,
                match_external_id: pair.1,
                confidence: s.confidence,
                explanation: s.explanation,
                target_tracks,
                source_tracks,
                merged_tracks,
            });
        }
    }

    candidates.sort_by(|a, b| {
        b.confidence
            .cmp(&a.confidence)
            .then_with(|| a.target_album.title.cmp(&b.target_album.title))
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
            <div class="ml-[220px] max-md:ml-0 flex-1 min-h-screen p-6 max-md:p-4">
                <Transition fallback=move || view! { <div class="text-zinc-500">"Loading merge candidates..."</div> }>
                    {move || data.get().map(|result| match result {
                        Err(e) => view! {
                            <ErrorPanel message="Failed to load merge candidates." details=e.to_string() />
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

                            view! {
                                <div class={cls(GLASS, "mb-5")}>
                                    <div class=GLASS_HEADER>
                                        <h2 class=GLASS_TITLE>{format!("Merge Albums from Potential Matches · {}", artist_name)}</h2>
                                        <a href=format!("/artists/{}", artist_id_back) class={cls(BTN, "px-2.5 py-0.5 text-xs no-underline inline-flex items-center gap-1.5") }>
                                            <ArrowLeft size=14 />
                                            "Back to Artist"
                                        </a>
                                    </div>
                                    <div class=GLASS_BODY>
                                        <p class="text-sm text-zinc-500 dark:text-zinc-400 m-0">
                                            "These rows come from album potential-match conflicts where the suggested provider ID is already linked to another local album."
                                        </p>
                                    </div>
                                </div>

                                {if data.candidates.is_empty() {
                                    view! {
                                        <div class={cls(GLASS, "p-5 text-sm text-zinc-500 dark:text-zinc-400") }>
                                            "No merge conflicts from potential matches."
                                        </div>
                                    }.into_any()
                                } else {
                                    view! {
                                        <div class="flex flex-col gap-4">
                                            {data.candidates.into_iter().map(|candidate| {
                                                let target_id = candidate.target_album.id.clone();
                                                let source_id = candidate.source_album.id.clone();
                                                view! {
                                                    <div class={cls(GLASS, "overflow-hidden")}>
                                                        <div class=GLASS_HEADER>
                                                            <h3 class="text-sm font-semibold text-zinc-900 dark:text-zinc-100 m-0">
                                                                {format!("{} {}%", provider_display_name(&candidate.match_provider), candidate.confidence)}
                                                            </h3>
                                                            <span class="text-xs text-zinc-500 dark:text-zinc-400">
                                                                {candidate.explanation.unwrap_or_default()}
                                                            </span>
                                                        </div>
                                                            <div class={cls(GLASS_BODY, "space-y-2")}>
                                                            <div class="grid grid-cols-1 lg:grid-cols-2 gap-2">
                                                                <div class="p-2 rounded-lg border border-emerald-500/20 bg-emerald-500/[.06] min-w-0">
                                                                    <div class="text-xs uppercase tracking-wide text-emerald-700 dark:text-emerald-300 mb-1">"Source A (keep target)"</div>
                                                                    <div class="text-sm font-medium text-zinc-800 dark:text-zinc-200 truncate">{candidate.target_album.title.clone()}</div>
                                                                    <div class="text-xs text-zinc-500 dark:text-zinc-400 truncate">{candidate.target_album.id.clone()}</div>
                                                                    {match candidate.target_album.cover_url.clone() {
                                                                        Some(url) => view! {
                                                                            <img src=url alt="" class="size-10 mt-1 rounded object-cover border border-emerald-500/20 dark:border-emerald-500/30 bg-zinc-200 dark:bg-zinc-800" />
                                                                        }.into_any(),
                                                                        None => view! { <span></span> }.into_any(),
                                                                    }}
                                                                    <div class="flex flex-wrap gap-1 mt-1">
                                                                        {candidate.target_links.clone().into_iter().map(|link| {
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
                                                                        }).collect_view()}
                                                                    </div>
                                                                    <div class="mt-2 text-xs text-zinc-600 dark:text-zinc-300 space-y-0.5 max-h-44 overflow-auto">
                                                                        {candidate.target_tracks.clone().into_iter().map(|t| view! {
                                                                            <div class="truncate">{format!("{:02}. {}", t.track_number, t.title)}</div>
                                                                        }).collect_view()}
                                                                    </div>
                                                                </div>

                                                                <div class="p-2 rounded-lg border border-amber-500/20 bg-amber-500/[.06] min-w-0">
                                                                    <div class="text-xs uppercase tracking-wide text-amber-700 dark:text-amber-300 mb-1">"Source B (remove source)"</div>
                                                                    <div class="text-sm font-medium text-zinc-800 dark:text-zinc-200 truncate">{candidate.source_album.title.clone()}</div>
                                                                    <div class="text-xs text-zinc-500 dark:text-zinc-400 truncate">{candidate.source_album.id.clone()}</div>
                                                                    {match candidate.source_album.cover_url.clone() {
                                                                        Some(url) => view! {
                                                                            <img src=url alt="" class="size-10 mt-1 rounded object-cover border border-amber-500/20 dark:border-amber-500/30 bg-zinc-200 dark:bg-zinc-800" />
                                                                        }.into_any(),
                                                                        None => view! { <span></span> }.into_any(),
                                                                    }}
                                                                    <div class="flex flex-wrap gap-1 mt-1">
                                                                        {candidate.source_links.clone().into_iter().map(|link| {
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
                                                                        }).collect_view()}
                                                                    </div>
                                                                    <div class="mt-2 text-xs text-zinc-600 dark:text-zinc-300 space-y-0.5 max-h-44 overflow-auto">
                                                                        {candidate.source_tracks.clone().into_iter().map(|t| view! {
                                                                            <div class="truncate">{format!("{:02}. {}", t.track_number, t.title)}</div>
                                                                        }).collect_view()}
                                                                    </div>
                                                                </div>
                                                            </div>

                                                            <div class="p-2 rounded-lg border border-blue-500/20 bg-blue-500/[.06]">
                                                                <div class="text-xs uppercase tracking-wide text-blue-700 dark:text-blue-300 mb-1">"Merge Outcome"</div>
                                                                <div class="flex items-start gap-3">
                                                                    {match candidate.result_cover_url.clone() {
                                                                        Some(url) => view! {
                                                                            <img src=url alt="" class="size-14 rounded-md object-cover border border-blue-500/20 dark:border-blue-500/30 shrink-0 bg-zinc-200 dark:bg-zinc-800" />
                                                                        }.into_any(),
                                                                        None => view! {
                                                                            <div class="size-14 rounded-md inline-flex items-center justify-center border border-blue-500/20 dark:border-blue-500/30 bg-zinc-200 dark:bg-zinc-800 text-zinc-500 dark:text-zinc-400 text-xs shrink-0">"No Art"</div>
                                                                        }.into_any(),
                                                                    }}
                                                                    <div class="min-w-0 flex-1">
                                                                        <div class="text-sm font-semibold text-zinc-900 dark:text-zinc-100 truncate">{candidate.result_title.clone()}</div>
                                                                        <div class="text-xs text-zinc-500 dark:text-zinc-400 mt-0.5">"Outcome uses Source A title/artwork and combines provider links from Source A + Source B."</div>
                                                                        <div class="text-xs text-zinc-500 dark:text-zinc-400 mt-1">{format!("{} provider links · {} tracks after merge", candidate.result_links.len(), candidate.merged_tracks.len())}</div>
                                                                        <div class="flex flex-wrap gap-1 mt-1">
                                                                            {candidate.result_links.clone().into_iter().map(|link| {
                                                                                let label = provider_display_name(&link.provider);
                                                                                match link.external_url {
                                                                                    Some(url) => view! {
                                                                                        <a
                                                                                            href=url
                                                                                            target="_blank"
                                                                                            rel="noreferrer"
                                                                                            class="inline-flex items-center px-1.5 py-px text-[10px] font-medium text-blue-600 dark:text-blue-400 bg-blue-500/[.08] border border-blue-500/20 rounded no-underline hover:bg-blue-500/15"
                                                                                            title=link.external_id.clone()
                                                                                        >
                                                                                            {label}
                                                                                        </a>
                                                                                    }.into_any(),
                                                                                    None => view! {
                                                                                        <span class="inline-flex items-center px-1.5 py-px text-[10px] font-medium text-zinc-500 dark:text-zinc-400 bg-zinc-500/[.06] border border-zinc-500/10 rounded" title=link.external_id.clone()>{label}</span>
                                                                                    }.into_any(),
                                                                                }
                                                                            }).collect_view()}
                                                                        </div>
                                                                    </div>
                                                                </div>
                                                                <div class="mt-2 text-xs text-zinc-600 dark:text-zinc-300 space-y-0.5 max-h-56 overflow-auto">
                                                                    {candidate.merged_tracks.clone().into_iter().map(|t| view! {
                                                                        <div class="truncate">{format!("{}-{:02}. {}", t.disc_number, t.track_number, t.title)}</div>
                                                                    }).collect_view()}
                                                                </div>
                                                            </div>

                                                            <div class="flex justify-end">
                                                                <button
                                                                    type="button"
                                                                    class={cls(BTN_PRIMARY, "px-3 py-1 text-xs")}
                                                                    on:click=move |_| {
                                                                        dispatch_with_toast(
                                                                            ServerAction::MergeAlbums {
                                                                                target_album_id: target_id.clone(),
                                                                                source_album_id: source_id.clone(),
                                                                            },
                                                                            "Albums merged",
                                                                        );
                                                                    }
                                                                >
                                                                    "Merge Albums"
                                                                </button>
                                                            </div>
                                                        </div>
                                                    </div>
                                                }
                                            }).collect_view()}
                                        </div>
                                    }.into_any()
                                }}
                            }.into_any()
                        }
                    })}
                </Transition>
            </div>
        </div>
    }
}
