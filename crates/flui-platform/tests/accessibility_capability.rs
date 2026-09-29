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

use accesskit::{Action, ActionRequest, Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate};
use flui_platform::{FakeAccessibility, PlatformAccessibility};

/// A one-node tree published under `id`.
fn tree_update(id: u64, label: &str) -> TreeUpdate {
    let root = NodeId(id);
    let mut node = Node::new(Role::Button);
    node.set_label(label.to_string());
    TreeUpdate {
        nodes: vec![(root, node)],
        tree: Some(TreeInfo::new(root)),
        tree_id: TreeId::ROOT,
        focus: root,
    }
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
