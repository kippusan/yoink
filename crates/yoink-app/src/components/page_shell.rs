use leptos::prelude::*;

use super::Sidebar;

/// Page layout wrapper — renders the sidebar + main content panel.
///
/// ```rust,ignore
/// <PageShell active="dashboard">
///     // page content goes here
/// </PageShell>
/// ```
#[component]
pub fn PageShell(#[prop(into)] active: String, children: Children) -> impl IntoView {
    view! {
        <div class="flex min-h-screen">
            <Sidebar active=active />
            <div class="ml-[220px] max-md:ml-0 flex-1 min-h-screen overflow-x-hidden">
                {children()}
            </div>
        </div>
    }
}
