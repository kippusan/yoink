use leptos::prelude::*;
use lucide_leptos::{House, SearchX};

use crate::components::{MobileMenuButton, Sidebar};
use crate::hooks::set_page_title;
use crate::styles::{BTN_PRIMARY, HEADER_BAR, MUTED, cls};

#[component]
pub fn NotFoundPage() -> impl IntoView {
    set_page_title("Not Found");
    view! {
        <div class="flex min-h-screen">
            <Sidebar active="" />
            <div class="ml-[220px] max-md:ml-0 flex-1 min-h-screen overflow-x-hidden">
                // Header
                <div class=HEADER_BAR>
                    <div class="flex items-center gap-2"><MobileMenuButton /><h1 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100 m-0">"Not Found"</h1></div>
                </div>

                // Content
                <div class="flex flex-col items-center justify-center px-6 py-20 text-center">
                    <div class="size-16 rounded-full bg-zinc-100 dark:bg-zinc-800 inline-flex items-center justify-center mb-5 [&_svg]:size-8 [&_svg]:text-zinc-400 dark:[&_svg]:text-zinc-500">
                        <SearchX />
                    </div>
                    <h2 class="text-xl font-bold text-zinc-900 dark:text-zinc-100 mb-2">"Page not found"</h2>
                    <p class={cls(MUTED, "text-sm mb-6 max-w-xs")}>
                        "The page you\u{2019}re looking for doesn\u{2019}t exist or has been moved."
                    </p>
                    <a href="/" class={cls(BTN_PRIMARY, "inline-flex items-center gap-1.5 no-underline")}>
                        <House size=16 />
                        "Back to Dashboard"
                    </a>
                </div>
            </div>
        </div>
    }
}
