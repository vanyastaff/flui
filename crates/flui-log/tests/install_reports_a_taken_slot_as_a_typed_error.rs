//! `Install` demands the slot. When it is taken the caller gets a typed error,
//! not a panic — the historical `Logger::init` called `expect` here.

mod support;

use flui_log::{LogConfig, SetupError, SubscriberOwnership, SubscriberPolicy};

#[test]
fn install_succeeds_once_then_reports_the_slot_as_taken() {
    // First: an empty slot, so the demand is satisfiable.
    let ownership = flui_log::setup(&LogConfig::default(), SubscriberPolicy::Install)
        .expect("the slot is empty on entry to this binary");
    assert_eq!(ownership, SubscriberOwnership::Installed);

    // Second: the slot is now taken, and `Install` must say so as a value.
    let error = flui_log::setup(&LogConfig::default(), SubscriberPolicy::Install)
        .expect_err("`Install` cannot take a slot that is already owned");

    assert!(
        matches!(error, SetupError::SubscriberAlreadyInstalled),
        "expected `SubscriberAlreadyInstalled`, got {error:?}"
    );

    // The message is what an author sees, so it has to name the way out.
    let message = error.to_string();
    assert!(
        message.contains("SubscriberPolicy::Auto") && message.contains("SubscriberPolicy::Inherit"),
        "the error must name the policies that would have worked; got {message:?}"
    );
}
