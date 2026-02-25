use leptos::prelude::*;

use crate::shared::{
    build_albums_by_artist, monitored_artist_image_url, search_artist_image_url,
    search_artist_profile_url, MonitoredAlbum, MonitoredArtist, SearchArtistResult, ServerAction,
};

use crate::app::actions::dispatch_action;

use super::super::components::Sidebar;
use super::super::hooks::use_sse_version;

// ── Tailwind class constants ────────────────────────────────

const GLASS: &str = "bg-white/70 dark:bg-zinc-800/60 backdrop-blur-[12px] border border-black/[.06] dark:border-white/[.08] rounded-xl mb-6 overflow-hidden";
const GLASS_HEADER: &str = "px-5 py-3.5 border-b border-black/[.06] dark:border-white/[.06] flex items-center justify-between gap-3";
const GLASS_TITLE: &str = "text-[15px] font-semibold text-zinc-900 dark:text-zinc-100 m-0";
const GLASS_BODY: &str = "px-5 py-4";
const MUTED: &str = "text-zinc-500 dark:text-zinc-400";
const EMPTY: &str = "text-center py-10 px-4 text-zinc-400 dark:text-zinc-600 text-sm";
const LINK: &str = "text-blue-500 no-underline font-medium hover:text-blue-400 hover:underline";
const BTN_PRIMARY: &str = "inline-flex items-center justify-center gap-1.5 px-3.5 py-1.5 bg-blue-500 dark:bg-blue-500 backdrop-blur-[8px] border border-blue-500 rounded-lg font-inherit text-[13px] font-medium cursor-pointer text-white no-underline transition-all duration-150 whitespace-nowrap shadow-[0_2px_12px_rgba(59,130,246,.25)] hover:bg-blue-400 hover:border-blue-400 hover:shadow-[0_4px_20px_rgba(59,130,246,.35)]";
const SEARCH_INPUT: &str = "py-2 px-3.5 border border-black/[.08] dark:border-white/10 rounded-lg font-inherit text-sm bg-white/60 dark:bg-zinc-800/60 backdrop-blur-[8px] text-zinc-900 dark:text-zinc-100 outline-none w-full max-w-[360px] transition-[border-color,box-shadow] duration-150 focus:border-blue-500 focus:shadow-[0_0_0_3px_rgba(59,130,246,.15)] dark:focus:shadow-[0_0_0_3px_rgba(59,130,246,.2)] placeholder:text-zinc-400 dark:placeholder:text-zinc-600";
const SEARCH_RESULT: &str = "flex items-center gap-3.5 px-4 py-3 border-b border-black/[.04] dark:border-white/[.04] transition-[background] duration-[120ms] last:border-b-0 hover:bg-blue-500/[.04] dark:hover:bg-blue-500/[.06]";
const ARTIST_CARD: &str = "bg-white/70 dark:bg-zinc-800/60 backdrop-blur-[12px] border border-black/[.06] dark:border-white/[.08] rounded-xl p-4 flex items-center gap-3.5 transition-[transform,box-shadow,border-color] duration-200 relative overflow-hidden no-underline cursor-pointer hover:-translate-y-0.5 hover:shadow-[0_8px_32px_rgba(59,130,246,.1)] hover:border-blue-500/20 dark:hover:shadow-[0_8px_32px_rgba(59,130,246,.15)] dark:hover:border-blue-500/30";
const ARTIST_AVATAR: &str = "size-12 rounded-full object-cover border-2 border-blue-500/20 dark:border-blue-500/30 shrink-0 bg-zinc-200 dark:bg-zinc-800";
const ARTIST_FALLBACK: &str = "size-12 rounded-full inline-flex items-center justify-center bg-zinc-200 dark:bg-zinc-800 text-zinc-500 dark:text-zinc-400 font-bold text-lg border-2 border-blue-500/20 dark:border-blue-500/30 shrink-0";

fn cls(a: &str, b: &str) -> String {
    format!("{a} {b}")
}

// ── DTOs ────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ArtistsData {
    pub monitored: Vec<MonitoredArtist>,
    pub albums: Vec<MonitoredAlbum>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SearchResult {
    pub results: Vec<SearchArtistResult>,
    pub error: Option<String>,
}

// ── Server functions ────────────────────────────────────────

#[server(GetArtistsData, "/leptos")]
pub async fn get_artists_data() -> Result<ArtistsData, ServerFnError> {
    let ctx = use_context::<crate::shared::ServerContext>()
        .ok_or_else(|| ServerFnError::new("ServerContext not available"))?;

    let monitored = ctx.monitored_artists.read().await.clone();
    let albums = ctx.monitored_albums.read().await.clone();

    Ok(ArtistsData { monitored, albums })
}

