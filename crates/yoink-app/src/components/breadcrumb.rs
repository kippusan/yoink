use leptos::prelude::*;
use lucide_leptos::ChevronRight;

use crate::styles::{
    BREADCRUMB_CURRENT, BREADCRUMB_LINK, BREADCRUMB_NAV, BREADCRUMB_SEP, HEADER_BAR,
};

use super::MobileMenuButton;

/// A single breadcrumb segment.
pub struct BreadcrumbItem {
    pub label: String,
    pub href: Option<String>,
}

impl BreadcrumbItem {
    pub fn link(label: impl Into<String>, href: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            href: Some(href.into()),
        }
    }
    pub fn current(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            href: None,
        }
    }
}

/// Renders a sticky header bar with breadcrumb navigation.
///
/// ```rust,ignore
/// <Breadcrumb items=vec![
///     BreadcrumbItem::link("Library", "/library"),
///     BreadcrumbItem::link("Pendulum", "/artists/42"),
///     BreadcrumbItem::current("Immersion"),
/// ] />
/// ```
#[component]
pub fn Breadcrumb(items: Vec<BreadcrumbItem>) -> impl IntoView {
    view! {
        <div class=HEADER_BAR>
            <nav class=BREADCRUMB_NAV aria-label="Breadcrumb"><MobileMenuButton />
                {items.into_iter().enumerate().map(|(i, item)| {
                    let sep = if i > 0 {
                        Some(view! { <span class=BREADCRUMB_SEP><ChevronRight /></span> })
                    } else {
                        None
                    };
                    let segment = match item.href {
                        Some(href) => view! {
                            <a href=href class=BREADCRUMB_LINK>{item.label}</a>
                        }.into_any(),
                        None => view! {
                            <span class=BREADCRUMB_CURRENT>{item.label}</span>
                        }.into_any(),
                    };
                    view! { <>{sep}{segment}</> }
                }).collect_view()}
            </nav>
        </div>
    }
}
