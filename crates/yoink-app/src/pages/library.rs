use leptos::prelude::*;

use crate::components::{Breadcrumb, BreadcrumbItem, PageShell};
use crate::hooks::set_page_title;

#[component]
pub fn LibraryPage() -> impl IntoView {
    set_page_title("Library - Artists");

    view! {
        <PageShell active="library-artists">
                <Breadcrumb items=vec![
                    BreadcrumbItem::link("Library", "/library/artists"),
                    BreadcrumbItem::current("Artists"),
                ] />
                <crate::pages::artists::ArtistsTabContent show_header=false />
        </PageShell>
    }
}