#[server(SearchArtists, "/leptos")]
pub async fn search_artists(query: String) -> Result<SearchResult, ServerFnError> {
    let ctx = use_context::<crate::shared::ServerContext>()
        .ok_or_else(|| ServerFnError::new("ServerContext not available"))?;

    let trimmed = query.trim().to_string();
    if trimmed.is_empty() {
        return Ok(SearchResult {
            results: vec![],
            error: None,
        });
    }

    match (ctx.search_artists)(trimmed).await {
        Ok(results) => Ok(SearchResult {
            results,
            error: None,
        }),
        Err(err) => Ok(SearchResult {
            results: vec![],
            error: Some(err),
        }),
    }
}

// ── Page component ──────────────────────────────────────────

#[component]
pub fn ArtistsPage() -> impl IntoView {
    let version = use_sse_version();
    let data = Resource::new(move || version.get(), |_| get_artists_data());

    // Search state
    let (query, set_query) = signal(String::new());
    let search_result: Resource<Result<SearchResult, ServerFnError>> = Resource::new(
        move || query.get(),
        |q| async move {
            if q.trim().is_empty() {
                Ok(SearchResult {
                    results: vec![],
                    error: None,
                })
            } else {
                search_artists(q).await
            }
        },
    );

    view! {
        <div class="flex min-h-screen">
            <Sidebar active="artists" />
            <div class="ml-[220px] max-md:ml-0 flex-1 min-h-screen">
                <Suspense fallback=move || view! {
                    <div class="bg-white/70 dark:bg-zinc-800/60 backdrop-blur-[16px] border-b border-black/[.06] dark:border-white/[.06] px-6 py-3.5 flex items-center justify-between sticky top-0 z-40">
                        <h1 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100 m-0">"Artists"</h1>
                        <span class={cls(MUTED, "text-[13px]")}>"Loading\u{2026}"</span>
                    </div>
                }>
                    {move || {
                        data.get().map(|result| match result {
                            Err(e) => view! {
                                <div class="p-6">
                                    <div class="text-red-500">{format!("Error: {e}")}</div>
                                </div>
                            }.into_any(),
                            Ok(data) => {
                                view! { <ArtistsContent data=data query=query set_query=set_query search_result=search_result /> }.into_any()
                            }
                        })
                    }}
                </Suspense>
            </div>
        </div>
    }
}

#[component]
fn ArtistsContent(
    data: ArtistsData,
    query: ReadSignal<String>,
    set_query: WriteSignal<String>,
    search_result: Resource<Result<SearchResult, ServerFnError>>,
) -> impl IntoView {
    let monitored_count = data.monitored.len();
    let albums_by_artist = build_albums_by_artist(data.albums);

    view! {
        // Header
        <div class="bg-white/70 dark:bg-zinc-800/60 backdrop-blur-[16px] border-b border-black/[.06] dark:border-white/[.06] px-6 py-3.5 flex items-center justify-between sticky top-0 z-40">
            <h1 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100 m-0">"Artists"</h1>
            <span class={cls(MUTED, "text-[13px]")}>{format!("{monitored_count} monitored")}</span>
        </div>

        <div class="p-6 max-md:p-4">
            // Search panel
            <div class={cls(GLASS, "mb-5 !overflow-visible relative z-50")}>
                <div class={cls(GLASS_BODY, "!overflow-visible")}>
                    <div class="flex flex-wrap items-center gap-2">
                        <input
                            type="text"
                            class={cls(SEARCH_INPUT, "max-w-full")}
                            placeholder="Search artist name..."
                            autocomplete="off"
                            prop:value=move || query.get()
                            on:input=move |ev| {
                                let val = event_target_value(&ev);
                                set_query.set(val);
                            }
                        />
                    </div>
                </div>
            </div>

            // Search results
            <Suspense fallback=move || ()>
                {move || {
                    search_result.get().map(|result| {
                        match result {
                            Err(e) => view! {
                                <div class="px-4 py-3 mb-6 rounded-[10px] text-[13px] border border-red-500/30 bg-red-500/[.08] text-red-600">
                                    {format!("Search error: {e}")}
                                </div>
                            }.into_any(),
                            Ok(sr) => {
                                let error_view = sr.error.map(|msg| view! {
                                    <div class="px-4 py-3 mb-6 rounded-[10px] text-[13px] border border-red-500/30 bg-red-500/[.08] text-red-600">
                                        {msg}
                                    </div>
                                });

                                let results_view = if sr.results.is_empty() {
                                    None
                                } else {
                                    Some(view! {
                                        <div class=GLASS>
                                            <div class=GLASS_HEADER>
                                                <h2 class=GLASS_TITLE>"Search Results"</h2>
                                            </div>
                                            <div>
                                                {sr.results.into_iter().map(|artist| {
                                                    view! { <SearchResultRow artist=artist /> }
                                                }).collect_view()}
                                            </div>
                                        </div>
                                    })
                                };

                                view! {
                                    <div>
                                        {error_view}
                                        {results_view}
                                    </div>
                                }.into_any()
                            }
                        }
                    })
                }}
            </Suspense>

            // Monitored artists collection
            <div class=GLASS>
                <div class=GLASS_HEADER>
                    <h2 class=GLASS_TITLE>"Your Collection"</h2>
                </div>
                {if data.monitored.is_empty() {
                    view! { <div class=EMPTY>"No monitored artists yet. Search and add one above."</div> }.into_any()
                } else {
                    view! {
                        <div class={cls(GLASS_BODY, "p-4")}>
                            <div class="grid grid-cols-[repeat(auto-fill,minmax(280px,1fr))] gap-4">
                                {data.monitored.into_iter().map(|artist| {
                                    let artist_albums = albums_by_artist
                                        .get(&artist.id)
                                        .cloned()
                                        .unwrap_or_default();
                                    view! { <ArtistCard artist=artist albums=artist_albums /> }
                                }).collect_view()}
                            </div>
                        </div>
                    }.into_any()
                }}
            </div>
        </div>
    }
}

