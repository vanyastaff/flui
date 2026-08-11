//! The `PlatformAccessibility` capability contract, against the headless fake.
//!
//! These pin the seam's behaviour, not a screen reader's. What CI cannot reach
//! is stated in the Linux bridge issue: there is no AT-SPI session bus here, so
//! a real adapter never activates and no assistive technology ever reads a
//! tree. What *is* checkable is that the capability publishes only while
//! active, that attach/detach reaches a listener, and that inbound actions
//! arrive addressed in the same id space the published tree used — which is the
//! property that makes the round trip work at all.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use accesskit::{Action, ActionRequest, Node, NodeId, Role, Tree, TreeId, TreeUpdate};
use flui_platform::{FakeAccessibility, PlatformAccessibility};

/// A one-node tree published under `id`.
fn tree_update(id: u64, label: &str) -> TreeUpdate {
    let root = NodeId(id);
    let mut node = Node::new(Role::Button);
    node.set_label(label.to_string());
    TreeUpdate {
        nodes: vec![(root, node)],
        tree: Some(Tree::new(root)),
        tree_id: TreeId::ROOT,
        focus: root,
    }
}

/// **The cost contract.** Nothing is published while no assistive technology is
/// attached, so an application that never runs a screen reader pays nothing for
/// the capability existing. A fake that recorded unconditionally would hide the
/// only property worth having here.
#[test]
fn nothing_is_published_while_inactive() {
    let accessibility = FakeAccessibility::new();

    accessibility.publish(tree_update(1, "Submit"));

    assert!(!accessibility.is_active());
    assert_eq!(accessibility.published_count(), 0);
}

#[test]
fn publishing_while_active_records_the_tree() {
    let accessibility = FakeAccessibility::new();
    accessibility.set_active(true);

    accessibility.publish(tree_update(1, "Submit"));

    let published = accessibility.published();
    assert_eq!(published.len(), 1);
    let (_, node) = published[0].nodes.first().expect("one node");
    assert_eq!(node.label(), Some("Submit"));
    assert_eq!(node.role(), Role::Button);
}

/// Attach and detach both reach the listener. A composition root drives
/// semantics assembly off this, so a missed detach leaves assembly running for
/// a screen reader that is gone — a per-frame tree walk charged to nobody.
#[test]
fn activation_changes_reach_the_listener() {
    let accessibility = FakeAccessibility::new();
    let seen: Arc<parking_lot::Mutex<Vec<bool>>> = Arc::new(parking_lot::Mutex::new(Vec::new()));

    let seen_in_listener = Arc::clone(&seen);
    accessibility.set_activation_listener(Arc::new(move |active| {
        seen_in_listener.lock().push(active);
    }));

    accessibility.set_active(true);
    accessibility.set_active(false);

    assert_eq!(*seen.lock(), vec![true, false]);
}

/// A listener that publishes from inside the activation callback must not
/// deadlock against the lock that dispatched it. This is the clone-and-release
/// discipline the real platform paths use, and the shape a composition root
/// will actually write: "attached — send the current tree now."
#[test]
fn a_listener_that_publishes_does_not_deadlock() {
    let accessibility = Arc::new(FakeAccessibility::new());
    let ran = Arc::new(AtomicBool::new(false));

    let inner = Arc::clone(&accessibility);
    let ran_in_listener = Arc::clone(&ran);
    accessibility.set_activation_listener(Arc::new(move |active| {
        if active {
            inner.publish(tree_update(1, "Submit"));
            ran_in_listener.store(true, Ordering::SeqCst);
        }
    }));

    accessibility.set_active(true);

    assert!(ran.load(Ordering::SeqCst), "the listener ran to completion");
    assert_eq!(
        accessibility.published_count(),
        1,
        "and its publish took effect"
    );
}

/// Inbound actions carry the node id from the published tree. That identity is
/// the whole reason the round trip closes without a translation table: the same
/// value a composition root hands to `SemanticsOwner::resolve_action`.
#[test]
fn an_action_arrives_addressed_by_the_published_node_id() {
    let accessibility = FakeAccessibility::new();
    let targets: Arc<parking_lot::Mutex<Vec<(u64, Action)>>> =
        Arc::new(parking_lot::Mutex::new(Vec::new()));

    let targets_in_listener = Arc::clone(&targets);
    accessibility.set_action_listener(Arc::new(move |request: ActionRequest| {
        targets_in_listener
            .lock()
            .push((request.target_node.0, request.action));
    }));

    accessibility.set_active(true);
    accessibility.publish(tree_update(42, "Submit"));

    accessibility.request_action(ActionRequest {
        action: Action::Click,
        target_tree: TreeId::ROOT,
        target_node: NodeId(42),
        data: None,
    });

    assert_eq!(*targets.lock(), vec![(42, Action::Click)]);
}

/// Registering a listener replaces the previous one rather than fanning out.
/// Two live listeners would mean a re-registering composition root silently
/// driving assembly twice.
#[test]
fn registering_a_listener_replaces_the_previous_one() {
    let accessibility = FakeAccessibility::new();
    let first = Arc::new(AtomicUsize::new(0));
    let second = Arc::new(AtomicUsize::new(0));

    let first_counter = Arc::clone(&first);
    accessibility.set_activation_listener(Arc::new(move |_| {
        first_counter.fetch_add(1, Ordering::SeqCst);
    }));
    let second_counter = Arc::clone(&second);
    accessibility.set_activation_listener(Arc::new(move |_| {
        second_counter.fetch_add(1, Ordering::SeqCst);
    }));

    accessibility.set_active(true);

    assert_eq!(first.load(Ordering::SeqCst), 0, "the replaced listener");
    assert_eq!(second.load(Ordering::SeqCst), 1, "the current listener");
}

/// An action arriving after detach is stale and must be dropped, not delivered.
/// A composition root that stops assembly on detach has no tree to resolve it
/// against, so forwarding would hand it a target it cannot look up.
#[test]
fn an_action_after_detach_is_dropped() {
    let accessibility = FakeAccessibility::new();
    let delivered = Arc::new(AtomicUsize::new(0));

    let counter = Arc::clone(&delivered);
    accessibility.set_action_listener(Arc::new(move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
    }));

    accessibility.set_active(true);
    accessibility.set_active(false);

    accessibility.request_action(ActionRequest {
        action: Action::Click,
        target_tree: TreeId::ROOT,
        target_node: NodeId(42),
        data: None,
    });

    assert_eq!(delivered.load(Ordering::SeqCst), 0);
}

/// Attach/detach is a transition: re-asserting the same state must not
/// re-notify, or a composition root would run redundant enable/disable cycles.
#[test]
fn re_asserting_the_same_activation_state_does_not_re_notify() {
    let accessibility = FakeAccessibility::new();
    let notifications = Arc::new(AtomicUsize::new(0));

    let counter = Arc::clone(&notifications);
    accessibility.set_activation_listener(Arc::new(move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
    }));

    accessibility.set_active(true);
    accessibility.set_active(true);
    accessibility.set_active(true);

    assert_eq!(notifications.load(Ordering::SeqCst), 1);
}
