use leptos::prelude::*;

use yoink_shared::{LibraryTrack, SearchTrackResult, ServerAction};

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
pub struct SearchTracksResult {
    pub results: Vec<SearchTrackResult>,
    pub error: Option<String>,
}

#[server(GetLibraryTracksData, "/leptos")]
pub async fn get_library_tracks_data() -> Result<Vec<LibraryTrack>, ServerFnError> {
    let ctx = use_context::<yoink_shared::ServerContext>()
        .ok_or_else(|| ServerFnError::new("ServerContext not available"))?;

    (ctx.fetch_library_tracks)()
        .await
        .map(|tracks| {
            tracks
                .into_iter()
                .filter(|t| t.track.monitored)
                .collect::<Vec<_>>()
        })
        .map_err(ServerFnError::new)
}

#[server(SearchTracksLibrary, "/leptos")]
pub async fn search_tracks_library(query: String) -> Result<SearchTracksResult, ServerFnError> {
    let ctx = use_context::<yoink_shared::ServerContext>()
        .ok_or_else(|| ServerFnError::new("ServerContext not available"))?;

    let q = query.trim().to_string();
    if q.is_empty() {
        return Ok(SearchTracksResult {
            results: vec![],
            error: None,
        });
    }

    match (ctx.search_tracks)(q).await {
        Ok(results) => Ok(SearchTracksResult {
            results,
            error: None,
        }),
        Err(err) => Ok(SearchTracksResult {
            results: vec![],
            error: Some(err),
        }),
    }
}

