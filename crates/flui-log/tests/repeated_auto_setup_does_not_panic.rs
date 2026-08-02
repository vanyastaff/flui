//! Constructing the runtime twice must neither panic nor swap the subscriber.
//! The historical entry points panicked on the second `run_app` in a process.

mod support;

use flui_log::{LogConfig, SubscriberOwnership, SubscriberPolicy};

#[test]
fn the_first_auto_installs_and_every_later_one_leaves_it_unchanged() {
    let first = flui_log::setup(&LogConfig::default(), SubscriberPolicy::Auto)
        .expect("the slot is empty on entry to this binary");
    assert_eq!(first.ownership, SubscriberOwnership::Installed);

    for attempt in 1..=5 {
        let repeat = flui_log::setup(&LogConfig::default(), SubscriberPolicy::Auto)
            .unwrap_or_else(|error| panic!("repeat {attempt} must not fail: {error}"));
        assert_eq!(
            repeat.ownership,
            SubscriberOwnership::Unchanged,
            "repeat {attempt} must preserve the subscriber installed first"
        );
    }
}
