use leptos::prelude::*;
use lucide_leptos::X;

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum DialogSize {
    Sm,
    #[default]
    Md,
    Lg,
}

impl DialogSize {
    const fn classes(self) -> &'static str {
        match self {
            Self::Sm => "max-w-sm",
            Self::Md => "max-w-md",
            Self::Lg => "max-w-lg",
        }
    }
}

pub const DIALOG_BACKDROP_CLASS: &str = "fixed inset-0 z-[9999] bg-black/40 dark:bg-black/60 backdrop-blur-sm flex items-center justify-center";
const CARD: &str = "bg-white/80 dark:bg-zinc-800/80 backdrop-blur-[16px] border border-black/[.08] dark:border-white/[.1] rounded-xl shadow-xl w-full mx-4";
const HEADER: &str = "px-5 py-4 border-b border-black/[.06] dark:border-white/[.06] flex items-center justify-between gap-3 shrink-0";
const TITLE: &str = "text-[15px] font-semibold text-zinc-900 dark:text-zinc-100 m-0";
const SUBTITLE: &str = "text-[13px] text-zinc-500 dark:text-zinc-400 mt-0.5";
const RESULT_ROW: &str = "flex items-center gap-3 px-4 py-2.5 border-b border-black/[.04] dark:border-white/[.04] transition-[background] duration-[120ms] last:border-b-0 hover:bg-blue-500/[.04] dark:hover:bg-blue-500/[.06]";
const AVATAR: &str = "size-9 rounded-full object-cover border border-blue-500/20 dark:border-blue-500/30 shrink-0 bg-zinc-200 dark:bg-zinc-800";
const FALLBACK_AVATAR: &str = "size-9 rounded-full inline-flex items-center justify-center bg-zinc-200 dark:bg-zinc-800 text-zinc-500 dark:text-zinc-400 font-bold text-sm border border-blue-500/20 dark:border-blue-500/30 shrink-0";
const SECTION_LABEL: &str = "px-5 py-2 text-[11px] uppercase tracking-wider font-semibold text-zinc-400 dark:text-zinc-500 border-b border-black/[.04] dark:border-white/[.04] bg-zinc-50/50 dark:bg-zinc-900/30";

#[component]
pub fn DialogShell(
    open: RwSignal<bool>,
    #[prop(into)] title: String,
    #[prop(optional, into)] subtitle: Option<String>,
    #[prop(optional)] size: DialogSize,
    #[prop(optional, into)] class: String,
    children: Children,
) -> impl IntoView {
    let title_text = StoredValue::new(title);
    let subtitle_text = StoredValue::new(subtitle);

    view! {
        <div
            class=move || crate::cls!(CARD, size.classes(), class.as_str())
            on:click=|ev: leptos::ev::MouseEvent| ev.stop_propagation()
            role="dialog"
            aria-modal="true"
        >
            <div class=HEADER>
                <div class="min-w-0">
                    <h3 class=TITLE>{move || title_text.with_value(|t| t.clone())}</h3>
                    {move || {
                        subtitle_text.with_value(|subtitle| {
                            subtitle
                                .as_ref()
                                .map(|text| view! { <p class=SUBTITLE>{text.clone()}</p> }.into_any())
                                .unwrap_or_else(|| view! { <span></span> }.into_any())
                        })
                    }}
                </div>
                <button
                    type="button"
                    class="inline-flex items-center justify-center size-7 rounded-md text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-200 cursor-pointer bg-transparent border-none p-0 [&_svg]:size-4"
                    on:click=move |_| open.set(false)
                    title="Close"
                >
                    <X />
                </button>
            </div>
            {children()}
        </div>
    }
}

#[component]
pub fn DialogResultRow(children: Children) -> impl IntoView {
    view! {
        <div class=RESULT_ROW>{children()}</div>
    }
}

#[component]
pub fn ArtistAvatar(#[prop(into)] name: String, image_url: Option<String>) -> impl IntoView {
    let fallback = super::fallback_initial(&name);
    let has_image = image_url.is_some();
    let resolved_image_url = image_url.unwrap_or_default();

    view! {
        <Show
            when=move || has_image
            fallback=move || view! { <div class=FALLBACK_AVATAR>{fallback.clone()}</div> }
        >
            <img class=AVATAR src=resolved_image_url.clone() alt="" />
        </Show>
    }
}

#[component]
pub fn DialogSectionLabel(children: Children) -> impl IntoView {
    view! {
        <div class=SECTION_LABEL>{children()}</div>
    }
}
