//! Announcements through the owner's two announcer nodes (ADR-0204 §4).

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use flui_foundation::RenderId;
use flui_semantics::{
    AnnounceError, Announcements, SemanticsIdentitySource, SemanticsNode, SemanticsOwner,
    TreeUpdate, Urgency,
};
use parking_lot::Mutex;

fn root() -> SemanticsNode {
    let mut node = SemanticsNode::new().with_source_render_id(RenderId::new_gen(
        0,
        core::num::NonZeroU32::new(7).expect("fixture generation is non-zero"),
    ));
    node.config_mut().set_label("Window");
    node
}

fn announcements() -> Announcements {
    let ids = SemanticsIdentitySource::default()
        .allocate(2)
        .expect("a fresh source has identities");
    Announcements::new(ids[0], ids[1])
}

/// An owner rooted and published once, its announcer nodes delivered, with
/// the updates it delivers and a switch that makes the next deliveries panic.
fn owner_with(
    announcements: &Announcements,
) -> (SemanticsOwner, Arc<Mutex<Vec<TreeUpdate>>>, Arc<AtomicBool>) {
    let received: Arc<Mutex<Vec<TreeUpdate>>> = Arc::default();
    let failing = Arc::new(AtomicBool::new(false));
    let sink = Arc::clone(&received);
    let fail = Arc::clone(&failing);
    let mut owner = SemanticsOwner::new(Arc::new(move |update: &TreeUpdate| {
        assert!(
            !fail.load(Ordering::SeqCst),
            "the adapter rejects the update"
        );
        sink.lock().push(update.clone());
    }));
    let root = owner.insert(root());
    owner.set_root(Some(root));
    owner.set_announcements(announcements.clone());
    owner.flush();
    (owner, received, failing)
}

/// The value the last delivered update gave the node of `urgency`.
fn delivered_value(
    received: &Mutex<Vec<TreeUpdate>>,
    announcements: &Announcements,
    urgency: Urgency,
) -> Option<String> {
    let id = accesskit::NodeId(announcements.node_id(urgency).as_u64());
    received
        .lock()
        .last()?
        .nodes
        .iter()
        .find(|(node, _)| *node == id)
        .and_then(|(_, node)| node.value().map(str::to_owned))
}

/// The announcer nodes are the last children of the root, live at their
/// urgency, and an announcement reaches the matching node's value through
/// an incremental update that carries nothing else.
#[test]
fn announcements_reach_the_live_region_value() {
    let announcements = announcements();
    let (mut owner, received, _) = owner_with(&announcements);
    let polite = accesskit::NodeId(announcements.node_id(Urgency::Polite).as_u64());
    let assertive = accesskit::NodeId(announcements.node_id(Urgency::Assertive).as_u64());
    {
        let first = &received.lock()[0];
        let root_id = first.tree.as_ref().expect("a full update").root;
        let (_, root) = first
            .nodes
            .iter()
            .find(|(id, _)| *id == root_id)
            .expect("the root is published");
        assert_eq!(
            &root.children()[root.children().len() - 2..],
            &[polite, assertive]
        );
        for (id, live) in [
            (polite, accesskit::Live::Polite),
            (assertive, accesskit::Live::Assertive),
        ] {
            let (_, node) = first
                .nodes
                .iter()
                .find(|(node, _)| *node == id)
                .expect("each announcer node is published");
            assert_eq!(node.role(), accesskit::Role::Label);
            assert_eq!(node.live(), Some(live));
        }
    }

    owner
        .announce("Message sent", Urgency::Polite)
        .expect("an installed queue accepts");
    assert!(
        owner.needs_flush(),
        "an announcement is work for the next flush"
    );
    owner.flush();
    assert_eq!(
        delivered_value(&received, &announcements, Urgency::Polite).as_deref(),
        Some("Message sent")
    );
    assert_eq!(
        received.lock().last().expect("an update").nodes.len(),
        1,
        "only the polite node changed"
    );
    assert!(
        !owner.needs_flush(),
        "a delivered announcement leaves the queue"
    );

    owner
        .announce("Connection lost", Urgency::Assertive)
        .expect("an installed queue accepts");
    owner.flush();
    assert_eq!(
        delivered_value(&received, &announcements, Urgency::Assertive).as_deref(),
        Some("Connection lost")
    );
}

