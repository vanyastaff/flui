//! A slot whose generation space is exhausted refuses its removal (ADR-0313
//! §3): the preflight peek happens before any teardown runs, the occupant
//! stays live, and the refusal does not expose the slot to the next insert.

use super::*;

/// A slot's terminal seed: only this injection is private; refusal and reuse
/// go through the public tree.
fn terminal_generation(tree: &mut ElementTree, id: ElementId) -> ElementId {
    tree.generations
        .seed_generation(id.index(), Generation::LAST.as_u32());
    let generation = tree
        .generations
        .generation(id.index())
        .map(Generation::as_nonzero)
        .expect("the seeded terminal generation");
    let id = ElementId::new_gen(id.index(), generation);
    tree.get_mut(id)
        .expect("terminal occupant")
        .element_mut()
        .set_self_id(id);
    id
}

struct UnmountCounter(std::sync::atomic::AtomicUsize);
impl flui_foundation::observe::TreeObserver for UnmountCounter {
    fn element_unmounted(&self, _: &flui_foundation::observe::ElementUnmounted) {
        self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

fn exhausted_generation_refuses_removal(mode: u8) {
    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    let view = TestView {
        name: "live".into(),
    };
    let root = tree.mount_root(&view, &mut owner.element_owner_mut());
    let candidate = tree.insert(&view, root, 0, &mut owner.element_owner_mut());
    // Counter exhaustion is unreachable in a bounded consumer run. Only
    // this injection is private; refusal and reuse go through the public tree.
    tree.generations
        .seed_generation(candidate.index(), Generation::LAST.as_u32());
    let generation = tree
        .generations
        .generation(candidate.index())
        .map(Generation::as_nonzero)
        .expect("the seeded terminal generation");
    let candidate = ElementId::new_gen(candidate.index(), generation);
    tree.get_mut(candidate)
        .expect("live candidate")
        .element_mut()
        .set_self_id(candidate);
    tree.get_mut(root)
        .expect("root")
        .set_child_ids(vec![candidate]);
    for _ in 0..2 {
        let failure = std::panic::catch_unwind(AssertUnwindSafe(|| match mode {
            0 => tree.remove(candidate, &mut owner.element_owner_mut()),
            1 => tree.remove_finalized(candidate, &mut owner.element_owner_mut()),
            _ => tree.discard_unannounced(candidate, &mut owner.element_owner_mut()),
        }))
        .expect_err("exhausted removal must refuse");
        assert!(
            flui_foundation::panic::payload_text(&*failure)
                .expect("generation failure text")
                .contains("exhausted its packed generations")
        );
        let node = tree.get(candidate).expect("refused occupant stays live");
        assert_eq!(
            node.element().lifecycle(),
            crate::element::Lifecycle::Active
        );
        assert_eq!(node.parent(), Some(root));
    }
    let sibling = tree.insert(&view, root, 1, &mut owner.element_owner_mut());
    assert_ne!(
        sibling.index(),
        candidate.index(),
        "refusal cannot expose an exhausted slot"
    );
    assert!(
        tree.remove(sibling, &mut owner.element_owner_mut())
            .is_some()
    );
    let replacement = tree.insert(&view, root, 1, &mut owner.element_owner_mut());
    assert!(!tree.contains(sibling), "ordinary stale IDs remain refused");
    assert!(tree.contains(candidate) && tree.contains(replacement));
}

fn exhausted_subtree_refuses_before_teardown(target: usize, mode: SubtreeRemoval) {
    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    let observer = std::sync::Arc::new(UnmountCounter(std::sync::atomic::AtomicUsize::new(0)));
    owner.set_tree_observer(observer.clone());
    let view = TestView {
        name: "subtree".into(),
    };
    let parent = tree.mount_root(&view, &mut owner.element_owner_mut());
    let root = tree.insert(&view, parent, 0, &mut owner.element_owner_mut());
    let middle = tree.insert(&view, root, 0, &mut owner.element_owner_mut());
    let leaf = tree.insert(&view, middle, 0, &mut owner.element_owner_mut());
    let sibling = tree.insert(&view, root, 1, &mut owner.element_owner_mut());
    let mut ids = [root, middle, leaf, sibling];
    ids[target] = terminal_generation(&mut tree, ids[target]);
    let [root, middle, leaf, sibling] = ids;
    tree.get_mut(parent)
        .expect("parent")
        .set_child_ids(vec![root]);
    tree.get_mut(root)
        .expect("root")
        .set_child_ids(vec![middle, sibling]);
    tree.get_mut(middle)
        .expect("middle")
        .set_child_ids(vec![leaf]);
    for (child, parent) in [
        (root, parent),
        (middle, root),
        (leaf, middle),
        (sibling, root),
    ] {
        tree.get_mut(child).expect("child").parent = Some(parent);
    }
    for _ in 0..2 {
        let failure = std::panic::catch_unwind(AssertUnwindSafe(|| {
            tree.remove_subtree(root, &mut owner.element_owner_mut(), mode);
        }))
        .expect_err("whole subtree retirement must refuse");
        assert!(
            flui_foundation::panic::payload_text(&*failure)
                .expect("generation refusal")
                .contains("exhausted its packed generations")
        );
        assert_eq!(
            observer.0.load(std::sync::atomic::Ordering::Relaxed),
            0,
            "no teardown observation may precede admission"
        );
        assert_refused_subtree_intact(&tree, parent, ids);
    }
    let healthy = tree.insert(&view, parent, 1, &mut owner.element_owner_mut());
    tree.remove_subtree(
        healthy,
        &mut owner.element_owner_mut(),
        SubtreeRemoval::Finalize,
    );
    assert!(!tree.contains(healthy));
    assert_eq!(observer.0.load(std::sync::atomic::Ordering::Relaxed), 1);
    let replacement = tree.insert(&view, parent, 1, &mut owner.element_owner_mut());
    assert_ne!(healthy, replacement);
    assert!(!tree.contains(healthy));
    assert!(ids.iter().all(|id| tree.contains(*id)) && tree.contains(replacement));
}

fn assert_refused_subtree_intact(tree: &ElementTree, parent: ElementId, ids: [ElementId; 4]) {
    let [root, middle, leaf, sibling] = ids;
    for (id, parent) in [
        (root, parent),
        (middle, root),
        (leaf, middle),
        (sibling, root),
    ] {
        let node = tree.get(id).expect("entire refused subtree stays live");
        assert_eq!(node.parent(), Some(parent));
        assert_eq!(
            node.element().lifecycle(),
            crate::element::Lifecycle::Active
        );
    }
    assert_eq!(tree.get(parent).expect("parent").child_ids(), &[root]);
    assert_eq!(
        tree.get(root).expect("root").child_ids(),
        &[middle, sibling]
    );
    assert_eq!(tree.get(middle).expect("middle").child_ids(), &[leaf]);
}

fn exhausted_subtree_root_preserves_finalize_integrity() {
    exhausted_subtree_refuses_before_teardown(0, SubtreeRemoval::Finalize);
}
fn exhausted_subtree_middle_preserves_finalize_integrity() {
    exhausted_subtree_refuses_before_teardown(1, SubtreeRemoval::Finalize);
}
fn exhausted_subtree_leaf_preserves_finalize_integrity() {
    exhausted_subtree_refuses_before_teardown(2, SubtreeRemoval::Finalize);
}
fn exhausted_subtree_root_preserves_deactivate_integrity() {
    exhausted_subtree_refuses_before_teardown(0, SubtreeRemoval::DeactivateKeyed);
}
fn exhausted_subtree_middle_preserves_deactivate_integrity() {
    exhausted_subtree_refuses_before_teardown(1, SubtreeRemoval::DeactivateKeyed);
}
fn exhausted_subtree_leaf_preserves_deactivate_integrity() {
    exhausted_subtree_refuses_before_teardown(2, SubtreeRemoval::DeactivateKeyed);
}

fn exhausted_wrapper_refuses_before_keyed_descendant_deactivation() {
    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    let view = TestView {
        name: "wrapper".into(),
    };
    let parent = tree.mount_root(&view, &mut owner.element_owner_mut());
    let wrapper = tree.insert(&view, parent, 0, &mut owner.element_owner_mut());
    let wrapper = terminal_generation(&mut tree, wrapper);
    let key = GlobalKey::new();
    let keyed = tree.insert(
        &KeyedTestView { key: key.clone() },
        wrapper,
        0,
        &mut owner.element_owner_mut(),
    );
    let leaf = tree.insert(&view, keyed, 0, &mut owner.element_owner_mut());
    tree.get_mut(parent)
        .expect("parent")
        .set_child_ids(vec![wrapper]);
    tree.get_mut(wrapper)
        .expect("wrapper")
        .set_child_ids(vec![keyed]);
    tree.get_mut(keyed)
        .expect("keyed")
        .set_child_ids(vec![leaf]);
    for _ in 0..2 {
        std::panic::catch_unwind(AssertUnwindSafe(|| {
            tree.remove_subtree(
                wrapper,
                &mut owner.element_owner_mut(),
                SubtreeRemoval::DeactivateKeyed,
            );
        }))
        .expect_err("wrapper refuses before any keyed detach");
        assert_eq!(tree.get(parent).expect("parent").child_ids(), &[wrapper]);
        assert_eq!(tree.get(wrapper).expect("wrapper").child_ids(), &[keyed]);
        assert_eq!(tree.get(keyed).expect("keyed").parent(), Some(wrapper));
        assert_eq!(tree.get(keyed).expect("keyed").child_ids(), &[leaf]);
        assert_eq!(owner.element_for_global_key(&key), Some(keyed));
        for id in [wrapper, keyed, leaf] {
            assert_eq!(
                tree.get(id)
                    .expect("refused occupant")
                    .element()
                    .lifecycle(),
                crate::element::Lifecycle::Active
            );
        }
    }
    let healthy = tree.insert(&view, parent, 1, &mut owner.element_owner_mut());
    tree.remove_subtree(
        healthy,
        &mut owner.element_owner_mut(),
        SubtreeRemoval::Finalize,
    );
    assert!(!tree.contains(healthy));
    assert!(tree.contains(wrapper) && tree.contains(keyed) && tree.contains(leaf));
}

fn terminal_keyed_subtree_soft_removal_needs_no_generation_advance() {
    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    let view = TestView {
        name: "wrapper".into(),
    };
    let parent = tree.mount_root(&view, &mut owner.element_owner_mut());
    let wrapper = tree.insert(&view, parent, 0, &mut owner.element_owner_mut());
    let key = GlobalKey::new();
    let keyed = tree.insert(
        &KeyedTestView { key: key.clone() },
        wrapper,
        0,
        &mut owner.element_owner_mut(),
    );
    let keyed = terminal_generation(&mut tree, keyed);
    owner.element_owner_mut().unregister_global_key(&key);
    owner.element_owner_mut().register_global_key(&key, keyed);
    let leaf = tree.insert(&view, keyed, 0, &mut owner.element_owner_mut());
    let leaf = terminal_generation(&mut tree, leaf);
    tree.get_mut(parent)
        .expect("parent")
        .set_child_ids(vec![wrapper]);
    tree.get_mut(wrapper)
        .expect("wrapper")
        .set_child_ids(vec![keyed]);
    tree.get_mut(keyed)
        .expect("keyed")
        .set_child_ids(vec![leaf]);
    tree.remove_subtree(
        wrapper,
        &mut owner.element_owner_mut(),
        SubtreeRemoval::DeactivateKeyed,
    );
    assert!(!tree.contains(wrapper));
    assert_eq!(owner.element_for_global_key(&key), Some(keyed));
    assert_eq!(
        tree.get(keyed).expect("keyed survives").child_ids(),
        &[leaf]
    );
    assert_eq!(
        tree.get(keyed)
            .expect("keyed survives")
            .element()
            .lifecycle(),
        crate::element::Lifecycle::Inactive
    );
    assert!(tree.contains(leaf));
}

fn exhausted_eager_removal_keeps_its_occupant_live() {
    exhausted_generation_refuses_removal(0);
}
fn exhausted_finalization_keeps_its_occupant_live() {
    exhausted_generation_refuses_removal(1);
}
fn exhausted_unannounced_retirement_keeps_its_occupant_live() {
    exhausted_generation_refuses_removal(2);
}

#[test]
fn exhaustion_refusal_matrix() {
    crate::table_test::run_table(
        "exhaustion_refusal_matrix",
        &[
            (
                "exhausted_eager_removal_keeps_its_occupant_live",
                exhausted_eager_removal_keeps_its_occupant_live as fn(),
            ),
            (
                "exhausted_finalization_keeps_its_occupant_live",
                exhausted_finalization_keeps_its_occupant_live as fn(),
            ),
            (
                "exhausted_unannounced_retirement_keeps_its_occupant_live",
                exhausted_unannounced_retirement_keeps_its_occupant_live as fn(),
            ),
            (
                "exhausted_subtree_root_preserves_finalize_integrity",
                exhausted_subtree_root_preserves_finalize_integrity as fn(),
            ),
            (
                "exhausted_subtree_middle_preserves_finalize_integrity",
                exhausted_subtree_middle_preserves_finalize_integrity as fn(),
            ),
            (
                "exhausted_subtree_leaf_preserves_finalize_integrity",
                exhausted_subtree_leaf_preserves_finalize_integrity as fn(),
            ),
            (
                "exhausted_subtree_root_preserves_deactivate_integrity",
                exhausted_subtree_root_preserves_deactivate_integrity as fn(),
            ),
            (
                "exhausted_subtree_middle_preserves_deactivate_integrity",
                exhausted_subtree_middle_preserves_deactivate_integrity as fn(),
            ),
            (
                "exhausted_subtree_leaf_preserves_deactivate_integrity",
                exhausted_subtree_leaf_preserves_deactivate_integrity as fn(),
            ),
            (
                "exhausted_wrapper_refuses_before_keyed_descendant_deactivation",
                exhausted_wrapper_refuses_before_keyed_descendant_deactivation as fn(),
            ),
            (
                "terminal_keyed_subtree_soft_removal_needs_no_generation_advance",
                terminal_keyed_subtree_soft_removal_needs_no_generation_advance as fn(),
            ),
        ],
    );
}
