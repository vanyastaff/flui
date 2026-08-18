//! The incremental-publish contract of `SemanticsOwner::flush`.
//!
//! Flutter's `SemanticsOwner.sendSemanticsUpdate` serializes only dirty
//! nodes into the platform update and returns immediately when nothing is
//! dirty. FLUI rebuilds its semantics arena every assembly pass (ADR-0014),
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

/// **The idle contract.** An assembly pass that reproduces the same tree —
/// the shape of "something upstream was marked, nothing observable moved" —
/// must deliver nothing to the platform. Before the diff, every such pass
/// republished the entire tree and relied on the adapter to suppress the
/// resulting event storm.
#[test]
fn a_rebuild_that_changes_nothing_publishes_nothing() {
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
}

/// **The O(dirty) contract.** One toggled label in the tree publishes
/// exactly the node that changed — not its unchanged siblings, not the
/// unchanged root. Before the diff this update carried all four nodes.
#[test]
fn a_single_content_change_publishes_exactly_that_node() {
    let (mut owner, received) = recording_owner();

    rebuild(&mut owner, &["alpha", "beta", "gamma"]);
    owner.flush();

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

/// A structural change surfaces through the parent whose children list
/// changed — that republished parent is also how an adapter learns the
/// removed child is gone (AccessKit removals are implicit). The unchanged
/// sibling must not ride along.
#[test]
fn removing_a_child_republishes_its_parent_and_nothing_else() {
    let (mut owner, received) = recording_owner();

    rebuild(&mut owner, &["alpha", "beta"]);
    owner.flush();

    rebuild(&mut owner, &["alpha"]);
    owner.flush();

    let updates = received.lock();
    assert_eq!(updates.len(), 2);
    let diff = &updates[1];
    let labels: Vec<_> = diff.nodes.iter().filter_map(|(_, n)| n.label()).collect();
    assert_eq!(
        labels,
        vec!["root"],
        "only the parent whose children changed is republished"
    );
    assert!(
        !diff.nodes.iter().any(|(_, n)| n.label() == Some("beta")),
        "a removed node never appears in the update that removes it"
    );
}

/// A node that is removed and later returns with identical content must be
/// republished: the adapter dropped it when its parent was republished
/// without it, so "unchanged since last published" is no longer true of
/// anything the adapter still holds. This is the pin on the mirror's
/// prune-on-removal step.
#[test]
fn a_removed_then_restored_node_is_republished() {
    let (mut owner, received) = recording_owner();

    rebuild(&mut owner, &["alpha", "beta"]);
    owner.flush();
    rebuild(&mut owner, &["alpha"]);
    owner.flush();

    rebuild(&mut owner, &["alpha", "beta"]);
    owner.flush();

    let updates = received.lock();
    assert_eq!(updates.len(), 3);
    let restored = &updates[2];
    assert!(
        restored
            .nodes
            .iter()
            .any(|(_, n)| n.label() == Some("beta")),
        "the restored node must be re-sent — the adapter no longer has it"
    );
}

/// The first delivered update is self-contained: it carries the tree
/// metadata (`TreeUpdate::tree`) an adapter needs to initialize, and every
/// node. Incremental follow-ups carry `tree: None` — that asymmetry is the
/// owner's promise that any update with tree metadata stands alone, which
/// the Linux bridge relies on to answer a late-activating screen reader.
#[test]
fn full_updates_carry_tree_metadata_and_diffs_do_not() {
    let (mut owner, received) = recording_owner();

    rebuild(&mut owner, &["alpha", "beta"]);
    owner.flush();
    rebuild(&mut owner, &["alpha", "CHANGED"]);
    owner.flush();

    let updates = received.lock();
    assert_eq!(updates.len(), 2);
    assert!(
        updates[0].tree.is_some(),
        "the initial update must be adapter-initializing"
    );
    assert_eq!(
        updates[0].nodes.len(),
        3,
        "and self-contained: root plus both children"
    );
    assert!(
        updates[1].tree.is_none(),
        "an incremental update must not claim to stand alone"
    );
}

/// `send_full_tree` exists for a reconnecting adapter that may have
/// forgotten everything, so it must bypass the diff even when nothing
/// changed since the last publish.
#[test]
fn send_full_tree_republishes_everything_even_when_clean() {
    let (mut owner, received) = recording_owner();

    rebuild(&mut owner, &["alpha", "beta"]);
    owner.flush();

    owner.send_full_tree();

    let updates = received.lock();
    assert_eq!(updates.len(), 2);
    let full = &updates[1];
    assert_eq!(full.nodes.len(), 3, "all nodes, not a diff");
    assert!(full.tree.is_some(), "self-contained, adapter-initializing");
}

/// A focus movement is delivered even when it is the only change. Focus is
/// tree-level state in AccessKit — it rides the update envelope, not any
/// node's payload — so a focus-only frame publishes an update with **no**
/// nodes at all. Suppressing it as "empty diff" would strand the screen
/// reader on the old node.
#[test]
fn a_focus_change_is_published_even_when_no_node_payload_changed() {
    let (mut owner, received) = recording_owner();

    // Build directly (not via `rebuild`) so the fixture can set focus.
    let build = |owner: &mut SemanticsOwner, focused: usize| {
        owner.clear();
        let root = owner.insert(node(0, "root"));
        for (index, label) in ["alpha", "beta"].iter().enumerate() {
            let mut child = node(u32::try_from(index).expect("fixture fits u32") + 1, label);
            child.config_mut().set_focused(index == focused);
            let child = owner.insert(child);
            owner.add_child(root, child);
        }
        owner.set_root(Some(root));
    };

    build(&mut owner, 0);
    owner.flush();
    build(&mut owner, 1);
    owner.flush();

    let updates = received.lock();
    assert_eq!(updates.len(), 2);
    let (first, second) = (&updates[0], &updates[1]);
    assert_ne!(
        first.focus, second.focus,
        "the envelope focus must track the moved flag"
    );
    assert!(
        second.nodes.is_empty(),
        "focus is envelope state — no node payload changed, none is republished"
    );
}

/// Focus ambiguity counts EVERY claimant, publishable or not — exactly as
/// the full-publish path does (`tree_to_update` warns and falls back to the
/// root when two nodes claim focus, regardless of addressability). An
/// unrelated incremental flush must not "resolve" the ambiguity toward the
/// one claimant that happens to be publishable.
#[test]
fn an_unaddressable_claimant_keeps_focus_ambiguous_across_incremental_flushes() {
    let (mut owner, received) = recording_owner();

    let root = owner.insert(node(0, "root"));
    let mut focused = node(1, "focused");
    focused.config_mut().set_focused(true);
    let focused = owner.insert(focused);
    // A second claimant with no render identity: never publishable, but a
    // claimant all the same.
    let mut phantom = SemanticsNode::new();
    phantom.config_mut().set_label("phantom");
    phantom.config_mut().set_focused(true);
    let phantom = owner.insert(phantom);
    owner.add_child(root, focused);
    owner.add_child(root, phantom);
    owner.set_root(Some(root));

    owner.flush();
    let root_focus = {
        let updates = received.lock();
        assert_eq!(
            updates[0].focus,
            updates[0]
                .tree
                .as_ref()
                .expect("initializing update carries tree metadata")
                .root,
            "two claimants are ambiguous; the full publish focuses the root"
        );
        updates[0].focus
    };

    // An unrelated content change; both focus claims still stand.
    owner
        .get_mut(root)
        .expect("root is live")
        .config_mut()
        .set_label("root renamed");
    owner.flush();

    let updates = received.lock();
    assert_eq!(updates.len(), 2);
    assert_eq!(
        updates[1].focus, root_focus,
        "the ambiguity did not go away, so neither may the root fallback"
    );
}

/// A focus flag toggled on an unpublishable node still changes what the
/// tree as a whole claims: a second claimant appearing means ambiguity, and
/// the published focus must retreat to the root — the same answer a full
/// republish of this tree would give.
#[test]
fn toggling_an_unaddressable_nodes_focus_is_observed() {
    let (mut owner, received) = recording_owner();

    let root = owner.insert(node(0, "root"));
    let mut focused = node(1, "focused");
    focused.config_mut().set_focused(true);
    let focused_sid = owner.insert(focused);
    let mut phantom = SemanticsNode::new();
    phantom.config_mut().set_label("phantom");
    let phantom = owner.insert(phantom);
    owner.add_child(root, focused_sid);
    owner.add_child(root, phantom);
    owner.set_root(Some(root));

    owner.flush();
    {
        let updates = received.lock();
        let focused_identity = updates[0]
            .nodes
            .iter()
            .find(|(_, n)| n.label() == Some("focused"))
            .map(|(id, _)| *id)
            .expect("the focused node is published");
        assert_eq!(
            updates[0].focus, focused_identity,
            "a single publishable claimant is the focus"
        );
    }

    // The unpublishable node now claims focus too.
    owner
        .get_mut(phantom)
        .expect("phantom is live")
        .config_mut()
        .set_focused(true);
    owner.flush();

    let updates = received.lock();
    assert_eq!(
        updates.len(),
        2,
        "the focus retreat must be delivered even though no payload changed"
    );
    assert_eq!(
        updates[1].focus,
        updates[0]
            .tree
            .as_ref()
            .expect("initializing update carries tree metadata")
            .root,
        "two claimants are ambiguous; focus falls back to the root"
    );
}

/// Re-pointing the root at an already-published, otherwise-untouched node
/// is a root-identity change with zero dirty content — `set_root` itself
/// must count as a change, or the flush gate returns before the
/// root-escalation check ever runs and the adapter stays on the old root
/// forever.
#[test]
fn repointing_the_root_at_a_clean_node_escalates_to_a_full_update() {
    let (mut owner, received) = recording_owner();

    // Two independent addressable nodes; `a` is the published root.
    let a = owner.insert(node(1, "a"));
    let b = owner.insert(node(2, "b"));
    owner.set_root(Some(a));
    owner.flush();
    assert_eq!(received.lock().len(), 1, "the initial publish under root a");

    // Only the root pointer moves; no node content is touched.
    owner.set_root(Some(b));
    owner.flush();

    let updates = received.lock();
    assert_eq!(
        updates.len(),
        2,
        "a root-identity change must be delivered even with no dirty content"
    );
    let repointed = &updates[1];
    let new_root = repointed
        .tree
        .as_ref()
        .expect("a root change is a self-contained, adapter-initializing update")
        .root;
    let b_identity = owner
        .get(b)
        .and_then(flui_semantics::SemanticsNode::accessibility_id)
        .expect("b is render-backed");
    assert_eq!(
        new_root.0,
        b_identity.as_u64(),
        "and it names the new root identity"
    );
}

/// `needs_flush` is the public would-a-flush-do-anything predicate, so it
/// must match `flush`'s own gate: a scheduled full publish is pending work
/// even when every node is clean. A frame loop consulting the narrower
/// predicate would never deliver the reconnect update.
#[test]
fn needs_flush_reports_a_scheduled_full_publish_on_a_clean_tree() {
    let (mut owner, received) = recording_owner();

    rebuild(&mut owner, &["alpha"]);
    owner.flush();
    assert!(
        !owner.needs_flush(),
        "a settled owner has nothing to deliver"
    );

    owner.schedule_full_publish();
    assert!(
        owner.needs_flush(),
        "a pending full publish is pending work — the predicate must say so"
    );

    owner.flush();
    assert!(
        !owner.needs_flush(),
        "delivering it settles the owner again"
    );
    assert_eq!(
        received.lock().len(),
        2,
        "and the flush the predicate promised actually happened"
    );
}

/// A different root identity is a different tree to the adapter; a diff
/// cannot express that, so it must escalate to a self-contained full
/// update.
#[test]
fn a_root_identity_change_escalates_to_a_full_update() {
    let (mut owner, received) = recording_owner();

    rebuild(&mut owner, &["alpha"]);
    owner.flush();

    // Same shape, entirely different identities (different render slots).
    owner.clear();
    let root = owner.insert(node(100, "root"));
    let child = owner.insert(node(101, "alpha"));
    owner.add_child(root, child);
    owner.set_root(Some(root));
    owner.flush();

    let updates = received.lock();
    assert_eq!(updates.len(), 2);
    let replaced = &updates[1];
    assert!(
        replaced.tree.is_some(),
        "a new root re-initializes the adapter"
    );
    assert_eq!(replaced.nodes.len(), 2, "and carries the whole new tree");
}
