//! The incremental-publish contract of `SemanticsOwner::flush`.
//!
//! The flush serializes only changed nodes into the platform update and
//! returns immediately when nothing is dirty. FLUI rebuilds its semantics arena every assembly pass (flui-semantics ARCHITECTURE.md, semantics assembly),
//! so "dirty" alone cannot distinguish a real change from a rebuild that
//! reproduced the same tree — the flush diffs per-node payloads, keyed by
//! stable [`AccessibilityNodeId`], against the last delivered update. These
//! tests pin the observable halves of that contract:
//!
//! - a pass that changes nothing publishes **nothing**;
//! - a pass that changes one node publishes **that node**, not the tree.
//!
//! Every fixture drives the owner exactly the way the pipeline does
//! (`rebuild_semantics_owner`): clear the arena, reinsert every node, root
//! it, flush. The tests deliberately use only API that predates the diff so
//! they can run — and fail — against the pre-diff implementation.

use std::sync::Arc;

use flui_foundation::RenderId;
use flui_semantics::{SemanticsNode, SemanticsOwner, TreeUpdate};
use parking_lot::Mutex;

/// A render-backed node: production assembly always attaches a source
/// render id, and it is what makes a node publishable at all. Generation 7
/// keeps the stable id space visibly distinct from arena positions.
fn node(index: u32, label: &str) -> SemanticsNode {
    let mut node = SemanticsNode::new().with_source_render_id(RenderId::new_gen(
        index,
        core::num::NonZeroU32::new(7).expect("fixture generation is non-zero"),
    ));
    node.config_mut().set_label(label);
    node
}

/// Rebuild the owner's arena the way `run_semantics` does every pass:
/// clear, reinsert, root. `labels[i]` becomes child `i` of the root.
fn rebuild(owner: &mut SemanticsOwner, labels: &[&str]) {
    owner.clear();
    let root = owner.insert(node(0, "root"));
    for (index, label) in labels.iter().enumerate() {
        let child = owner.insert(node(
            u32::try_from(index).expect("fixture fits u32") + 1,
            label,
        ));
        owner.add_child(root, child);
    }
    owner.set_root(Some(root));
}

/// An owner whose callback records every delivered update.
fn recording_owner() -> (SemanticsOwner, Arc<Mutex<Vec<TreeUpdate>>>) {
    let received: Arc<Mutex<Vec<TreeUpdate>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&received);
    let owner = SemanticsOwner::new(Arc::new(move |update: &TreeUpdate| {
        sink.lock().push(update.clone());
    }));
    (owner, received)
}

/// **The incremental-publish contract.** An identical rebuild publishes
/// nothing (the idle contract); one changed label publishes exactly that node,
/// not its siblings or the unchanged root (the O(dirty) contract).
#[test]
fn a_rebuild_publishes_only_what_changed() {
    let (mut owner, received) = recording_owner();

    rebuild(&mut owner, &["alpha", "beta", "gamma"]);
    owner.flush();
    assert_eq!(
        received.lock().len(),
        1,
        "the first publish is the full tree"
    );

    rebuild(&mut owner, &["alpha", "beta", "gamma"]);
    owner.flush();
    assert_eq!(
        received.lock().len(),
        1,
        "an identical rebuild must not deliver a second update"
    );

    rebuild(&mut owner, &["alpha", "CHANGED", "gamma"]);
    owner.flush();

    let updates = received.lock();
    assert_eq!(updates.len(), 2, "the change itself must be delivered");
    let diff = &updates[1];
    assert_eq!(
        diff.nodes.len(),
        1,
        "one changed node publishes one node, not the tree: got {:?}",
        diff.nodes
            .iter()
            .map(|(id, node)| (id.0, node.label().map(str::to_owned)))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        diff.nodes[0].1.label(),
        Some("CHANGED"),
        "and it is the node whose content changed"
    );
}

#[test]
fn detaching_checks_the_actual_parent_and_preserves_reparenting() {
    let (mut owner, received) = recording_owner();
    let root = owner.insert(node(0, "root"));
    let first = owner.insert(node(1, "first parent"));
    let second = owner.insert(node(2, "second parent"));
    let child = owner.insert(node(3, "child"));
    owner.set_root(Some(root));
    owner.add_child(root, first);
    owner.add_child(root, second);
    owner.add_child(first, child);
    owner.flush();

    owner.remove_child(second, child);
    owner.remove_child(flui_foundation::SemanticsId::new(99), child);
    assert_eq!(owner.tree().parent(child), Some(first));
    assert_eq!(owner.tree().children(first), Some(&[child][..]));
    owner.flush();
    assert_eq!(received.lock().len(), 1, "a refused detach changes nothing");

    owner.add_child(second, child);
    assert_eq!(owner.tree().parent(child), Some(second));
    assert_eq!(owner.tree().children(first), Some(&[][..]));
    assert_eq!(owner.tree().children(second), Some(&[child][..]));
    owner.flush();
    {
        let updates = received.lock();
        let moved = &updates[1];
        let first_id = owner
            .get(first)
            .expect("first parent")
            .accessibility_id()
            .expect("render-backed");
        let second_id = owner
            .get(second)
            .expect("second parent")
            .accessibility_id()
            .expect("render-backed");
        let child_id = owner
            .get(child)
            .expect("child")
            .accessibility_id()
            .expect("render-backed");
        let published = |id| {
            &moved
                .nodes
                .iter()
                .find(|(node_id, _)| node_id.0 == id)
                .expect("changed parent published")
                .1
        };
        assert_eq!(published(first_id.as_u64()).children(), []);
        assert_eq!(
            published(second_id.as_u64()).children(),
            &[accesskit::NodeId(child_id.as_u64())]
        );
    }

    owner.remove_child(second, child);
    assert_eq!(owner.tree().parent(child), None);
    assert_eq!(owner.tree().children(second), Some(&[][..]));
    owner.add_child(first, child);
    assert_eq!(owner.tree().parent(child), Some(first));
    assert_eq!(owner.tree().children(first), Some(&[child][..]));
}
