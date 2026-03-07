use leptos::prelude::*;

use super::MobileMenuButton;
use crate::styles::HEADER_BAR;

#[component]
pub fn PageHeader(
    #[prop(into)] title: String,
    #[prop(optional, into)] subtitle: Option<String>,
) -> impl IntoView {
    let subtitle_view = if let Some(subtitle_text) = subtitle {
        view! {
            <span class="text-[13px] text-zinc-500 dark:text-zinc-400 truncate">
                {subtitle_text}
            </span>
        }
        .into_any()
    } else {
        view! { <span></span> }.into_any()
    };

    view! {
        <div class=HEADER_BAR>
            <div class="flex items-center gap-3 min-w-0">
                <MobileMenuButton />
                <h1 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100 m-0 truncate">
                    {title}
                </h1>
                {subtitle_view}
            </div>
        </div>
    }
}
