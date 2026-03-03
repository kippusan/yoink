use leptos::prelude::*;
use std::collections::HashSet;

use yoink_shared::{SearchAlbumResult, SearchArtistResult, SearchTrackResult, ServerAction};

use crate::components::toast::dispatch_with_toast_loading;
use crate::components::{MobileMenuButton, Sidebar};
use crate::hooks::set_page_title;
use crate::styles::{
    BREADCRUMB_CURRENT, BREADCRUMB_NAV, BTN_PRIMARY, EMPTY, GLASS, GLASS_BODY, GLASS_HEADER,
    GLASS_TITLE, HEADER_BAR, MUTED, btn_cls, cls,
};

const SEARCH_INPUT: &str = "py-2 px-3.5 border border-black/[.08] dark:border-white/10 rounded-lg font-inherit text-sm bg-white/60 dark:bg-zinc-800/60 backdrop-blur-[8px] text-zinc-900 dark:text-zinc-100 outline-none w-full max-w-[420px] transition-[border-color,box-shadow] duration-150 focus:border-blue-500 focus:shadow-[0_0_0_3px_rgba(59,130,246,.15)] dark:focus:shadow-[0_0_0_3px_rgba(59,130,246,.2)] placeholder:text-zinc-400 dark:placeholder:text-zinc-600";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SearchAllResult {
    pub artists: Vec<SearchArtistResult>,
    pub albums: Vec<SearchAlbumResult>,
    pub tracks: Vec<SearchTrackResult>,
}

#[server(SearchAll, "/leptos")]
pub async fn search_all(query: String) -> Result<SearchAllResult, ServerFnError> {
    let ctx = use_context::<yoink_shared::ServerContext>()
        .ok_or_else(|| ServerFnError::new("ServerContext not available"))?;

    let q = query.trim().to_string();
    if q.is_empty() {
        return Ok(SearchAllResult {
            artists: vec![],
            albums: vec![],
            tracks: vec![],
        });
    }

    let artists = (ctx.search_artists)(q.clone()).await.unwrap_or_default();
    let albums = (ctx.search_albums)(q.clone()).await.unwrap_or_default();
    let tracks = (ctx.search_tracks)(q).await.unwrap_or_default();

    Ok(SearchAllResult {
        artists,
        albums,
        tracks,
    })
}

#[component]
pub fn SearchPage() -> impl IntoView {
    set_page_title("Search");

    let (query, set_query) = signal(String::new());
    let (dedupe, set_dedupe) = signal(true);
    let result = Resource::new(move || query.get(), |q| async move { search_all(q).await });

    view! {
        <div class="flex min-h-screen">
            <Sidebar active="search" />
            <div class="ml-[220px] max-md:ml-0 flex-1 min-h-screen overflow-x-hidden">
                <div class=HEADER_BAR>
                    <nav class=BREADCRUMB_NAV aria-label="Breadcrumb"><MobileMenuButton />
                        <span class=BREADCRUMB_CURRENT>"Search"</span>
                    </nav>
                </div>

                <div class="p-6 max-md:p-4 space-y-5">
                    <div class=GLASS>
                        <div class=GLASS_BODY>
                            <div class="flex flex-wrap items-center gap-3">
                                <input
                                    type="text"
                                    class=SEARCH_INPUT
                                    placeholder="Search artists, albums, and tracks..."
                                    prop:value=move || query.get()
                                    on:input=move |ev| set_query.set(event_target_value(&ev))
                                />
                                <label class="inline-flex items-center gap-1.5 text-xs text-zinc-600 dark:text-zinc-300 select-none cursor-pointer">
                                    <input
                                        type="checkbox"
                                        prop:checked=move || dedupe.get()
                                        on:change=move |ev| set_dedupe.set(event_target_checked(&ev))
                                    />
                                    "Deduplicate"
                                </label>
                            </div>
                        </div>
                    </div>

                    <Transition fallback=move || view! { <div class=EMPTY>"Searching..."</div> }>
                        {move || {
                            let q = query.get();
                            if q.trim().is_empty() {
                                return Some(view! { <div class=EMPTY>"Type to search across all providers."</div> }.into_any());
                            }

                            result.get().map(|r| match r {
                                Err(e) => view! { <div class="text-sm text-red-500">{e.to_string()}</div> }.into_any(),
                                Ok(r) => {
                                    view! {
                                        <div class="space-y-5">
                                            <SearchArtistsSection items=r.artists dedupe=dedupe.get() />
                                            <SearchAlbumsSection items=r.albums dedupe=dedupe.get() />
                                            <SearchTracksSection items=r.tracks dedupe=dedupe.get() />
                                        </div>
                                    }.into_any()
                                }
                            })
                        }}
                    </Transition>
                </div>
            </div>
        </div>
    }
}

