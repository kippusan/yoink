use wasm_bindgen::JsCast;

#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    use yoink_app::App;

    // Hydrate against the #app container, not <body>, because the SSR
    // out-of-order streaming appends <template>/<script> nodes to <body>
    // that would cause a hydration mismatch.
    let app_el: web_sys::HtmlElement = leptos::prelude::document()
        .get_element_by_id("app")
        .expect("missing #app element")
        .unchecked_into();
    leptos::mount::hydrate_from(app_el, App).forget();
}
