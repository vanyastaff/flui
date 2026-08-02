//! `Auto`, subscriber already owned by the application: FLUI installs nothing
//! and the application's subscriber keeps receiving events.

mod support;

use flui_log::{LogConfig, SubscriberOwnership, SubscriberPolicy};
use support::HostSubscriber;

#[test]
fn auto_preserves_a_subscriber_the_application_installed() {
    let host = HostSubscriber::new();
    tracing::subscriber::set_global_default(host.clone())
        .expect("this binary owns the slot, so the host's install must succeed");

    let ownership = flui_log::setup(&LogConfig::default(), SubscriberPolicy::Auto)
        .expect("`Auto` never fails on a taken slot");

    assert_eq!(ownership.ownership, SubscriberOwnership::Unchanged);

    // Ownership is only a claim until an event proves where it landed. The
    // host's subscriber accepts every level, so a `trace!` arriving there is
    // proof FLUI's `info,wgpu=warn` default never replaced it.
    tracing::trace!(target: "host_owned_target", "still routed to the host");

    assert_eq!(
        host.targets(),
        vec!["host_owned_target".to_owned()],
        "the application's subscriber must still be the one receiving events"
    );
}
