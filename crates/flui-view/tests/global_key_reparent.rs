//! GlobalKey reparenting test — locks the inactive-queue
//! reactivation wiring.
//!
//! The inactive-queue reactivation path emits a
//! `ReconcileEvent::Reparent` with `from_parent: None` when an
//! element registered under a `GlobalKey` is pulled back from the
//! `BuildOwner::inactive_elements` queue and re-attached at a new
//! (parent, slot). This file is the end-to-end lock for that
//! path: it observes the event stream via the
//! `ReconcileEventCollector` and asserts the disposition
//! distribution matches the contract.
//!
//! Cross-parent same-frame ACTIVE reparent (case 2 below) is locked here too:
//! the tree forgets the active element from its old parent, moves it under the
//! new parent, and emits `from_parent: Some(old_parent)`.

#![cfg(feature = "testing")]
use flui_foundation::ViewKey;
use flui_view::LifecycleContext;
use flui_view::{
    BuildContext, BuildOwner, ElementTree, GlobalKey, IntoView, StatefulView, View, ViewExt,
    ViewState,
    tree::{
        ReconcileEventKind,
        test_utils::{CollectedEvent, reconcile_children},
    },
};
use std::cell::RefCell;
use std::rc::Rc;

use crate::reconcile_capture::capture;

// ============================================================================
// Fixtures — a stateful keyed counter widget so state survival is
// observable after a reparent (counter value persists across the
// inactive-queue migration).
// ============================================================================

/// Pure leaf used as a parent scaffold so we can build a two-parent
/// tree to migrate between.
#[derive(Clone)]
struct Spacer;

impl StatefulView for Spacer {
    type State = SpacerState;
    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        SpacerState
    }
}

struct SpacerState;
impl ViewState<Spacer> for SpacerState {
    fn build(&self, _view: &Spacer, _ctx: &dyn BuildContext) -> impl IntoView {
        crate::global_key_duplication::Leaf.boxed()
    }
}

impl View for Spacer {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

/// Keyed stateful counter. State holds an `i32 count` we mutate to
/// prove migration preserves it.
#[derive(Clone)]
struct KeyedCounter {
    key: GlobalKey<CounterState>,
    initial: i32,
}

struct CounterState {
    count: i32,
}

impl CounterState {
    fn count(&self) -> i32 {
        self.count
    }
    #[expect(
        dead_code,
        reason = "the fixture keeps a mutating accessor for tests that exercise mutable state; none does yet"
    )]
    fn bump(&mut self, by: i32) {
        self.count += by;
    }
}

impl StatefulView for KeyedCounter {
    type State = CounterState;
    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        CounterState {
            count: self.initial,
        }
    }
}

impl ViewState<KeyedCounter> for CounterState {
    fn build(&self, _v: &KeyedCounter, _ctx: &dyn BuildContext) -> impl IntoView {
        crate::global_key_duplication::Leaf.boxed()
    }
}

impl View for KeyedCounter {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
    fn key(&self) -> Option<&dyn ViewKey> {
        Some(&self.key)
    }
}

// ============================================================================
// Helpers
// ============================================================================

fn fresh_tree() -> (Rc<RefCell<ElementTree>>, Rc<RefCell<BuildOwner>>) {
    let tree = Rc::new(RefCell::new(ElementTree::new()));
    let owner = Rc::new(RefCell::new(BuildOwner::new()));
    flui_view::test_only_set_global_key_registry(&tree, &owner);
    (tree, owner)
}

fn direct_children_in_slot_order(
    tree: &ElementTree,
    parent: flui_foundation::ElementId,
) -> Vec<flui_foundation::ElementId> {
    let mut children: Vec<_> = tree
        .iter_nodes()
        .filter(|(_, node)| node.parent() == Some(parent))
        .map(|(id, node)| (node.slot(), id))
        .collect();
    children.sort_by_key(|(slot, _)| *slot);
    children.into_iter().map(|(_, id)| id).collect()
}

// ============================================================================
// GlobalKey reparent tests
// ============================================================================

