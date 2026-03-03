use leptos::portal::Portal;
use leptos::prelude::*;
use yoink_shared::{ArtistImageOption, ServerAction, Uuid, provider_display_name};

use crate::components::toast::dispatch_with_toast_loading;
use crate::pages::artist_detail::get_artist_images;

// ── Tailwind class constants ────────────────────────────────

const BACKDROP: &str = "fixed inset-0 z-[9999] bg-black/40 dark:bg-black/60 backdrop-blur-sm flex items-center justify-center";
const CARD: &str = "bg-white/80 dark:bg-zinc-800/80 backdrop-blur-[16px] border border-black/[.08] dark:border-white/[.1] rounded-xl shadow-xl p-6 max-w-md w-full mx-4 max-h-[85vh] overflow-y-auto";
const TITLE: &str = "text-[15px] font-semibold text-zinc-900 dark:text-zinc-100 m-0 mb-4";
const LABEL: &str = "block text-[13px] font-medium text-zinc-700 dark:text-zinc-300 mb-1.5";
const INPUT: &str = "w-full px-3 py-2 text-sm bg-white/60 dark:bg-zinc-800/60 border border-black/[.08] dark:border-white/10 rounded-lg text-zinc-900 dark:text-zinc-100 outline-none transition-[border-color,box-shadow] duration-150 focus:border-blue-500 focus:shadow-[0_0_0_2px_rgba(59,130,246,.12)]";
const BTN_CANCEL: &str = "inline-flex items-center justify-center gap-1.5 px-3.5 py-1.5 bg-white/60 dark:bg-zinc-800/60 backdrop-blur-[8px] border border-black/[.08] dark:border-white/10 rounded-lg font-inherit text-[13px] font-medium cursor-pointer text-zinc-600 dark:text-zinc-300 no-underline transition-all duration-150 whitespace-nowrap hover:bg-white/85 hover:border-zinc-400 dark:hover:bg-zinc-800/85 dark:hover:border-zinc-500";
const BTN_SAVE: &str = "inline-flex items-center justify-center gap-1.5 px-3.5 py-1.5 bg-blue-500 backdrop-blur-[8px] border border-blue-500 rounded-lg font-inherit text-[13px] font-medium cursor-pointer text-white no-underline transition-all duration-150 whitespace-nowrap shadow-[0_2px_12px_rgba(59,130,246,.25)] hover:bg-blue-400 hover:border-blue-400 hover:shadow-[0_4px_20px_rgba(59,130,246,.35)]";
const BTN_SECONDARY: &str = "inline-flex items-center justify-center gap-1.5 px-3 py-1.5 bg-white/60 dark:bg-zinc-800/60 backdrop-blur-[8px] border border-black/[.08] dark:border-white/10 rounded-lg font-inherit text-[12px] font-medium cursor-pointer text-zinc-600 dark:text-zinc-300 no-underline transition-all duration-150 whitespace-nowrap hover:bg-white/85 hover:border-blue-500/20 dark:hover:bg-zinc-800/85 dark:hover:border-blue-500/30";

const IMG_OPTION: &str =
    "relative cursor-pointer rounded-xl overflow-hidden border-2 transition-all duration-150";
const IMG_OPTION_SELECTED: &str = "border-blue-500 shadow-[0_0_0_2px_rgba(59,130,246,.25)]";
const IMG_OPTION_UNSELECTED: &str = "border-transparent hover:border-blue-500/30";
const IMG_THUMB: &str = "size-20 object-cover bg-zinc-200 dark:bg-zinc-800";

/// Source of the selected image.
#[derive(Clone, PartialEq)]
enum ImageSource {
    /// A provider image (index into the provider images list).
    Provider(usize),
    /// The current image (keep as-is).
    Current,
    /// A custom URL typed by the user.
    Custom,
    /// No image (clear).
    None,
}

