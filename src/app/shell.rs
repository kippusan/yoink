use leptos::prelude::*;

/// Theme bootstrap script — runs before paint to avoid FOUC.
/// Sets the `dark` class on `<html>` from localStorage / prefers-color-scheme.
const THEME_BOOTSTRAP: &str = r#"
(() => {
  const stored = localStorage.getItem('theme');
  const prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
  if (stored === 'dark' || (!stored && prefersDark)) {
    document.documentElement.classList.add('dark');
  } else {
    document.documentElement.classList.remove('dark');
  }
})();
"#;

/// Hydration bootstrap — loads the WASM module and calls hydrate().
/// We pass the WASM path explicitly because cargo-leptos renames the file
/// from yoink_bg.wasm → yoink.wasm but doesn't patch the JS reference.
const HYDRATE_SCRIPT: &str = r#"
import init, { hydrate } from '/pkg/yoink.js';
await init({ module_or_path: '/pkg/yoink.wasm' });
hydrate();
"#;

/// The HTML shell rendered around every Leptos page (server-side only).
///
/// This is a plain function, not a `#[component]`, because it produces the
/// full HTML document (`<!DOCTYPE>`, `<html>`, `<head>`, `<body>`) which is
/// NOT part of the hydrated tree. `hydrate_body(App)` only hydrates what's
/// inside `<body>`, i.e. the `<App/>` component.
pub fn shell() -> impl IntoView {
    view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8" />
                <meta name="viewport" content="width=device-width, initial-scale=1" />
                <title>"yoink"</title>
                <script>{THEME_BOOTSTRAP}</script>
                <link rel="stylesheet" href="/pkg/yoink.css" />
                <script type="module">{HYDRATE_SCRIPT}</script>
            </head>
            <body class="min-h-screen bg-zinc-100 text-zinc-900 dark:bg-zinc-950 dark:text-zinc-100">
                <div id="app">
                    <App />
                </div>
            </body>
        </html>
    }
}

use super::App;
