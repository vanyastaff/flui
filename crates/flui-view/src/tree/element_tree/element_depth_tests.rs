//! `ElementBase::depth` is the element's depth in the tree (root = 0), the
//! same value its node carries, not the sibling slot it was mounted into.

use super::*;

#[test]
fn element_depth_is_the_tree_depth_not_the_sibling_slot() {
    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    let root = tree.mount_root(&leaf("root"), &mut owner.element_owner_mut());
    let a = tree.insert(&leaf("a"), root, 0, &mut owner.element_owner_mut());
    let b = tree.insert(&leaf("b"), a, 0, &mut owner.element_owner_mut());
    let c = tree.insert(&leaf("c"), root, 3, &mut owner.element_owner_mut());

    for (id, expected) in [(root, 0), (a, 1), (b, 2), (c, 1)] {
        let node = tree.get(id).expect("inserted node is live");
        assert_eq!(node.element().depth(), expected);
        assert_eq!(node.element().depth(), node.depth());
    }
}

#[test]
#[serial_test::serial(global_key_registry)]
fn globalkey_retake_restamps_element_depth_for_the_moved_subtree() {
    use parking_lot::RwLock;

    let tree = Arc::new(RwLock::new(ElementTree::new()));
    let owner = Arc::new(RwLock::new(BuildOwner::new()));
    crate::test_only_set_global_key_registry(&tree, &owner);

    let insert = |view: &dyn View, parent: ElementId, slot: usize| {
        tree.write()
            .insert(view, parent, slot, &mut owner.write().element_owner_mut())
    };
    let root = tree
        .write()
        .mount_root(&leaf("root"), &mut owner.write().element_owner_mut());
    let shallow_parent = insert(&leaf("shallow"), root, 0);
    let deep_1 = insert(&leaf("deep-1"), root, 1);
    let deep_2 = insert(&leaf("deep-2"), deep_1, 0);
    let deep_3 = insert(&leaf("deep-3"), deep_2, 0);

    let keyed = Keyed {
        key: crate::GlobalKey::new(),
    };
    let moved = insert(&keyed, shallow_parent, 0);
    let child = insert(&leaf("child"), moved, 0);
    let grandchild = insert(&leaf("grandchild"), child, 0);
    tree.write()
        .get_mut(moved)
        .expect("moved node exists")
        .set_child_ids(vec![child]);
    tree.write()
        .get_mut(child)
        .expect("child exists")
        .set_child_ids(vec![grandchild]);

    tree.write()
        .remove(moved, &mut owner.write().element_owner_mut());
    let migrated = insert(&keyed, deep_3, 0);
    assert_eq!(migrated, moved, "GlobalKey retake preserves identity");

    {
        let tree = tree.read();
        for (id, expected) in [(moved, 4), (child, 5), (grandchild, 6)] {
            let node = tree.get(id).expect("moved subtree is live");
            assert_eq!(node.depth(), expected);
            assert_eq!(
                node.element().depth(),
                expected,
                "the element's depth follows its node through the retake"
            );
        }
    }

    crate::test_only_clear_global_key_registry();
}
