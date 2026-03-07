//! Transfer state detection helpers for slskd downloads.

use super::models::Transfer;

fn state_text(t: &Transfer) -> String {
    t.state
        .as_deref()
        .or(t.state_description.as_deref())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

const FAILURE_KEYWORDS: &[&str] = &[
    "rejected",
    "failed",
    "cancel",
    "aborted",
    "timed out",
    "timeout",
    "errored",
    "denied",
];

pub(crate) fn is_failure(t: &Transfer) -> bool {
    let s = state_text(t);
    FAILURE_KEYWORDS.iter().any(|kw| s.contains(kw))
}

pub(crate) fn is_complete_success(t: &Transfer) -> bool {
    if is_failure(t) {
        return false;
    }

    let s = state_text(t);
    if s.contains("completed") || s.contains("complete") || s.contains("succeeded") {
        return true;
    }

    matches!(
        (t.size, t.bytes_transferred, t.bytes_remaining),
        (Some(total), Some(done), Some(remaining))
            if total > 0 && remaining <= 0 && done >= total
    )
}