#[component]
pub fn LibraryTracksTab() -> impl IntoView {
    let data = Resource::new(|| (), |_| get_library_tracks_data());
    let (query, set_query) = signal(String::new());
    let (sort_key, set_sort_key) = signal("artist".to_string());
    let (filter_key, set_filter_key) = signal("all".to_string());

    let search_result: Resource<Result<SearchTracksResult, ServerFnError>> = Resource::new(
        move || query.get(),
        |q| async move { search_tracks_library(q).await },
    );

    view! {
        <Transition fallback=move || view! { <div class="p-6 max-md:p-4"><div class=EMPTY>"Loading tracks..."</div></div> }>
            {move || {
                data.get().map(|result| match result {
                    Err(e) => view! { <div class="p-6 text-sm text-red-500">{e.to_string()}</div> }.into_any(),
                    Ok(tracks) => {
                        let tracks = StoredValue::new(tracks);
                        view! {
                            <div class="p-6 max-md:p-4 space-y-5">
                                <div class=GLASS>
                                    <div class=GLASS_BODY>
                                        <div class="flex flex-wrap items-center gap-2">
                                            <input
                                                type="text"
                                                class=SEARCH_INPUT
                                                placeholder="Search tracks (local + provider)..."
                                                prop:value=move || query.get()
                                                on:input=move |ev| set_query.set(event_target_value(&ev))
                                            />
                                            <select class=SELECT on:change=move |ev| set_filter_key.set(event_target_value(&ev))>
                                                <option value="all" selected=true>"All"</option>
                                                <option value="wanted">"Wanted"</option>
                                                <option value="acquired">"Acquired"</option>
                                            </select>
                                            <select class=SELECT on:change=move |ev| set_sort_key.set(event_target_value(&ev))>
                                                <option value="az">"A-Z"</option>
                                                <option value="album">"By Album"</option>
                                                <option value="artist" selected=true>"By Artist"</option>
                                                <option value="duration">"Duration"</option>
                                            </select>
                                        </div>
                                    </div>
                                </div>

                                <div class=GLASS>
                                    <div class=GLASS_HEADER>
                                        <h2 class=GLASS_TITLE>"Tracks"</h2>
                                    </div>
                                    <div class={cls(GLASS_BODY, "p-0!")}>
                                        <div class="divide-y divide-black/[.04] dark:divide-white/[.04]">
                                            {move || {
                                                let q = query.get().trim().to_lowercase();
                                                let filter = filter_key.get();
                                                let sort = sort_key.get();

                                                tracks.with_value(|all| {
                                                    let mut rows: Vec<_> = all
                                                        .iter()
                                                        .filter(|row| {
                                                            let t = &row.track;
                                                            if !q.is_empty()
                                                                && !t.title.to_lowercase().contains(&q)
                                                                && !row.album_title.to_lowercase().contains(&q)
                                                                && !row.artist_name.to_lowercase().contains(&q)
                                                            {
                                                                return false;
                                                            }
                                                            match filter.as_str() {
                                                                "wanted" => t.monitored && !t.acquired,
                                                                "acquired" => t.acquired,
                                                                _ => true,
                                                            }
                                                        })
                                                        .cloned()
                                                        .collect();

                                                    match sort.as_str() {
                                                        "az" => rows.sort_by(|a, b| a.track.title.to_lowercase().cmp(&b.track.title.to_lowercase())),
                                                        "album" => rows.sort_by(|a, b| a.album_title.to_lowercase().cmp(&b.album_title.to_lowercase())),
                                                        "duration" => rows.sort_by(|a, b| b.track.duration_secs.cmp(&a.track.duration_secs)),
                                                        _ => rows.sort_by(|a, b| a.artist_name.to_lowercase().cmp(&b.artist_name.to_lowercase())),
                                                    }

                                                    if rows.is_empty() {
                                                        return view! { <div class=EMPTY>"No matching tracks"</div> }.into_any();
                                                    }

                                                    rows.into_iter()
                                                        .map(|row| {
                                                            let t = row.track.clone();
                                                            let href = format!(
                                                                "/artists/{}/albums/{}",
                                                                row.artist_id,
                                                                row.album_id
                                                            );
                                                            let wanted = t.monitored && !t.acquired;
                                                            view! {
                                                                <div class="px-4 py-2.5 flex items-center gap-3">
                                                                    <div class="flex-1 min-w-0">
                                                                        <div class="text-sm text-zinc-900 dark:text-zinc-100 truncate">{t.title}</div>
                                                                        <div class={cls(MUTED, "text-xs truncate")}>{format!("{} · {}", row.artist_name, row.album_title)}</div>
                                                                    </div>
                                                                    {if t.acquired {
                                                                        view! { <span class="text-[10px] px-1.5 py-px rounded bg-green-500/[.12] text-green-600 dark:text-green-400">"Acquired"</span> }.into_any()
                                                                    } else if wanted {
                                                                        view! { <span class="text-[10px] px-1.5 py-px rounded bg-amber-500/[.12] text-amber-600 dark:text-amber-300">"Wanted"</span> }.into_any()
                                                                    } else {
                                                                        view! { <span class="text-[10px] px-1.5 py-px rounded bg-zinc-500/[.08] text-zinc-500 dark:text-zinc-400">"Idle"</span> }.into_any()
                                                                    }}
                                                                    <span class={cls(MUTED, "text-xs tabular-nums")}>{t.duration_display}</span>
                                                                    <a href=href class="text-xs text-blue-500 hover:text-blue-400 no-underline">"Open"</a>
                                                                </div>
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
                                                            <h2 class=GLASS_TITLE>"Add Tracks From Providers"</h2>
                                                        </div>
                                                        <div class={cls(GLASS_BODY, "p-0!")}>
                                                            <div class="divide-y divide-black/[.04] dark:divide-white/[.04]">
                                                                {sr.results.into_iter().map(|r| {
                                                                    let loading = RwSignal::new(false);
                                                                    let provider = r.provider.clone();
                                                                    let external_track_id = r.external_id.clone();
                                                                    let external_album_id = r.album_external_id.clone();
                                                                    let artist_external_id = r.artist_external_id.clone();
                                                                    let artist_name = r.artist_name.clone();
                                                                    view! {
                                                                        <div class="px-4 py-3 flex items-center gap-3">
                                                                            <div class="flex-1 min-w-0">
                                                                                <div class="text-sm font-medium text-zinc-900 dark:text-zinc-100 truncate">{r.title}</div>
                                                                                <div class={cls(MUTED, "text-xs truncate")}>{format!("{} · {}", r.artist_name, r.album_title)}</div>
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
                                                        </div>
                                                    </div>
                                                }.into_any()
                                            }
                                        })
                                    }}
                                </Suspense>
                            </div>
                        }.into_any()
                    }
                })
            }}
        </Transition>
    }
}

#[component]
pub fn LibraryTracksPage() -> impl IntoView {
    set_page_title("Library - Tracks");
    view! {
        <div class="flex min-h-screen">
            <Sidebar active="library-tracks" />
            <div class="ml-[220px] max-md:ml-0 flex-1 min-h-screen overflow-x-hidden">
                <div class=HEADER_BAR>
                    <nav class=BREADCRUMB_NAV aria-label="Breadcrumb"><MobileMenuButton />
                        <a href="/library/artists" class=BREADCRUMB_LINK>"Library"</a>
                        <span class=BREADCRUMB_SEP><lucide_leptos::ChevronRight /></span>
                        <span class=BREADCRUMB_CURRENT>"Tracks"</span>
                    </nav>
                </div>
                <LibraryTracksTab />
            </div>
        </div>
    }
}
