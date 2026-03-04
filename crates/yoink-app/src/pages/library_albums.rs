use std::collections::HashMap;

use leptos::prelude::*;

use yoink_shared::{
    MonitoredAlbum, MonitoredArtist, SearchAlbumResult, ServerAction, album_cover_url,
    album_type_label,
};

use crate::components::toast::dispatch_with_toast_loading;
use crate::components::{MobileMenuButton, Sidebar};
use crate::hooks::set_page_title;
use crate::styles::{
    BREADCRUMB_CURRENT, BREADCRUMB_LINK, BREADCRUMB_NAV, BREADCRUMB_SEP, HEADER_BAR,
};
use crate::styles::{
    BTN_PRIMARY, EMPTY, GLASS, GLASS_BODY, GLASS_HEADER, GLASS_TITLE, MUTED, SELECT, btn_cls, cls,
};

const SEARCH_INPUT: &str = "py-2 px-3.5 border border-black/[.08] dark:border-white/10 rounded-lg font-inherit text-sm bg-white/60 dark:bg-zinc-800/60 backdrop-blur-[8px] text-zinc-900 dark:text-zinc-100 outline-none w-full max-w-[360px] transition-[border-color,box-shadow] duration-150 focus:border-blue-500 focus:shadow-[0_0_0_3px_rgba(59,130,246,.15)] dark:focus:shadow-[0_0_0_3px_rgba(59,130,246,.2)] placeholder:text-zinc-400 dark:placeholder:text-zinc-600";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LibraryAlbumsData {
    pub artists: Vec<MonitoredArtist>,
    pub albums: Vec<MonitoredAlbum>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SearchAlbumsResult {
    pub results: Vec<SearchAlbumResult>,
    pub error: Option<String>,
}

#[server(GetLibraryAlbumsData, "/leptos")]
pub async fn get_library_albums_data() -> Result<LibraryAlbumsData, ServerFnError> {
    let ctx = use_context::<yoink_shared::ServerContext>()
        .ok_or_else(|| ServerFnError::new("ServerContext not available"))?;

    let artists = ctx.monitored_artists.read().await.clone();
    let all_albums = ctx.monitored_albums.read().await.clone();

    // Keep only albums that are at least partially monitored:
    // - fully monitored album, OR
    // - has any monitored track (even if already acquired, i.e. not partially_wanted).
    let mut albums = Vec::new();
    for album in all_albums {
        if album.monitored {
            albums.push(album);
            continue;
        }

        let tracks = (ctx.fetch_tracks)(album.id).await.unwrap_or_default();
        if tracks.iter().any(|t| t.monitored) {
            albums.push(album);
        }
    }

    Ok(LibraryAlbumsData { artists, albums })
}

#[server(SearchAlbums, "/leptos")]
pub async fn search_albums(query: String) -> Result<SearchAlbumsResult, ServerFnError> {
    let ctx = use_context::<yoink_shared::ServerContext>()
        .ok_or_else(|| ServerFnError::new("ServerContext not available"))?;

    let q = query.trim().to_string();
    if q.is_empty() {
        return Ok(SearchAlbumsResult {
            results: vec![],
            error: None,
        });
    }

    match (ctx.search_albums)(q).await {
        Ok(results) => Ok(SearchAlbumsResult {
            results,
            error: None,
        }),
        Err(err) => Ok(SearchAlbumsResult {
            results: vec![],
            error: Some(err),
        }),
    }
}

#[component]
pub fn LibraryAlbumsTab() -> impl IntoView {
    let data = Resource::new(|| (), |_| get_library_albums_data());
    let (query, set_query) = signal(String::new());
    let (sort_key, set_sort_key) = signal("recent".to_string());
    let (filter_key, set_filter_key) = signal("all".to_string());

    let search_result: Resource<Result<SearchAlbumsResult, ServerFnError>> = Resource::new(
        move || query.get(),
        |q| async move { search_albums(q).await },
    );

    view! {
        <Transition fallback=move || view! { <div class="p-6 max-md:p-4"><div class=EMPTY>"Loading albums..."</div></div> }>
            {move || {
                data.get().map(|result| match result {
                    Err(e) => view! { <div class="p-6 text-sm text-red-500">{e.to_string()}</div> }.into_any(),
                    Ok(data) => {
                        let artist_names: HashMap<_, _> = data
                            .artists
                            .into_iter()
                            .map(|a| (a.id, a.name))
                            .collect();
                        let albums = StoredValue::new(data.albums);

                        view! {
                            <div class="p-6 max-md:p-4 space-y-5">
                                <div class=GLASS>
                                    <div class=GLASS_BODY>
                                        <div class="flex flex-wrap items-center gap-2">
                                            <input
                                                type="text"
                                                class=SEARCH_INPUT
                                                placeholder="Search albums (local + provider)..."
                                                prop:value=move || query.get()
                                                on:input=move |ev| set_query.set(event_target_value(&ev))
                                            />
                                            <select class=SELECT on:change=move |ev| set_filter_key.set(event_target_value(&ev))>
                                                <option value="all" selected=true>"All"</option>
                                                <option value="monitored">"Monitored"</option>
                                                <option value="wanted">"Wanted"</option>
                                                <option value="acquired">"Acquired"</option>
                                            </select>
                                            <select class=SELECT on:change=move |ev| set_sort_key.set(event_target_value(&ev))>
                                                <option value="az">"A-Z"</option>
                                                <option value="newest">"Newest"</option>
                                                <option value="oldest">"Oldest"</option>
                                                <option value="recent" selected=true>"Recently Added"</option>
                                                <option value="artist">"By Artist"</option>
                                            </select>
                                        </div>
                                    </div>
                                </div>

                                <div class=GLASS>
                                    <div class=GLASS_HEADER>
                                        <h2 class=GLASS_TITLE>"Albums"</h2>
                                    </div>
                                    <div class={cls(GLASS_BODY, "p-4")}>
                                        <div class="grid grid-cols-[repeat(auto-fill,minmax(220px,1fr))] gap-4">
                                            {move || {
                                                let q = query.get().trim().to_lowercase();
                                                let filter = filter_key.get();
                                                let sort = sort_key.get();
                                                albums.with_value(|all| {
                                                    let mut items: Vec<_> = all
                                                        .iter()
                                                        .filter(|a| {
                                                            if !q.is_empty() {
                                                                let artist = artist_names.get(&a.artist_id).cloned().unwrap_or_default();
                                                                if !a.title.to_lowercase().contains(&q)
                                                                    && !artist.to_lowercase().contains(&q)
                                                                {
                                                                    return false;
                                                                }
                                                            }
                                                            match filter.as_str() {
                                                                "monitored" => a.monitored,
                                                                "wanted" => a.wanted || a.partially_wanted,
                                                                "acquired" => a.acquired,
                                                                _ => true,
                                                            }
                                                        })
                                                        .collect();

                                                    match sort.as_str() {
                                                        "az" => items.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase())),
                                                        "newest" => items.sort_by(|a, b| b.release_date.cmp(&a.release_date)),
                                                        "oldest" => items.sort_by(|a, b| a.release_date.cmp(&b.release_date)),
                                                        "artist" => items.sort_by(|a, b| {
                                                            let an = artist_names.get(&a.artist_id).cloned().unwrap_or_default();
                                                            let bn = artist_names.get(&b.artist_id).cloned().unwrap_or_default();
                                                            an.to_lowercase().cmp(&bn.to_lowercase())
                                                        }),
                                                        _ => items.sort_by(|a, b| b.added_at.cmp(&a.added_at)),
                                                    }

                                                    if items.is_empty() {
                                                        return view! { <div class="col-span-full"><div class=EMPTY>"No matching albums"</div></div> }.into_any();
                                                    }

                                                    items
                                                        .into_iter()
                                                        .map(|album| {
                                                            let artist_name = artist_names
                                                                .get(&album.artist_id)
                                                                .cloned()
                                                                .unwrap_or_else(|| "Unknown Artist".to_string());
                                                            let href = format!("/artists/{}/albums/{}", album.artist_id, album.id);
                                                            let cover = album_cover_url(album, 320);
                                                            let at = album_type_label(album.album_type.as_deref(), &album.title);
                                                            view! {
                                                                <a href=href class="bg-white/70 dark:bg-zinc-800/60 border border-black/[.06] dark:border-white/[.08] rounded-xl p-3 no-underline transition-[transform,border-color] duration-150 hover:-translate-y-0.5 hover:border-blue-500/30">
                                                                    <div class="aspect-square rounded-lg overflow-hidden bg-zinc-200 dark:bg-zinc-800 mb-2">
                                                                        {match cover {
                                                                            Some(url) => view! { <img src=url class="w-full h-full object-cover" alt="" /> }.into_any(),
                                                                            None => view! { <div class="w-full h-full grid place-items-center text-zinc-400 text-xs">"No Cover"</div> }.into_any(),
                                                                        }}
                                                                    </div>
                                                                    <div class="text-sm font-semibold text-zinc-900 dark:text-zinc-100 truncate">{album.title.clone()}</div>
                                                                    <div class={cls(MUTED, "text-xs truncate")}>{artist_name}</div>
                                                                    <div class="flex items-center gap-1.5 mt-1.5 flex-wrap">
                                                                        <span class="text-[10px] px-1.5 py-px rounded bg-zinc-500/[.08] text-zinc-500 dark:text-zinc-400">{at}</span>
                                                                        {if album.acquired {
                                                                            view! { <span class="text-[10px] px-1.5 py-px rounded bg-green-500/[.12] text-green-600 dark:text-green-400">"Acquired"</span> }.into_any()
                                                                        } else if album.wanted || album.partially_wanted {
                                                                            view! { <span class="text-[10px] px-1.5 py-px rounded bg-amber-500/[.12] text-amber-600 dark:text-amber-300">"Wanted"</span> }.into_any()
                                                                        } else {
                                                                            view! { <span></span> }.into_any()
                                                                        }}
                                                                    </div>
                                                                </a>
                                                            }
                                                        })
                                                        .collect_view()
                                                        .into_any()
                                                })
                                            }}
                                        </div>
                                    </div>
                                </div>

                                <Suspense>
                                    {move || {
                                        if query.get().trim().is_empty() {
                                            return Some(view! { <span></span> }.into_any());
                                        }
                                        search_result.get().map(|res| match res {
                                            Err(e) => view! { <div class="text-sm text-red-500">{e.to_string()}</div> }.into_any(),
                                            Ok(sr) => {
                                                if sr.results.is_empty() {
                                                    return view! { <span></span> }.into_any();
                                                }
                                                view! {
                                                    <div class=GLASS>
                                                        <div class=GLASS_HEADER>
                                                            <h2 class=GLASS_TITLE>"Add Albums From Providers"</h2>
                                                        </div>
                                                        <div class={cls(GLASS_BODY, "p-0!")}>
                                                            <div class="divide-y divide-black/[.04] dark:divide-white/[.04]">
                                                                {sr.results.into_iter().map(|r| {
                                                                    let loading = RwSignal::new(false);
                                                                    let provider = r.provider.clone();
                                                                    let external_album_id = r.external_id.clone();
                                                                    let artist_external_id = r.artist_external_id.clone();
                                                                    let artist_name = r.artist_name.clone();
                                                                    view! {
                                                                        <div class="px-4 py-3 flex items-center gap-3">
                                                                            <div class="flex-1 min-w-0">
                                                                                <div class="text-sm font-medium text-zinc-900 dark:text-zinc-100 truncate">{r.title}</div>
                                                                                <div class={cls(MUTED, "text-xs truncate")}>{format!("{} · {}", r.artist_name, r.provider)}</div>
                                                                            </div>
                                                                            <button
                                                                                type="button"
                                                                                class=move || btn_cls(BTN_PRIMARY, "px-2.5 py-1 text-xs", loading.get())
                                                                                disabled=move || loading.get()
                                                                                on:click=move |_| {
                                                                                    dispatch_with_toast_loading(
                                                                                        ServerAction::AddAlbum {
                                                                                            provider: provider.clone(),
                                                                                            external_album_id: external_album_id.clone(),
                                                                                            artist_external_id: artist_external_id.clone(),
                                                                                            artist_name: artist_name.clone(),
                                                                                            monitor_all: false,
                                                                                        },
                                                                                        "Album added",
                                                                                        Some(loading),
                                                                                    );
                                                                                }
                                                                            >
                                                                                "Add"
                                                                            </button>
                                                                        </div>
                                                                    }
                                                                }).collect_view()}
                                                            </div>
                                                        </div>
                                                    </div>
                                                }.into_any()
                                            }
                                        })
                                    }}
                                </Suspense>
                            </div>
                        }
                        .into_any()
                    }
                })
            }}
        </Transition>
    }
}

#[component]
pub fn LibraryAlbumsPage() -> impl IntoView {
    set_page_title("Library - Albums");
    view! {
        <div class="flex min-h-screen">
            <Sidebar active="library-albums" />
            <div class="ml-[220px] max-md:ml-0 flex-1 min-h-screen overflow-x-hidden">
                <div class=HEADER_BAR>
                    <nav class=BREADCRUMB_NAV aria-label="Breadcrumb"><MobileMenuButton />
                        <a href="/library/artists" class=BREADCRUMB_LINK>"Library"</a>
                        <span class=BREADCRUMB_SEP><lucide_leptos::ChevronRight /></span>
                        <span class=BREADCRUMB_CURRENT>"Albums"</span>
                    </nav>
                </div>
                <LibraryAlbumsTab />
            </div>
        </div>
    }
}
