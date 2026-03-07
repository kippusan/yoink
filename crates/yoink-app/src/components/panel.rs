use leptos::prelude::*;

use crate::styles::{GLASS, GLASS_BODY, GLASS_HEADER, GLASS_TITLE};

#[component]
pub fn Panel(#[prop(optional, into)] class: String, children: Children) -> impl IntoView {
    view! {
        <div class=move || crate::cls!(GLASS, class.as_str())>{children()}</div>
    }
}

#[component]
pub fn PanelHeader(#[prop(optional, into)] class: String, children: Children) -> impl IntoView {
    view! {
        <div class=move || crate::cls!(GLASS_HEADER, class.as_str())>{children()}</div>
    }
}

#[component]
pub fn PanelTitle(#[prop(optional, into)] class: String, children: Children) -> impl IntoView {
    view! {
        <h2 class=move || crate::cls!(GLASS_TITLE, class.as_str())>{children()}</h2>
    }
}

#[component]
pub fn PanelBody(#[prop(optional, into)] class: String, children: Children) -> impl IntoView {
    view! {
        <div class=move || crate::cls!(GLASS_BODY, class.as_str())>{children()}</div>
    }
}
