//! `Auto`, no subscriber present: FLUI installs its platform default.
//!
//! Its own binary, with exactly one test that writes the process-global
//! subscriber slot. The slot can only be written once per process, so every
//! scenario in this directory owns a binary and none of them can observe
//! another's state or depend on execution order. This holds under `cargo test`
//! (one process per integration-test file) as well as under `cargo nextest`
//! (one process per test).

mod support;

use flui_log::{LogConfig, SubscriberOwnership, SubscriberPolicy};
use support::HostSubscriber;

#[test]
fn auto_installs_the_platform_default_into_an_empty_slot() {
    let ownership = flui_log::setup(&LogConfig::default(), SubscriberPolicy::Auto)
        .expect("the default directives parse and the slot is empty");

    assert_eq!(ownership.ownership, SubscriberOwnership::Installed);

    // `Installed` has to mean the slot is genuinely taken, not just that the
    // call returned that word. Nothing else can claim it now.
    assert!(
        tracing::subscriber::set_global_default(HostSubscriber::new()).is_err(),
        "`Installed` must mean FLUI's subscriber actually holds the slot"
    );
}
