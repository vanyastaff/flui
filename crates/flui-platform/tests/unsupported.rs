//! Every error and outcome that can mean "not here" carries one [`Unsupported`]
//! with its reason (ADR-0151 §4): made from it, it answers it back unchanged,
//! says the reason in its message and names it as its source.

use std::error::Error;

use flui_platform::drag::DragError;
use flui_platform::haptics::HapticOutcome;
use flui_platform::text_store::TextStoreHostError;
use flui_platform::{
    AdmissionError, CursorError, InvalidPreference, PreferenceQueryError, StorageError,
    SubmitError, Unsupported, UnsupportedReason,
};

/// Every reason there is, whether asking again may succeed, and its message.
const ANSWERS: [(UnsupportedReason, bool, &str); 7] = [
    (
        UnsupportedReason::NotOnThisPlatform,
        false,
        "not on this platform",
    ),
    (
        UnsupportedReason::NotOnThisDevice,
        false,
        "not on this device",
    ),
    (
        UnsupportedReason::NotYetOnThisHost,
        false,
        "not yet implemented by this host",
    ),
    (
        UnsupportedReason::NotDeclared {
            what: "android.permission.VIBRATE",
        },
        false,
        "the application does not declare android.permission.VIBRATE",
    ),
    (UnsupportedReason::Unavailable, true, "unavailable for now"),
    (
        UnsupportedReason::NotRegistered,
        false,
        "nothing is registered to provide it",
    ),
    (
        UnsupportedReason::NoWindow,
        false,
        "its window is not there",
    ),
];

/// Every reason there is.
const REASONS: [UnsupportedReason; ANSWERS.len()] = {
    let mut reasons = [UnsupportedReason::NoWindow; ANSWERS.len()];
    let mut at = 0;
    while at < ANSWERS.len() {
        reasons[at] = ANSWERS[at].0;
        at += 1;
    }
    reasons
};

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
    carries(AdmissionError::from, AdmissionError::unsupported);
    assert_eq!(AdmissionError::InvalidGeometry.unsupported(), None);
    carries(
        |unsupported| SubmitError::from(AdmissionError::from(unsupported)),
        SubmitError::unsupported,
    );
    assert_eq!(SubmitError::Closed.unsupported(), None);
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

/// Only a reason that may pass is worth asking again, and each reason says
/// which absence it is: a host that has not implemented what the operating
/// system has does not read as the platform lacking it.
fn each_reason_answers_retry_and_message() {
    for (reason, retryable, message) in ANSWERS {
        let unsupported = Unsupported::new(reason);
        assert_eq!(unsupported.is_retryable(), retryable, "{reason:?}");
        assert_eq!(reason.to_string(), message, "{reason:?}");
        assert_eq!(
            unsupported.to_string(),
            format!("unsupported: {message}"),
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
            ("cursors", cursors),
            ("drags", drags),
            ("storage", storage),
            ("preference queries", preference_queries),
            ("text store hosts", text_store_hosts),
            ("haptic outcomes", haptic_outcomes),
            (
                "each reason answers retry and message",
                each_reason_answers_retry_and_message,
            ),
        ],
    );
}