/// Messages of one frame are spoken together, and a message equal to the one
/// the node shows still changes its value, so the adapter announces it again.
#[test]
fn a_repeated_announcement_changes_the_value() {
    let announcements = announcements();
    let (mut owner, received, _) = owner_with(&announcements);
    owner.announce("Saved", Urgency::Polite).expect("accepted");
    owner
        .announce("3 items", Urgency::Polite)
        .expect("accepted");
    owner.flush();
    assert_eq!(
        delivered_value(&received, &announcements, Urgency::Polite).as_deref(),
        Some("Saved\n3 items")
    );

    let mut values = Vec::new();
    for _ in 0..3 {
        owner.announce("Saved", Urgency::Polite).expect("accepted");
        owner.flush();
        values.push(delivered_value(&received, &announcements, Urgency::Polite));
    }
    assert_eq!(values.len(), 3);
    assert_ne!(values[0], values[1], "a repeat is a change");
    assert_ne!(values[1], values[2], "a second repeat is a change too");
    for value in values.iter().flatten() {
        assert_eq!(value.trim_end_matches('\u{200B}'), "Saved");
    }
}

/// A delivery that fails keeps the message queued, and the next flush
/// delivers it; a message accepted while the update was out is not cleared
/// by that update's success.
#[test]
fn a_failed_delivery_keeps_the_announcement() {
    let announcements = announcements();
    let (mut owner, received, failing) = owner_with(&announcements);
    owner
        .announce("Uploaded", Urgency::Polite)
        .expect("accepted");
    failing.store(true, Ordering::SeqCst);
    let failed = catch_unwind(AssertUnwindSafe(|| owner.flush()));
    assert!(failed.is_err(), "the adapter panicked");
    failing.store(false, Ordering::SeqCst);
    assert!(owner.needs_flush(), "the message waits for a delivery");
    owner.flush();
    assert_eq!(
        delivered_value(&received, &announcements, Urgency::Polite).as_deref(),
        Some("Uploaded")
    );
}

/// The handle queues before it requests the frame, so the frame it requests
/// finds the message; with no one listening it drops messages and requests
/// nothing; once the presentation is gone it refuses.
#[test]
fn an_announcer_queues_then_requests_a_frame() {
    let announcements = announcements();
    let (mut owner, received, _) = owner_with(&announcements);
    let requests = Arc::new(AtomicUsize::new(0));
    let announcer = announcements.announcer({
        let requests = Arc::clone(&requests);
        Arc::new(move || {
            requests.fetch_add(1, Ordering::SeqCst);
        })
    });
    announcer
        .announce("Done", Urgency::Polite)
        .expect("an open presentation accepts");
    assert_eq!(requests.load(Ordering::SeqCst), 1);
    assert!(owner.needs_flush(), "queued before the frame was requested");
    owner.flush();
    assert_eq!(
        delivered_value(&received, &announcements, Urgency::Polite).as_deref(),
        Some("Done")
    );

    owner.disable();
    announcer
        .announce("Nobody hears this", Urgency::Polite)
        .expect("accepted while no one listens");
    assert_eq!(
        requests.load(Ordering::SeqCst),
        1,
        "no frame for no listener"
    );
    owner.enable();
    assert!(
        !owner.needs_flush(),
        "a screen reader that connects later is not read a backlog"
    );

    drop(owner);
    drop(announcements);
    assert_eq!(
        announcer.announce("Too late", Urgency::Polite),
        Err(AnnounceError::PresentationClosed)
    );
}

/// An agent reads and locates the interface, not the announcer nodes: they
/// are published to assistive technology alone.
#[test]
fn an_agent_neither_reads_nor_locates_the_announcer_nodes() {
    let announcements = announcements();
    let (mut owner, _, _) = owner_with(&announcements);
    owner
        .announce("Message sent", Urgency::Polite)
        .expect("an installed queue accepts");
    owner.flush();
    let tree = owner
        .read_wire(
            &flui_protocol::ReadQuery::new(),
            crate::fixture::placement(),
        )
        .expect("the tree reads");
    assert_eq!(tree.roots.len(), 1);
    assert!(
        tree.roots[0].children.is_empty(),
        "the window shows no announcer node"
    );
    let announcer = owner
        .locate_wire(
            &flui_protocol::LocateParams::new(flui_protocol::Locator::identifier(
                Urgency::Polite.announcer_identifier(),
            )),
            crate::fixture::placement(),
        )
        .expect("the tree reads");
    assert!(
        announcer.matches.is_empty(),
        "no locator reaches an announcer node"
    );
}
