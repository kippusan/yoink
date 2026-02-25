use leptoaster::{ToastBuilder, ToastLevel, ToastPosition, expect_toaster};

use crate::actions::dispatch_action;
use yoink_shared::ServerAction;

/// Dispatch a server action and show a toast on success or failure.
///
/// This is the standard one-liner for the 13 simple fire-and-forget call sites.
/// For actions that also need navigation, use `spawn_local` manually with
/// `expect_toaster()` directly.
pub fn dispatch_with_toast(action: ServerAction, success_msg: &str) {
    let toaster = expect_toaster();
    let msg = success_msg.to_string();
    leptos::task::spawn_local(async move {
        match dispatch_action(action).await {
            Ok(()) => toaster.toast(
                ToastBuilder::new(&msg)
                    .with_level(ToastLevel::Success)
                    .with_position(ToastPosition::BottomRight)
                    .with_expiry(Some(4_000)),
            ),
            Err(e) => toaster.toast(
                ToastBuilder::new(&format!("Error: {e}"))
                    .with_level(ToastLevel::Error)
                    .with_position(ToastPosition::BottomRight)
                    .with_expiry(Some(8_000)),
            ),
        }
    });
}
