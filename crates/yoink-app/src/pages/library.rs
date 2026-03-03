use leptos::prelude::*;

use crate::components::{MobileMenuButton, Sidebar};
use crate::hooks::set_page_title;
use crate::styles::{
    BREADCRUMB_CURRENT, BREADCRUMB_LINK, BREADCRUMB_NAV, BREADCRUMB_SEP, HEADER_BAR,
};

#[component]
pub fn LibraryPage() -> impl IntoView {
    set_page_title("Library - Artists");

    view! {
        <div class="flex min-h-screen">
            <Sidebar active="library-artists" />
            <div class="ml-[220px] max-md:ml-0 flex-1 min-h-screen overflow-x-hidden">
                <div class=HEADER_BAR>
                    <nav class=BREADCRUMB_NAV aria-label="Breadcrumb"><MobileMenuButton />
                        <a href="/library/artists" class=BREADCRUMB_LINK>"Library"</a>
                        <span class=BREADCRUMB_SEP><lucide_leptos::ChevronRight /></span>
                        <span class=BREADCRUMB_CURRENT>"Artists"</span>
                    </nav>
                </div>
                <crate::pages::artists::ArtistsTabContent show_header=false />
            </div>
        </div>
    }
}
