use leptoaster::{ToastBuilder, ToastLevel, ToastPosition, expect_toaster};
use leptos::prelude::*;

use crate::actions::dispatch_action;
use yoink_shared::ServerAction;

/// Dispatch a server action and show a toast on success or failure.
///
/// This is the standard one-liner for the 13 simple fire-and-forget call sites.
/// For actions that also need navigation, use `spawn_local` manually with
/// `expect_toaster()` directly.
pub fn dispatch_with_toast(action: ServerAction, success_msg: &str) {
    dispatch_with_toast_loading(action, success_msg, None);
}

/// Show a consistent error toast when required view context vanished.
pub fn toast_missing_context(entity: &str) {
    let toaster = expect_toaster();
    toaster.toast(
        ToastBuilder::new(format!("{entity} is no longer available"))
            .with_level(ToastLevel::Error)
            .with_position(ToastPosition::BottomRight)
            .with_expiry(Some(8_000)),
    );
}

/// Like `dispatch_with_toast`, but also sets a loading signal to `true` while
/// the async operation is in flight and back to `false` when it completes.
/// Pass `Some(signal)` to enable loading tracking, or `None` to behave like
/// the original `dispatch_with_toast`.
pub fn dispatch_with_toast_loading(
    action: ServerAction,
    success_msg: &str,
    loading: Option<RwSignal<bool>>,
) {
    let toaster = expect_toaster();
    let msg = success_msg.to_string();
    if let Some(l) = loading {
        l.set(true);
    }
    leptos::task::spawn_local(async move {
        match dispatch_action(action).await {
            Ok(()) => toaster.toast(
                ToastBuilder::new(&msg)
                    .with_level(ToastLevel::Success)
                    .with_position(ToastPosition::BottomRight)
                    .with_expiry(Some(4_000)),
            ),
            Err(e) => toaster.toast(
                ToastBuilder::new(format!("Error: {e}"))
                    .with_level(ToastLevel::Error)
                    .with_position(ToastPosition::BottomRight)
                    .with_expiry(Some(8_000)),
            ),
        }
        if let Some(l) = loading {
            l.set(false);
        }
    });
}