/// Dialog for editing artist name, picture (from providers or custom URL), and bio.
#[component]
pub fn EditArtistDialog(
    open: RwSignal<bool>,
    artist_id: Uuid,
    current_name: Signal<String>,
    current_image_url: Signal<Option<String>>,
    has_bio: Signal<bool>,
) -> impl IntoView {
    let name_input = RwSignal::new(String::new());
    let custom_url_input = RwSignal::new(String::new());
    let image_source = RwSignal::new(ImageSource::Current);
    let saving = RwSignal::new(false);
    let fetching_bio = RwSignal::new(false);
    let show_custom_url = RwSignal::new(false);

    // Provider images — fetched when dialog opens
    let provider_images: RwSignal<Vec<ArtistImageOption>> = RwSignal::new(Vec::new());
    let images_loading = RwSignal::new(false);

    let card_ref = NodeRef::<leptos::html::Div>::new();

    // Reset fields and fetch provider images when dialog opens
    Effect::new(move || {
        let is_open = open.get();
        if is_open {
            name_input.set(current_name.get_untracked());
            custom_url_input.set(String::new());
            image_source.set(ImageSource::Current);
            show_custom_url.set(false);
            provider_images.set(Vec::new());

            // Fetch provider images
            images_loading.set(true);
            let aid = artist_id.to_string();
            leptos::task::spawn_local(async move {
                match get_artist_images(aid).await {
                    Ok(images) => provider_images.set(images),
                    Err(_) => provider_images.set(Vec::new()),
                }
                images_loading.set(false);
            });
        }
        #[cfg(feature = "hydrate")]
        {
            use crate::components::confirm_dialog::scroll_lock;
            if is_open {
                scroll_lock::acquire();
            } else {
                scroll_lock::release();
            }
        }
    });

    let close_on_escape = move |ev: leptos::ev::KeyboardEvent| {
        if ev.key() == "Escape" {
            open.set(false);
        }
    };

    let close_on_backdrop = move |_: leptos::ev::MouseEvent| {
        open.set(false);
    };

    // Derive the resolved image URL from the current selection
    let resolved_image_url = move || -> Option<String> {
        match image_source.get() {
            ImageSource::Current => current_image_url.get(),
            ImageSource::Provider(idx) => {
                let imgs = provider_images.get();
                imgs.get(idx).map(|i| i.image_url.clone())
            }
            ImageSource::Custom => {
                let url = custom_url_input.get();
                if url.trim().is_empty() {
                    None
                } else {
                    Some(url.trim().to_string())
                }
            }
            ImageSource::None => None,
        }
    };

    let on_save = move |_: leptos::ev::MouseEvent| {
        let new_name = name_input.get_untracked().trim().to_string();
        let orig_name = current_name.get_untracked();
        let orig_image = current_image_url.get_untracked();
        let source = image_source.get_untracked();

        let name_changed = !new_name.is_empty() && new_name != orig_name;

        // Determine new image URL based on source
        let new_image_url: Option<String> = match source {
            ImageSource::Current => None, // no change
            ImageSource::Provider(idx) => {
                let imgs = provider_images.get_untracked();
                imgs.get(idx).map(|i| i.image_url.clone())
            }
            ImageSource::Custom => {
                let url = custom_url_input.get_untracked().trim().to_string();
                Some(url) // empty = clear
            }
            ImageSource::None => Some(String::new()), // clear
        };

        let image_changed =
            new_image_url.is_some() && new_image_url.as_deref() != orig_image.as_deref();

        if !name_changed && !image_changed {
            open.set(false);
            return;
        }

        let action = ServerAction::UpdateArtist {
            artist_id,
            name: if name_changed { Some(new_name) } else { None },
            image_url: if image_changed { new_image_url } else { None },
        };

        dispatch_with_toast_loading(action, "Artist updated", Some(saving));
        open.set(false);
    };

    let on_fetch_bio = move |_: leptos::ev::MouseEvent| {
        dispatch_with_toast_loading(
            ServerAction::FetchArtistBio { artist_id },
            "Fetching bio from providers\u{2026}",
            Some(fetching_bio),
        );
    };

    view! {
        <Portal>
            <Show when=move || open.get()>
                <div
                    class=BACKDROP
                    on:click=close_on_backdrop
                    on:keydown=close_on_escape
                    tabindex="-1"
                >
                    <div class=CARD on:click=|ev: leptos::ev::MouseEvent| ev.stop_propagation()
                        role="dialog" aria-modal="true" aria-label="Edit Artist"
                        node_ref=card_ref
                    >
                        <h3 class=TITLE>"Edit Artist"</h3>

                        // ── Name field ──────────────────────────
                        <div class="mb-4">
                            <label class=LABEL>"Name"</label>
                            <input
                                type="text"
                                class=INPUT
                                placeholder="Artist name"
                                prop:value=move || name_input.get()
                                on:input=move |ev| name_input.set(event_target_value(&ev))
                            />
                        </div>

                        // ── Image selection ─────────────────────
                        <div class="mb-4">
                            <label class=LABEL>"Picture"</label>

                            // Preview of selected image
                            <div class="flex items-center gap-3 mb-3">
                                {move || {
                                    let url = resolved_image_url();
                                    let name = name_input.get();
                                    let initial = name.chars().next()
                                        .map(|c| c.to_uppercase().to_string())
                                        .unwrap_or_else(|| "?".to_string());
                                    match url {
                                        Some(u) if !u.is_empty() => view! {
                                            <img
                                                class="size-16 rounded-full object-cover border-2 border-blue-500/20 dark:border-blue-500/30 bg-zinc-200 dark:bg-zinc-800 shrink-0"
                                                src=u alt=""
                                            />
                                        }.into_any(),
                                        _ => view! {
                                            <div class="size-16 rounded-full inline-flex items-center justify-center bg-zinc-200 dark:bg-zinc-800 text-zinc-500 dark:text-zinc-400 font-bold text-2xl border-2 border-blue-500/20 dark:border-blue-500/30 shrink-0">
                                                {initial}
                                            </div>
                                        }.into_any(),
                                    }
                                }}
                                <div class="text-[12px] text-zinc-400 dark:text-zinc-500">
                                    {move || match image_source.get() {
                                        ImageSource::Current => "Current image".to_string(),
                                        ImageSource::Provider(idx) => {
                                            let imgs = provider_images.get();
                                            imgs.get(idx)
                                                .map(|i| format!("From {}", provider_display_name(&i.provider)))
                                                .unwrap_or_else(|| "Provider image".to_string())
                                        }
                                        ImageSource::Custom => "Custom URL".to_string(),
                                        ImageSource::None => "No image".to_string(),
                                    }}
                                </div>
                            </div>

                            // Image options grid
                            <div class="flex flex-wrap gap-2 mb-2">
                                // Current image option
                                {move || {
                                    let cur = current_image_url.get();
                                    let is_selected = image_source.get() == ImageSource::Current;
                                    let border_cls = if is_selected { IMG_OPTION_SELECTED } else { IMG_OPTION_UNSELECTED };
                                    match cur {
                                        Some(url) if !url.is_empty() => view! {
                                            <div
                                                class=format!("{IMG_OPTION} {border_cls}")
                                                title="Keep current image"
                                                on:click=move |_| image_source.set(ImageSource::Current)
                                            >
                                                <img class=IMG_THUMB src=url alt="Current" />
                                                <div class="absolute bottom-0 inset-x-0 bg-black/60 text-white text-[10px] text-center py-0.5 font-medium">
                                                    "Current"
                                                </div>
                                            </div>
                                        }.into_any(),
                                        _ => view! { <span></span> }.into_any(),
                                    }
                                }}

                                // Provider images
                                {move || {
                                    let imgs = provider_images.get();
                                    let loading = images_loading.get();

                                    if loading {
                                        return view! {
                                            <div class="flex items-center gap-2 text-[12px] text-zinc-400 dark:text-zinc-500 py-2">
                                                <div class="size-4 border-2 border-blue-500/40 border-t-blue-500 rounded-full animate-spin"></div>
                                                "Loading provider images\u{2026}"
                                            </div>
                                        }.into_any();
                                    }

                                    if imgs.is_empty() {
                                        return view! { <span></span> }.into_any();
                                    }

                                    imgs.into_iter().enumerate().map(|(idx, img)| {
                                        let is_selected = move || image_source.get() == ImageSource::Provider(idx);
                                        let border_cls = move || if is_selected() { IMG_OPTION_SELECTED } else { IMG_OPTION_UNSELECTED };
                                        let provider_name = provider_display_name(&img.provider);
                                        let url = img.image_url.clone();
                                        view! {
                                            <div
                                                class=move || format!("{IMG_OPTION} {}", border_cls())
                                                title=format!("Use image from {provider_name}")
                                                on:click=move |_| image_source.set(ImageSource::Provider(idx))
                                            >
                                                <img class=IMG_THUMB src=url alt=provider_name.clone() />
                                                <div class="absolute bottom-0 inset-x-0 bg-black/60 text-white text-[10px] text-center py-0.5 font-medium">
                                                    {provider_name.clone()}
                                                </div>
                                            </div>
                                        }
                                    }).collect_view().into_any()
                                }}

                                // "No image" option
                                {move || {
                                    let is_selected = image_source.get() == ImageSource::None;
                                    let border_cls = if is_selected { IMG_OPTION_SELECTED } else { IMG_OPTION_UNSELECTED };
                                    view! {
                                        <div
                                            class=format!("{IMG_OPTION} {border_cls}")
                                            title="Remove image"
                                            on:click=move |_| image_source.set(ImageSource::None)
                                        >
                                            <div class="size-20 bg-zinc-100 dark:bg-zinc-800 flex items-center justify-center text-zinc-400 dark:text-zinc-500 text-xl">
                                                "\u{2715}"
                                            </div>
                                            <div class="absolute bottom-0 inset-x-0 bg-black/60 text-white text-[10px] text-center py-0.5 font-medium">
                                                "None"
                                            </div>
                                        </div>
                                    }
                                }}
                            </div>

                            // Custom URL toggle + input
                            <div class="mt-2">
                                {move || {
                                    let showing = show_custom_url.get();
                                    let is_custom = image_source.get() == ImageSource::Custom;
                                    if !showing && !is_custom {
                                        view! {
                                            <button
                                                type="button"
                                                class="text-[12px] font-medium text-blue-500 dark:text-blue-400 hover:text-blue-600 dark:hover:text-blue-300 bg-transparent border-none cursor-pointer p-0"
                                                on:click=move |_| {
                                                    show_custom_url.set(true);
                                                    image_source.set(ImageSource::Custom);
                                                }
                                            >
                                                "Use custom URL instead\u{2026}"
                                            </button>
                                        }.into_any()
                                    } else {
                                        view! {
                                            <div>
                                                <input
                                                    type="url"
                                                    class=INPUT
                                                    placeholder="https://example.com/photo.jpg"
                                                    prop:value=move || custom_url_input.get()
                                                    on:input=move |ev| {
                                                        custom_url_input.set(event_target_value(&ev));
                                                        image_source.set(ImageSource::Custom);
                                                    }
                                                    on:focus=move |_| {
                                                        image_source.set(ImageSource::Custom);
                                                    }
                                                />
                                                <p class="text-[11px] text-zinc-400 dark:text-zinc-500 mt-1 mb-0">
                                                    "Paste a direct image URL."
                                                </p>
                                            </div>
                                        }.into_any()
                                    }
                                }}
                            </div>
                        </div>

                        // ── Bio section ─────────────────────────
                        <div class="mb-5 pb-4 border-t border-black/[.06] dark:border-white/[.06] pt-4">
                            <label class=LABEL>"Bio"</label>
                            <div class="flex items-center gap-2">
                                <button
                                    type="button"
                                    class=BTN_SECONDARY
                                    disabled=move || fetching_bio.get()
                                    on:click=on_fetch_bio
                                >
                                    {move || if fetching_bio.get() {
                                        "Fetching\u{2026}"
                                    } else if has_bio.get() {
                                        "Re-fetch Bio"
                                    } else {
                                        "Fetch Bio"
                                    }}
                                </button>
                                <span class="text-[11px] text-zinc-400 dark:text-zinc-500">
                                    {move || if has_bio.get() {
                                        "Bio already present. Re-fetch to update."
                                    } else {
                                        "Fetches from linked providers (e.g. MusicBrainz)."
                                    }}
                                </span>
                            </div>
                        </div>

                        // ── Action buttons ──────────────────────
                        <div class="flex justify-end gap-2">
                            <button type="button" class=BTN_CANCEL on:click=move |_| open.set(false)>
                                "Cancel"
                            </button>
                            <button
                                type="button"
                                class=BTN_SAVE
                                disabled=move || saving.get()
                                on:click=on_save
                            >
                                {move || if saving.get() { "Saving\u{2026}" } else { "Save" }}
                            </button>
                        </div>
                    </div>
                </div>
            </Show>
        </Portal>
    }
}
