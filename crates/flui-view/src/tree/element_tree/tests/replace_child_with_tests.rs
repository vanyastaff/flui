use super::*;
use std::sync::Arc;

#[derive(Default)]
struct ReplacementObserver(parking_lot::Mutex<Vec<(bool, ElementId)>>);

impl flui_foundation::observe::TreeObserver for ReplacementObserver {
    fn element_mounted(&self, event: &flui_foundation::observe::ElementMounted) {
        self.0.lock().push((true, event.element));
    }

    fn element_unmounted(&self, event: &flui_foundation::observe::ElementUnmounted) {
        self.0.lock().push((false, event.element));
    }
}

#[derive(Clone)]
struct PanicsOnCreate;

impl View for PanicsOnCreate {
    fn create_element(&self) -> crate::element::ElementKind {
        panic!("replacement factory payload")
    }
}

#[test]
fn replace_child_with_finalizes_keyed_subtree_and_commits_one_scheduled_mount() {
    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    let observer = Arc::new(ReplacementObserver::default());
    owner.set_tree_observer(observer.clone());
    let parent = tree.mount_root(&UnitRenderHost, &mut owner.element_owner_mut());
    let first = tree.insert(&UnitRenderHost, parent, 0, &mut owner.element_owner_mut());
    let key = GlobalKey::<()>::new();
    let old = tree.insert(
        &KeyedTestView { key: key.clone() },
        parent,
        1,
        &mut owner.element_owner_mut(),
    );
    let descendant = tree.insert(&UnitRenderHost, old, 0, &mut owner.element_owner_mut());
    let last = tree.insert(&UnitRenderHost, parent, 2, &mut owner.element_owner_mut());
    tree.get_mut(parent)
        .unwrap()
        .set_child_ids(vec![first, old, last]);
    tree.get_mut(old).unwrap().set_child_ids(vec![descendant]);
    let (before_len, event_count) = (tree.len(), observer.0.lock().len());
    let new = tree.replace_child_with(parent, 1, &UnitRenderHost, &mut owner.element_owner_mut());
    assert_eq!(tree.get(parent).unwrap().child_ids(), &[first, new, last]);
    assert_eq!(tree.len(), before_len - 1);
    assert!(!tree.contains(old) && !tree.contains(descendant));
    assert_eq!(owner.element_for_global_key(&key), None);
    assert!(owner.global_key_reservations.is_empty());
    assert!(!owner.has_inactive_elements());
    let node = tree.get(new).unwrap();
    assert_eq!(
        (node.parent(), node.slot(), node.depth()),
        (Some(parent), 1, 1)
    );
    let initial = flui_foundation::RebuildReasons::from_reason(crate::RebuildReason::InitialMount);
    assert_eq!(owner.pending_rebuild_reasons(new), Some(initial));
    assert_eq!(owner.pending_rebuild_reasons(old), None);
    assert_eq!(
        &observer.0.lock()[event_count..],
        &[(false, descendant), (false, old), (true, new)],
    );
}

#[test]
fn replace_child_with_arms_render_tail_and_notifies_render_parent_without_a_frontier() {
    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    let pipeline = PipelineCell::new(flui_rendering::PipelineOwner::new());
    let parent = tree.mount_root_with_pipeline_owner(
        &UnitRenderHost,
        Some(pipeline.clone()),
        &mut owner.element_owner_mut(),
    );
    let first = tree.insert(&UnitRenderHost, parent, 0, &mut owner.element_owner_mut());
    let old = tree.insert(&UnitRenderHost, parent, 1, &mut owner.element_owner_mut());
    let last = tree.insert(&UnitRenderHost, parent, 2, &mut owner.element_owner_mut());
    tree.get_mut(parent)
        .unwrap()
        .set_child_ids(vec![first, old, last]);
    assert!(tree.reorder_render_children_after_build() > 0);
    let parent_render = tree.get(parent).unwrap().element().render_id().unwrap();
    let new = tree.replace_child_with(parent, 1, &UnitRenderHost, &mut owner.element_owner_mut());
    let expected =
        [first, new, last].map(|id| tree.get(id).unwrap().element().render_id().unwrap());
    pipeline.with(|owner| {
        assert_eq!(
            owner.render_tree().get(parent_render).unwrap().children(),
            &[expected[0], expected[2], expected[1]],
        );
    });
    assert!(tree.reorder_render_children_after_build() > 0);
    assert_eq!(tree.reorder_render_children_after_build(), 0);
    pipeline.with(|owner| {
        assert_eq!(
            owner.render_tree().get(parent_render).unwrap().children(),
            expected,
        );
    });
    let component = tree.insert(
        &TestView {
            name: "component".into(),
        },
        parent,
        3,
        &mut owner.element_owner_mut(),
    );
    tree.get_mut(parent)
        .unwrap()
        .set_child_ids(vec![first, new, last, component]);
    pipeline.with_mut(|owner| {
        owner.clear_all_dirty_nodes();
        owner
            .render_tree()
            .get(parent_render)
            .unwrap()
            .clear_needs_layout();
    });
    tree.replace_child_with(
        parent,
        3,
        &TestView {
            name: "replacement".into(),
        },
        &mut owner.element_owner_mut(),
    );
    pipeline.with(|owner| {
        assert!(
            owner
                .render_tree()
                .get(parent_render)
                .unwrap()
                .needs_layout()
        );
    });
    assert!(tree.reorder_render_children_after_build() > 0);
    assert_eq!(tree.reorder_render_children_after_build(), 0);
}

