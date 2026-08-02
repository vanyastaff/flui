//! `Inherit` changes nothing — not even when the slot is empty and installing
//! would have "worked". A host that has not installed a subscriber *yet* has
//! not delegated the decision to FLUI.

mod support;

use flui_log::{LogConfig, SubscriberOwnership, SubscriberPolicy};
use support::HostSubscriber;

#[test]
fn inherit_leaves_an_empty_slot_empty_however_many_times_it_runs() {
    // Repeated on purpose: an embedded host may construct FLUI many times in
    // one process, and none of those constructions may consume the slot.
    for attempt in 1..=5 {
        let ownership = flui_log::setup(&LogConfig::default(), SubscriberPolicy::Inherit)
            .unwrap_or_else(|error| panic!("`Inherit` cannot fail (attempt {attempt}): {error}"));
        assert_eq!(ownership, SubscriberOwnership::Inherited);
    }

    // The proof: the host can still claim the slot afterwards. If any of those
    // calls had installed something, this would fail.
    let host = HostSubscriber::new();
    tracing::subscriber::set_global_default(host.clone())
        .expect("`Inherit` must leave the slot free for the host to claim");

    tracing::info!(target: "host_owned_target", "the host owns logging");
    assert_eq!(host.targets(), vec!["host_owned_target".to_owned()]);
}