#[component]
fn SearchArtistsSection(items: Vec<SearchArtistResult>, dedupe: bool) -> impl IntoView {
    let items = if dedupe {
        let mut seen = HashSet::new();
        items
            .into_iter()
            .filter(|r| seen.insert(r.name.trim().to_lowercase()))
            .collect::<Vec<_>>()
    } else {
        items
    };

    view! {
        <div class=GLASS>
            <div class=GLASS_HEADER>
                <h2 class=GLASS_TITLE>{format!("Artists ({})", items.len())}</h2>
            </div>
            <div class={cls(GLASS_BODY, "p-0!")}>
                {if items.is_empty() {
                    view! { <div class=EMPTY>"No artists found"</div> }.into_any()
                } else {
                    view! {
                        <div class="divide-y divide-black/[.04] dark:divide-white/[.04]">
                            {items.into_iter().map(|r| {
                                let loading = RwSignal::new(false);
                                let provider = r.provider.clone();
                                let external_id = r.external_id.clone();
                                let name = r.name.clone();
                                let image_url = r.image_url.clone();
                                let external_url = r.url.clone();
                                let type_country = match (&r.artist_type, &r.country) {
                                    (Some(t), Some(c)) if !t.is_empty() && !c.is_empty() => {
                                        format!("{} · {}", t, c)
                                    }
                                    (Some(t), _) if !t.is_empty() => t.clone(),
                                    (_, Some(c)) if !c.is_empty() => c.clone(),
                                    _ => String::new(),
                                };
                                view! {
                                    <div class="px-4 py-3 flex items-center gap-3">
                                        <div class="size-10 rounded-md overflow-hidden bg-zinc-200 dark:bg-zinc-800 shrink-0">
                                            {match r.image_url.clone() {
                                                Some(url) => view! { <img src=url class="w-full h-full object-cover" alt="" /> }.into_any(),
                                                None => view! { <div class="w-full h-full grid place-items-center text-zinc-400 text-xs">"?"</div> }.into_any(),
                                            }}
                                        </div>
                                        <div class="flex-1 min-w-0">
                                            <div class="text-sm font-medium text-zinc-900 dark:text-zinc-100 truncate">{r.name}</div>
                                            <div class={cls(MUTED, "text-xs truncate")}>{
                                                if type_country.is_empty() {
                                                    r.provider.clone()
                                                } else {
                                                    format!("{} · {}", r.provider, type_country)
                                                }
                                            }</div>
                                            {if let Some(dis) = r.disambiguation.clone() {
                                                if dis.is_empty() {
                                                    view! { <span></span> }.into_any()
                                                } else {
                                                    view! { <div class={cls(MUTED, "text-xs truncate")}>{dis}</div> }.into_any()
                                                }
                                            } else {
                                                view! { <span></span> }.into_any()
                                            }}
                                            <div class="flex items-center gap-1.5 mt-1 flex-wrap">
                                                <span class="text-[10px] px-1.5 py-px rounded bg-zinc-500/[.08] text-zinc-500 dark:text-zinc-400 font-mono">{r.external_id.clone()}</span>
                                                {if let Some(pop) = r.popularity {
                                                    view! { <span class="text-[10px] px-1.5 py-px rounded bg-blue-500/[.10] text-blue-600 dark:text-blue-400">{format!("pop {pop}")}</span> }.into_any()
                                                } else {
                                                    view! { <span></span> }.into_any()
                                                }}
                                                {r.tags.iter().take(2).map(|t| {
                                                    view! { <span class="text-[10px] px-1.5 py-px rounded bg-amber-500/[.10] text-amber-700 dark:text-amber-300">{t.clone()}</span> }
                                                }).collect_view()}
                                            </div>
                                        </div>
                                        {if let Some(url) = r.url.clone() {
                                            view! { <a href=url target="_blank" rel="noreferrer" class="text-xs text-blue-500 hover:text-blue-400 no-underline">"Open"</a> }.into_any()
                                        } else {
                                            view! { <span></span> }.into_any()
                                        }}
                                        <button
                                            type="button"
                                            class=move || btn_cls(BTN_PRIMARY, "px-2.5 py-1 text-xs", loading.get())
                                            disabled=move || loading.get()
                                            on:click=move |_| {
                                                dispatch_with_toast_loading(
                                                    ServerAction::AddArtist {
                                                        name: name.clone(),
                                                        provider: provider.clone(),
                                                        external_id: external_id.clone(),
                                                        image_url: image_url.clone(),
                                                        external_url: external_url.clone(),
                                                    },
                                                    "Artist added",
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
                    }.into_any()
                }}
            </div>
        </div>
    }
}

#[component]
fn SearchAlbumsSection(items: Vec<SearchAlbumResult>, dedupe: bool) -> impl IntoView {
    let items = if dedupe {
        let mut seen = HashSet::new();
        items
            .into_iter()
            .filter(|r| {
                let key = format!(
                    "{}::{}::{}",
                    r.artist_name.trim().to_lowercase(),
                    r.title.trim().to_lowercase(),
                    r.release_date.clone().unwrap_or_default()
                );
                seen.insert(key)
            })
            .collect::<Vec<_>>()
    } else {
        items
    };

    view! {
        <div class=GLASS>
            <div class=GLASS_HEADER>
                <h2 class=GLASS_TITLE>{format!("Albums ({})", items.len())}</h2>
            </div>
            <div class={cls(GLASS_BODY, "p-0!")}>
                {if items.is_empty() {
                    view! { <div class=EMPTY>"No albums found"</div> }.into_any()
                } else {
                    view! {
                        <div class="divide-y divide-black/[.04] dark:divide-white/[.04]">
                            {items.into_iter().map(|r| {
                                let loading = RwSignal::new(false);
                                let provider = r.provider.clone();
                                let external_album_id = r.external_id.clone();
                                let artist_external_id = r.artist_external_id.clone();
                                let artist_name = r.artist_name.clone();
                                view! {
                                    <div class="px-4 py-3 flex items-center gap-3">
                                        <div class="size-10 rounded-md overflow-hidden bg-zinc-200 dark:bg-zinc-800 shrink-0">
                                            {match r.cover_url.clone() {
                                                Some(url) => view! { <img src=url class="w-full h-full object-cover" alt="" /> }.into_any(),
                                                None => view! { <div class="w-full h-full grid place-items-center text-zinc-400 text-xs">"No"</div> }.into_any(),
                                            }}
                                        </div>
                                        <div class="flex-1 min-w-0">
                                            <div class="text-sm font-medium text-zinc-900 dark:text-zinc-100 truncate">{r.title}</div>
                                            <div class={cls(MUTED, "text-xs truncate")}>{format!("{} · {}", r.artist_name, r.provider)}</div>
                                            <div class="flex items-center gap-1.5 mt-1 flex-wrap">
                                                {if let Some(rt) = r.album_type.clone() {
                                                    view! { <span class="text-[10px] px-1.5 py-px rounded bg-zinc-500/[.08] text-zinc-500 dark:text-zinc-400">{rt}</span> }.into_any()
                                                } else {
                                                    view! { <span></span> }.into_any()
                                                }}
                                                {if let Some(rd) = r.release_date.clone() {
                                                    view! { <span class="text-[10px] px-1.5 py-px rounded bg-zinc-500/[.08] text-zinc-500 dark:text-zinc-400">{rd}</span> }.into_any()
                                                } else {
                                                    view! { <span></span> }.into_any()
                                                }}
                                                {if r.explicit {
                                                    view! { <span class="text-[10px] px-1.5 py-px rounded bg-zinc-200 text-zinc-600 dark:bg-zinc-700 dark:text-zinc-300">"Explicit"</span> }.into_any()
                                                } else {
                                                    view! { <span></span> }.into_any()
                                                }}
                                                <span class="text-[10px] px-1.5 py-px rounded bg-zinc-500/[.08] text-zinc-500 dark:text-zinc-400 font-mono">{r.external_id.clone()}</span>
                                            </div>
                                        </div>
                                        {if let Some(url) = r.url.clone() {
                                            view! { <a href=url target="_blank" rel="noreferrer" class="text-xs text-blue-500 hover:text-blue-400 no-underline">"Open"</a> }.into_any()
                                        } else {
                                            view! { <span></span> }.into_any()
                                        }}
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
                    }.into_any()
                }}
            </div>
        </div>
    }
}

#[component]
fn SearchTracksSection(items: Vec<SearchTrackResult>, dedupe: bool) -> impl IntoView {
    let items = if dedupe {
        let mut seen = HashSet::new();
        items
            .into_iter()
            .filter(|r| {
                let key = format!(
                    "{}::{}::{}::{}",
                    r.artist_name.trim().to_lowercase(),
                    r.album_title.trim().to_lowercase(),
                    r.title.trim().to_lowercase(),
                    r.duration_secs
                );
                seen.insert(key)
            })
            .collect::<Vec<_>>()
    } else {
        items
    };

    view! {
        <div class=GLASS>
            <div class=GLASS_HEADER>
                <h2 class=GLASS_TITLE>{format!("Tracks ({})", items.len())}</h2>
            </div>
            <div class={cls(GLASS_BODY, "p-0!")}>
                {if items.is_empty() {
                    view! { <div class=EMPTY>"No tracks found"</div> }.into_any()
                } else {
                    view! {
                        <div class="divide-y divide-black/[.04] dark:divide-white/[.04]">
                            {items.into_iter().map(|r| {
                                let loading = RwSignal::new(false);
                                let provider = r.provider.clone();
                                let external_track_id = r.external_id.clone();
                                let external_album_id = r.album_external_id.clone();
                                let artist_external_id = r.artist_external_id.clone();
                                let artist_name = r.artist_name.clone();
                                view! {
                                    <div class="px-4 py-3 flex items-center gap-3">
                                        <div class="size-10 rounded-md overflow-hidden bg-zinc-200 dark:bg-zinc-800 shrink-0">
                                            {match r.album_cover_url.clone() {
                                                Some(url) => view! { <img src=url class="w-full h-full object-cover" alt="" /> }.into_any(),
                                                None => view! { <div class="w-full h-full grid place-items-center text-zinc-400 text-xs">"♪"</div> }.into_any(),
                                            }}
                                        </div>
                                        <div class="flex-1 min-w-0">
                                            <div class="text-sm font-medium text-zinc-900 dark:text-zinc-100 truncate">{r.title}</div>
                                            <div class={cls(MUTED, "text-xs truncate")}>{format!("{} · {}", r.artist_name, r.album_title)}</div>
                                            <div class="flex items-center gap-1.5 mt-1 flex-wrap">
                                                <span class="text-[10px] px-1.5 py-px rounded bg-blue-500/[.10] text-blue-600 dark:text-blue-400">
                                                    {r.provider.clone()}
                                                </span>
                                                <span class="text-[10px] px-1.5 py-px rounded bg-zinc-500/[.08] text-zinc-500 dark:text-zinc-400">{r.duration_display.clone()}</span>
                                                {if let Some(v) = r.version.clone() {
                                                    if v.is_empty() {
                                                        view! { <span></span> }.into_any()
                                                    } else {
                                                        view! { <span class="text-[10px] px-1.5 py-px rounded bg-zinc-500/[.08] text-zinc-500 dark:text-zinc-400">{v}</span> }.into_any()
                                                    }
                                                } else {
                                                    view! { <span></span> }.into_any()
                                                }}
                                                {if let Some(isrc) = r.isrc.clone() {
                                                    view! { <span class="text-[10px] px-1.5 py-px rounded bg-zinc-500/[.08] text-zinc-500 dark:text-zinc-400 font-mono">{isrc}</span> }.into_any()
                                                } else {
                                                    view! { <span></span> }.into_any()
                                                }}
                                                {if r.explicit {
                                                    view! { <span class="text-[10px] px-1.5 py-px rounded bg-zinc-200 text-zinc-600 dark:bg-zinc-700 dark:text-zinc-300">"Explicit"</span> }.into_any()
                                                } else {
                                                    view! { <span></span> }.into_any()
                                                }}
                                            </div>
                                        </div>
                                        <button
                                            type="button"
                                            class=move || btn_cls(BTN_PRIMARY, "px-2.5 py-1 text-xs", loading.get())
                                            disabled=move || loading.get()
                                            on:click=move |_| {
                                                dispatch_with_toast_loading(
                                                    ServerAction::AddTrack {
                                                        provider: provider.clone(),
                                                        external_track_id: external_track_id.clone(),
                                                        external_album_id: external_album_id.clone(),
                                                        artist_external_id: artist_external_id.clone(),
                                                        artist_name: artist_name.clone(),
                                                    },
                                                    "Track added",
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
                    }.into_any()
                }}
            </div>
        </div>
    }
}
