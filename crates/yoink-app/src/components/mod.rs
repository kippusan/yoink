mod album_card;
mod breadcrumb;
mod button;
pub(crate) mod confirm_dialog;
mod edit_artist_dialog;
mod error_panel;
pub(crate) mod link_provider_dialog;
mod page_shell;
mod resolve_artist_dialog;
mod sidebar;
mod sleeve_badge;
pub mod toast;

pub use album_card::{AlbumCard, MonitorToggle};
pub use breadcrumb::{Breadcrumb, BreadcrumbItem};
pub use button::{Button, ButtonSize, ButtonVariant};
pub use confirm_dialog::ConfirmDialog;
pub use edit_artist_dialog::EditArtistDialog;
pub use error_panel::ErrorPanel;
pub use link_provider_dialog::LinkProviderDialog;
pub use page_shell::PageShell;
pub use resolve_artist_dialog::ResolveArtistDialog;
pub use sidebar::{MobileMenuButton, Sidebar, SidebarProvider};
pub use sleeve_badge::{SleeveBadge, SleeveBadgeView};

/// Extract the first character of a name as an uppercase initial, or "?" if empty.
pub fn fallback_initial(name: &str) -> String {
    name.chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_string())
}
