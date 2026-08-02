//! A refused `Install` is inert: the subscriber already in place keeps working
//! and keeps receiving events.

mod support;

use flui_log::{LogConfig, SetupError, SubscriberPolicy};
use support::HostSubscriber;

#[test]
fn a_refused_install_leaves_the_host_subscriber_receiving_events() {
    let host = HostSubscriber::new();
    tracing::subscriber::set_global_default(host.clone())
        .expect("this binary owns the slot, so the host's install must succeed");

    let error = flui_log::setup(&LogConfig::default(), SubscriberPolicy::Install)
        .expect_err("`Install` cannot take a slot the host owns");
    assert!(matches!(error, SetupError::SubscriberAlreadyInstalled));

    tracing::trace!(target: "host_owned_target", "unchanged by the refused install");
    assert_eq!(
        host.targets(),
        vec!["host_owned_target".to_owned()],
        "a refused install must not disturb the subscriber that holds the slot"
    );
}