#[test]
fn replace_child_with_rejects_invalid_boundaries_before_mutation_and_keeps_factory_unbounded() {
    use flui_rendering::PipelineOwner as RenderOwner;
    fn expect_bug(
        tree: &mut ElementTree,
        owner: &mut BuildOwner,
        parent: ElementId,
        call_parent: ElementId,
        slot: usize,
        view: &dyn View,
    ) {
        let (len, edge) = (tree.len(), tree.get(parent).unwrap().child_ids().to_vec());
        let panic = std::panic::catch_unwind(AssertUnwindSafe(|| {
            tree.replace_child_with(call_parent, slot, view, &mut owner.element_owner_mut())
        }))
        .expect_err("invalid replacement boundary must panic");
        assert!(
            flui_foundation::panic::payload_text(&*panic)
                .is_some_and(|text| text.starts_with("BUG:")),
        );
        assert_eq!(
            (tree.len(), tree.get(parent).unwrap().child_ids()),
            (len, edge.as_slice())
        );
    }

    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    let observer = Arc::new(ReplacementObserver::default());
    owner.set_tree_observer(observer.clone());
    let pipeline = PipelineCell::new(flui_rendering::PipelineOwner::new());
    let parent = tree.mount_root_with_pipeline_owner(
        &UnitRenderHost,
        Some(pipeline.clone()),
        &mut owner.element_owner_mut(),
    );
    let key = GlobalKey::<()>::new();
    let old = tree.insert(
        &KeyedUnitRenderHost { key: key.clone() },
        parent,
        0,
        &mut owner.element_owner_mut(),
    );
    tree.get_mut(parent).unwrap().set_child_ids(vec![old]);
    assert!(tree.reorder_render_children_after_build() > 0);
    let render_parent = tree.get(parent).unwrap().element().render_id().unwrap();
    pipeline.with_mut(RenderOwner::clear_all_dirty_nodes);
    let events = observer.0.lock().clone();
    let old_render = tree.get(old).unwrap().element().render_id().unwrap();
    expect_bug(
        &mut tree,
        &mut owner,
        parent,
        parent,
        0,
        &KeyedTestView {
            key: GlobalKey::new(),
        },
    );
    assert_eq!(owner.element_for_global_key(&key), Some(old));
    assert!(!owner.global_key_reservations.is_empty());
    assert_eq!(*observer.0.lock(), events);
    assert_eq!(owner.pending_rebuild_reasons(parent), None);
    assert_eq!(owner.pending_rebuild_reasons(old), None);
    assert_eq!(pipeline.with(RenderOwner::dirty_node_count), 0);
    pipeline.with(|owner| {
        assert_eq!(
            owner.render_tree().get(render_parent).unwrap().children(),
            &[old_render],
        );
    });

    let stale = ElementId::new(999_999);
    expect_bug(&mut tree, &mut owner, parent, stale, 0, &UnitRenderHost);
    expect_bug(&mut tree, &mut owner, parent, parent, 1, &UnitRenderHost);
    tree.get_mut(parent).unwrap().child_ids[0] = stale;
    expect_bug(&mut tree, &mut owner, parent, parent, 0, &UnitRenderHost);
    tree.get_mut(parent).unwrap().child_ids[0] = old;
    tree.get_mut(old).unwrap().parent = None;
    expect_bug(&mut tree, &mut owner, parent, parent, 0, &UnitRenderHost);
    tree.get_mut(old).unwrap().parent = Some(parent);
    tree.get_mut(old).unwrap().slot = 7;
    expect_bug(&mut tree, &mut owner, parent, parent, 0, &UnitRenderHost);
    tree.get_mut(old).unwrap().slot = 0;

    let event_count = observer.0.lock().len();
    let panic = std::panic::catch_unwind(AssertUnwindSafe(|| {
        tree.replace_child_with(parent, 0, &PanicsOnCreate, &mut owner.element_owner_mut())
    }))
    .expect_err("replacement factory stays unbounded");
    assert_eq!(
        flui_foundation::panic::payload_text(&*panic),
        Some("replacement factory payload"),
    );
    assert!(!tree.contains(old));
    assert_eq!(tree.get(parent).unwrap().child_ids(), &[old]);
    assert!(owner.recovered_panics.is_empty());
    assert_eq!(&observer.0.lock()[event_count..], &[(false, old)]);
}
