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

/// Album glow script — extracts the dominant saturated color from each album
/// cover image and applies it as `--glow-rgb` on the parent `.sleeve`.
/// Uses a MutationObserver to handle dynamically rendered sleeves (SPA nav).
const ALBUM_GLOW: &str = r#"
(() => {
  function hslToRgb(h, s, l) {
    var c = (1 - Math.abs(2*l - 1)) * s;
    var x = c * (1 - Math.abs((h/60) % 2 - 1));
    var m = l - c/2;
    var r, g, b;
    if (h < 60)       { r=c; g=x; b=0; }
    else if (h < 120) { r=x; g=c; b=0; }
    else if (h < 180) { r=0; g=c; b=x; }
    else if (h < 240) { r=0; g=x; b=c; }
    else if (h < 300) { r=x; g=0; b=c; }
    else              { r=c; g=0; b=x; }
    return { r: Math.round((r+m)*255), g: Math.round((g+m)*255), b: Math.round((b+m)*255) };
  }
  function pickGlowColor(img) {
    var canvas = document.createElement('canvas');
    canvas.width = 24; canvas.height = 24;
    var ctx = canvas.getContext('2d', { willReadFrequently: true });
    if (!ctx) return null;
    ctx.drawImage(img, 0, 0, 24, 24);
    var data = ctx.getImageData(0, 0, 24, 24).data;
    var r = 0, g = 0, b = 0, n = 0;
    for (var i = 0; i < data.length; i += 4) {
      var pr = data[i], pg = data[i+1], pb = data[i+2], pa = data[i+3];
      if (pa < 180) continue;
      var max = Math.max(pr, pg, pb), min = Math.min(pr, pg, pb);
      var sat = max === 0 ? 0 : (max - min) / max;
      var lum = (pr + pg + pb) / 3;
      if (lum < 25 || lum > 235 || sat < 0.18) continue;
      r += pr; g += pg; b += pb; n++;
    }
    if (n === 0) return null;
    // Normalize lightness: convert average to HSL, set L to 55%, convert back
    var ar = r/n/255, ag = g/n/255, ab = b/n/255;
    var cmax = Math.max(ar, ag, ab), cmin = Math.min(ar, ag, ab);
    var d = cmax - cmin;
    var h = 0;
    if (d > 0) {
      if (cmax === ar) h = 60 * (((ag - ab)/d) % 6);
      else if (cmax === ag) h = 60 * ((ab - ar)/d + 2);
      else h = 60 * ((ar - ag)/d + 4);
    }
    if (h < 0) h += 360;
    var sl = (cmax + cmin) / 2;
    var ss = d === 0 ? 0 : d / (1 - Math.abs(2*sl - 1));
    // Clamp saturation to keep vibrancy, fix lightness for consistency
    var targetL = 0.55, targetS = Math.min(ss, 0.85);
    if (targetS < 0.3) targetS = 0.3;
    return hslToRgb(h, targetS, targetL);
  }
  function applyGlow(img) {
    var sleeve = img.closest('.sleeve');
    if (!sleeve || sleeve.dataset.glowApplied) return;
    try {
      var c = pickGlowColor(img);
      if (!c) return;
      sleeve.style.setProperty('--glow-rgb', c.r+', '+c.g+', '+c.b);
      sleeve.dataset.glowApplied = '1';
    } catch(_) {}
  }
  function processImg(img) {
    if (img.complete && img.naturalWidth > 0) { applyGlow(img); }
    else { img.addEventListener('load', function(e) { applyGlow(e.currentTarget); }, { once: true }); }
  }
  function init() {
    // Process existing covers
    document.querySelectorAll('.sleeve-cover').forEach(processImg);
    // Watch for new covers added by SPA navigation
    new MutationObserver(function(muts) {
      for (var m of muts) {
        for (var node of m.addedNodes) {
          if (node.nodeType !== 1) continue;
          if (node.classList && node.classList.contains('sleeve-cover')) processImg(node);
          else if (node.querySelectorAll) node.querySelectorAll('.sleeve-cover').forEach(processImg);
        }
      }
    }).observe(document.body, { childList: true, subtree: true });
  }
  if (document.body) init();
  else document.addEventListener('DOMContentLoaded', init);
})();
"#;

/// Hydration bootstrap — loads the WASM module and calls hydrate().
/// We pass the WASM path explicitly because cargo-leptos renames the file
/// from yoink_bg.wasm -> yoink.wasm but doesn't patch the JS reference.
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
                <link rel="icon" type="image/svg+xml" href="/yoink.svg" />
                <script>{THEME_BOOTSTRAP}</script>
                <link rel="stylesheet" href="/pkg/yoink.css" />
                <script type="module">{HYDRATE_SCRIPT}</script>
                <script defer>{ALBUM_GLOW}</script>
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