/// Case 2: a new parent can claim a GlobalKey element
/// that is still ACTIVE under another parent in the same frame. The old parent
/// forgets the child, the element id/state survive, and the trace event records
/// `from_parent: Some(old_parent)`.
#[serial_test::serial(global_key_registry)]
pub(crate) fn active_to_active_reparent_emits_from_parent_and_preserves_state() {
    let (tree, owner) = fresh_tree();

    let parent_a = tree
        .borrow_mut()
        .mount_root(&Spacer, &mut owner.borrow_mut().element_owner_mut());
    let key = GlobalKey::<CounterState>::new();
    let counter = KeyedCounter {
        key: key.clone(),
        initial: 17,
    };
    let key_hash = key.key_hash();

    let initial_views: Vec<Box<dyn View>> = vec![Box::new(Spacer), Box::new(counter.clone())];
    reconcile_children(
        &mut tree.borrow_mut(),
        parent_a,
        &initial_views,
        &mut owner.borrow_mut().element_owner_mut(),
    );
    let initial_children = direct_children_in_slot_order(&tree.borrow(), parent_a);
    let parent_b = initial_children[0];
    let original_id = initial_children[1];
    owner
        .borrow_mut()
        .schedule_build_for(original_id, 2, flui_view::RebuildReason::InitialMount);
    owner.borrow_mut().build_scope(&mut tree.borrow_mut());
    assert_eq!(
        key.with_current_state::<i32>(CounterState::count),
        Some(17),
        "precondition: keyed state is registered before the active move",
    );

    let migrated_id = std::cell::Cell::new(None);
    let events = capture(|| {
        let views: Vec<Box<dyn View>> = vec![Box::new(counter.clone())];
        reconcile_children(
            &mut tree.borrow_mut(),
            parent_b,
            &views,
            &mut owner.borrow_mut().element_owner_mut(),
        );
        migrated_id.set(
            direct_children_in_slot_order(&tree.borrow(), parent_b)
                .first()
                .copied(),
        );
        let donor_views: Vec<Box<dyn View>> = vec![Box::new(Spacer)];
        reconcile_children(
            &mut tree.borrow_mut(),
            parent_a,
            &donor_views,
            &mut owner.borrow_mut().element_owner_mut(),
        );
    });
    let migrated_id = migrated_id.get().expect("active insert returned an id");

    assert_eq!(
        migrated_id, original_id,
        "active GlobalKey move must reuse the original ElementId",
    );
    assert_eq!(
        key.with_current_state::<i32>(CounterState::count),
        Some(17),
        "state must survive active-to-active GlobalKey reparent",
    );

    {
        let tree = tree.borrow();
        assert!(
            !direct_children_in_slot_order(&tree, parent_a).contains(&original_id),
            "old active parent must forget the moved GlobalKey child",
        );
        assert_eq!(
            direct_children_in_slot_order(&tree, parent_b),
            vec![original_id],
            "new parent must list the moved child at the claimed slot",
        );
        assert!(
            tree.get(original_id).is_some(),
            "the donor's later child snapshot must not finalize the synchronously forgotten child",
        );
    }

    let reparent_events: Vec<&CollectedEvent> = events
        .iter()
        .filter(|event| event.kind == ReconcileEventKind::Reparent)
        .collect();
    assert_eq!(
        reparent_events.len(),
        1,
        "exactly one active Reparent event expected; got {events:?}",
    );
    let reparent = reparent_events[0];
    assert_eq!(reparent.parent, parent_b.as_u64());
    assert_eq!(reparent.from_parent, Some(parent_a.as_u64()));
    assert_eq!(reparent.child_key, Some(key_hash));

    let mount_or_unmount = events
        .iter()
        .filter(|event| {
            matches!(
                event.kind,
                ReconcileEventKind::Mount | ReconcileEventKind::Unmount
            )
        })
        // The keyed child displaces the unkeyed leaf parent B built earlier;
        // that unmount is B's own child, not the migrated subtree.
        .filter(|event| event.parent != parent_b.as_u64() || event.child_key == Some(key_hash))
        .count();
    assert_eq!(
        mount_or_unmount, 0,
        "active reparent must not mount or unmount the migrated subtree",
    );

    flui_view::test_only_clear_global_key_registry();
}

// Suppress the unused-import warning for bump (the field exists in
// the fixture for future tests that exercise mutable state).
#[allow(dead_code)]
fn _force_use(state: &mut CounterState) {
    state.bump(1);
}