/// A single search result row with an "Add" button.
#[component]
fn SearchResultRow(artist: SearchArtistResult) -> impl IntoView {
    let navigate = leptos_router::hooks::use_navigate();
    let image_url = search_artist_image_url(&artist, 160);
    let profile_url = search_artist_profile_url(&artist);
    let fallback_initial = artist
        .name
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_string());
    let artist_id = artist.id;
    let artist_name = artist.name.clone();
    let picture = artist.picture.clone();
    let tidal_url = artist.url.clone();

    view! {
        <div class=SEARCH_RESULT>
            {match image_url {
                Some(url) => view! {
                    <img class=ARTIST_AVATAR src=url alt="" />
                }.into_any(),
                None => view! {
                    <div class=ARTIST_FALLBACK>{fallback_initial}</div>
                }.into_any(),
            }}
            <div class="flex-1 min-w-0">
                <div class="text-[15px] font-semibold text-zinc-900 dark:text-zinc-100">{artist.name}</div>
                <a class={cls(LINK, "text-xs")} href=profile_url target="_blank" rel="noreferrer">"View on Tidal"</a>
            </div>
            <button type="button" class={cls(BTN_PRIMARY, "px-2.5 py-0.5 text-xs")}
                on:click=move |_| {
                    let name = artist_name.clone();
                    let pic = picture.clone();
                    let url = tidal_url.clone();
                    let navigate = navigate.clone();
                    leptos::task::spawn_local(async move {
                        if dispatch_action(ServerAction::AddArtist {
                            id: artist_id,
                            name,
                            picture: pic,
                            tidal_url: url,
                        }).await.is_ok() {
                            let path = format!("/artists/{artist_id}");
                            navigate(&path, Default::default());
                        }
                    });
                }>"+ Add"</button>
        </div>
    }
}

/// A monitored artist card linking to the detail page.
#[component]
fn ArtistCard(artist: MonitoredArtist, albums: Vec<MonitoredAlbum>) -> impl IntoView {
    let album_count = albums.len();
    let wanted = albums.iter().filter(|a| a.wanted).count();
    let acquired = albums.iter().filter(|a| a.acquired).count();
    let album_count_text = format!("{album_count} albums \u{00b7} {acquired} acquired \u{00b7} {wanted} wanted");
    let fallback_initial = artist
        .name
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_string());
    let artist_img = monitored_artist_image_url(&artist, 160);
    let detail_href = format!("/artists/{}", artist.id);

    view! {
        <a href=detail_href class=ARTIST_CARD>
            {match artist_img {
                Some(url) => view! {
                    <img class=ARTIST_AVATAR src=url alt="" />
                }.into_any(),
                None => view! {
                    <div class=ARTIST_FALLBACK>{fallback_initial}</div>
                }.into_any(),
            }}
            <div class="flex-1 min-w-0">
                <div class="text-[15px] font-bold text-zinc-900 dark:text-zinc-100 whitespace-nowrap overflow-hidden text-ellipsis">{artist.name}</div>
                <div class="text-xs text-zinc-500 dark:text-zinc-400">{album_count_text}</div>
            </div>
        </a>
    }
}
