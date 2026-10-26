//! Every error and outcome that can mean "not here" carries one [`Unsupported`]
//! with its reason (ADR-0151 §4): made from it, it answers it back unchanged,
//! says the reason in its message and names it as its source.

use std::error::Error;

use flui_platform::drag::DragError;
use flui_platform::haptics::HapticOutcome;
use flui_platform::text_store::TextStoreHostError;
use flui_platform::{
    CursorError, InvalidPreference, PreferenceQueryError, StorageError, Unsupported,
    UnsupportedReason, WindowCommandError, WindowShowError,
};

/// Every reason there is.
const REASONS: [UnsupportedReason; 6] = [
    UnsupportedReason::NotOnThisPlatform,
    UnsupportedReason::NotOnThisDevice,
    UnsupportedReason::NotDeclared {
        what: "android.permission.VIBRATE",
    },
    UnsupportedReason::Unavailable,
    UnsupportedReason::NotRegistered,
    UnsupportedReason::NoWindow,
];

/// `make` builds the error from each reason's [`Unsupported`], and the error
/// answers that value back, says its reason and names it as its source.
fn carries<E: Error + 'static>(make: fn(Unsupported) -> E, read: fn(&E) -> Option<Unsupported>) {
    for reason in REASONS {
        let unsupported = Unsupported::new(reason);
        let error = make(unsupported);
        assert_eq!(read(&error), Some(unsupported), "{error:?}");
        assert!(
            error.to_string().contains(&reason.to_string()),
            "{error} does not say {reason}"
        );
        assert_eq!(
            error
                .source()
                .and_then(|source| source.downcast_ref::<Unsupported>()),
            Some(&unsupported),
            "{error:?} names no source"
        );
    }
}

fn window_commands() {
    carries(WindowCommandError::from, WindowCommandError::unsupported);
    assert_eq!(WindowCommandError::Closed.unsupported(), None);
    assert_eq!(WindowCommandError::InvalidGeometry.unsupported(), None);
}

fn showing_a_window() {
    carries(WindowShowError::from, WindowShowError::unsupported);
    assert_eq!(WindowShowError::Closed.unsupported(), None);
}

fn cursors() {
    carries(CursorError::from, CursorError::unsupported);
    assert_eq!(CursorError::Backend("refused".into()).unsupported(), None);
}

fn drags() {
    carries(DragError::from, DragError::unsupported);
    assert_eq!(DragError::SessionClosed.unsupported(), None);
}

fn storage() {
    carries(StorageError::from, StorageError::unsupported);
    assert_eq!(StorageError::Busy.unsupported(), None);
}

fn preference_queries() {
    carries(
        PreferenceQueryError::from,
        PreferenceQueryError::unsupported,
    );
    assert_eq!(PreferenceQueryError::WrongThread.unsupported(), None);
    assert_eq!(
        PreferenceQueryError::from(InvalidPreference::GestureArea).unsupported(),
        None
    );
}

fn text_store_hosts() {
    carries(TextStoreHostError::from, TextStoreHostError::unsupported);
    assert_eq!(TextStoreHostError::NotFocused.unsupported(), None);
}

/// A haptic outcome is no error, but its absence is the same value: nothing
/// played, and the reason is the one it was made with.
fn haptic_outcomes() {
    for reason in REASONS {
        let unsupported = Unsupported::new(reason);
        let outcome = HapticOutcome::from(unsupported);
        assert_eq!(outcome.unsupported(), Some(unsupported));
        assert!(!outcome.played(), "{outcome:?} played");
    }
    assert_eq!(HapticOutcome::Submitted.unsupported(), None);
    assert_eq!(HapticOutcome::Suppressed.unsupported(), None);
}

/// Only a reason that may pass is worth asking again.
fn only_unavailable_is_retryable() {
    for reason in REASONS {
        assert_eq!(
            Unsupported::new(reason).is_retryable(),
            reason == UnsupportedReason::Unavailable,
            "{reason:?}"
        );
    }
}

#[test]
fn every_error_carries_its_reason() {
    crate::run_table(
        "every_error_carries_its_reason",
        &[
            ("window commands", window_commands),
            ("showing a window", showing_a_window),
            ("cursors", cursors),
            ("drags", drags),
            ("storage", storage),
            ("preference queries", preference_queries),
            ("text store hosts", text_store_hosts),
            ("haptic outcomes", haptic_outcomes),
            (
                "only unavailable is retryable",
                only_unavailable_is_retryable,
            ),
        ],
    );
}
