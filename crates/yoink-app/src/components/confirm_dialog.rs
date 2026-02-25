use leptos::portal::Portal;
use leptos::prelude::*;

// ── Tailwind class constants ────────────────────────────────

const BACKDROP: &str = "fixed inset-0 z-[9999] bg-black/40 dark:bg-black/60 backdrop-blur-sm flex items-center justify-center";
const CARD: &str = "bg-white/80 dark:bg-zinc-800/80 backdrop-blur-[16px] border border-black/[.08] dark:border-white/[.1] rounded-xl shadow-xl p-6 max-w-sm w-full mx-4";
const TITLE: &str = "text-[15px] font-semibold text-zinc-900 dark:text-zinc-100 m-0 mb-2";
const MESSAGE: &str = "text-sm text-zinc-600 dark:text-zinc-400 mb-6 leading-relaxed";
const CHECKBOX_WRAP: &str = "flex items-center gap-2 mb-6 -mt-3";
const CHECKBOX: &str =
    "size-4 rounded border border-black/15 dark:border-white/15 accent-blue-500 cursor-pointer";
const CHECKBOX_LABEL: &str = "text-sm text-zinc-600 dark:text-zinc-400 cursor-pointer select-none";
const BTN_CANCEL: &str = "inline-flex items-center justify-center gap-1.5 px-3.5 py-1.5 bg-white/60 dark:bg-zinc-800/60 backdrop-blur-[8px] border border-black/[.08] dark:border-white/10 rounded-lg font-inherit text-[13px] font-medium cursor-pointer text-zinc-600 dark:text-zinc-300 no-underline transition-all duration-150 whitespace-nowrap hover:bg-white/85 hover:border-zinc-400 dark:hover:bg-zinc-800/85 dark:hover:border-zinc-500";
const BTN_CONFIRM: &str = "inline-flex items-center justify-center gap-1.5 px-3.5 py-1.5 bg-blue-500 backdrop-blur-[8px] border border-blue-500 rounded-lg font-inherit text-[13px] font-medium cursor-pointer text-white no-underline transition-all duration-150 whitespace-nowrap shadow-[0_2px_12px_rgba(59,130,246,.25)] hover:bg-blue-400 hover:border-blue-400 hover:shadow-[0_4px_20px_rgba(59,130,246,.35)]";
const BTN_CONFIRM_DANGER: &str = "inline-flex items-center justify-center gap-1.5 px-3.5 py-1.5 bg-red-500 backdrop-blur-[8px] border border-red-500 rounded-lg font-inherit text-[13px] font-medium cursor-pointer text-white no-underline transition-all duration-150 whitespace-nowrap shadow-[0_2px_12px_rgba(239,68,68,.25)] hover:bg-red-400 hover:border-red-400 hover:shadow-[0_4px_20px_rgba(239,68,68,.35)]";

/// Reusable confirmation dialog with glass-morphism styling.
///
/// Rendered via `<Portal>` into `<body>` to escape stacking contexts from
/// ancestor elements with `backdrop-blur` or `transform`. Locks body scroll
/// while open.
///
/// # Props
/// - `open` — controls visibility (set to `true` to show)
/// - `title` — dialog heading (e.g. "Remove Artist")
/// - `message` — body text explaining the action
/// - `confirm_label` — text on the confirm button (e.g. "Remove")
/// - `danger` — if `true`, confirm button uses red/danger styling
/// - `checkbox_label` — if `Some(...)`, renders a checkbox; its checked state is
///   passed to `on_confirm` as a `bool`
/// - `on_confirm` — called with the checkbox state when the user clicks confirm
///   (always `false` when no checkbox is shown)
///
/// Closes on: Cancel button, backdrop click, or Escape key.
#[component]
pub fn ConfirmDialog(
    open: RwSignal<bool>,
    #[prop(into)] title: String,
    #[prop(into)] message: String,
    #[prop(into)] confirm_label: String,
    #[prop(default = false)] danger: bool,
    #[prop(optional, into)] checkbox_label: Option<String>,
    on_confirm: impl Fn(bool) + 'static + Clone + Send + Sync,
) -> impl IntoView {
    let confirm_class = if danger {
        BTN_CONFIRM_DANGER
    } else {
        BTN_CONFIRM
    };

    // Checkbox state — reset each time the dialog opens
    let checked = RwSignal::new(false);

    // Reset checkbox & lock body scroll when dialog opens/closes
    Effect::new(move || {
        let is_open = open.get();
        if is_open {
            checked.set(false);
        }
        #[cfg(feature = "hydrate")]
        {
            if let Some(body) = leptos::prelude::document().body() {
                let style = body.style();
                if is_open {
                    let _ = style.set_property("overflow", "hidden");
                } else {
                    let _ = style.remove_property("overflow");
                }
            }
        }
    });

    // Close on Escape key
    let close_on_escape = move |ev: leptos::ev::KeyboardEvent| {
        if ev.key() == "Escape" {
            open.set(false);
        }
    };

    // Close when clicking the backdrop.
    // The inner card has stopPropagation, so only actual backdrop clicks arrive here.
    let close_on_backdrop = move |_: leptos::ev::MouseEvent| {
        open.set(false);
    };

    // Store the callback in a StoredValue so it can be called from Fn closures
    let on_confirm = StoredValue::new(on_confirm);

    let on_confirm_click = move |_: leptos::ev::MouseEvent| {
        let val = checked.get_untracked();
        on_confirm.with_value(|f| f(val));
        open.set(false);
    };

    let on_cancel_click = move |_: leptos::ev::MouseEvent| {
        open.set(false);
    };

    // Store string values so they can be used in nested Fn closures
    let title = StoredValue::new(title);
    let message = StoredValue::new(message);
    let confirm_label = StoredValue::new(confirm_label);
    let checkbox_label = StoredValue::new(checkbox_label);

    view! {
        <Portal>
            <Show when=move || open.get()>
                <div
                    class=BACKDROP
                    on:click=close_on_backdrop
                    on:keydown=close_on_escape
                    tabindex="-1"
                >
                    <div class=CARD on:click=|ev: leptos::ev::MouseEvent| ev.stop_propagation()>
                        <h3 class=TITLE>{title.with_value(|t| t.clone())}</h3>
                        <p class=MESSAGE>{message.with_value(|m| m.clone())}</p>
                        {checkbox_label.with_value(|cb| cb.clone()).map(|label| view! {
                            <label class=CHECKBOX_WRAP>
                                <input
                                    type="checkbox"
                                    class=CHECKBOX
                                    prop:checked=move || checked.get()
                                    on:change=move |ev| {
                                        let val = event_target_checked(&ev);
                                        checked.set(val);
                                    }
                                />
                                <span class=CHECKBOX_LABEL>{label}</span>
                            </label>
                        })}
                        <div class="flex justify-end gap-2">
                            <button type="button" class=BTN_CANCEL on:click=on_cancel_click>
                                "Cancel"
                            </button>
                            <button type="button" class=confirm_class on:click=on_confirm_click>
                                {confirm_label.with_value(|c| c.clone())}
                            </button>
                        </div>
                    </div>
                </div>
            </Show>
        </Portal>
    }
}
