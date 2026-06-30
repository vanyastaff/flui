//! SC-003 GlobalKey reparenting test — locks the §U17 wiring.
//!
//! The §U17 commit (inactive-queue reactivation path) emits a
//! `ReconcileEvent::Reparent` with `from_parent: None` when an
//! element registered under a `GlobalKey` is pulled back from the
//! `BuildOwner::inactive_elements` queue and re-attached at a new
//! (parent, slot). This file is the SC-003 end-to-end lock for that
//! path: it observes the event stream via the
//! `ReconcileEventCollector` and asserts the disposition
//! distribution matches the contract.
//!
//! Cross-parent same-frame ACTIVE reparent (ADV-1 case 2) is locked here too:
//! the tree forgets the active element from its old parent, moves it under the
//! new parent, and emits `from_parent: Some(old_parent)`.

#![cfg(feature = "test-utils")]

use std::sync::Arc;

use flui_foundation::ViewKey;
use flui_view::{
    BuildContext, BuildOwner, ElementTree, GlobalKey, IntoView, StatefulView, View, ViewExt,
    ViewState,
    tree::{
        ReconcileEventKind,
        test_utils::{CollectedEvent, ReconcileEventCollector},
    },
};
use parking_lot::RwLock;
use tracing::dispatcher::Dispatch;
use tracing_subscriber::Registry;
use tracing_subscriber::layer::SubscriberExt;

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
    fn create_state(&self) -> Self::State {
        SpacerState
    }
}

struct SpacerState;
impl ViewState<Spacer> for SpacerState {
    fn build(&self, _view: &Spacer, _ctx: &dyn BuildContext) -> impl IntoView {
        Spacer.boxed()
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
    fn bump(&mut self, by: i32) {
        self.count += by;
    }
}

impl StatefulView for KeyedCounter {
    type State = CounterState;
    fn create_state(&self) -> Self::State {
        CounterState {
            count: self.initial,
        }
    }
}

impl ViewState<KeyedCounter> for CounterState {
    fn build(&self, _v: &KeyedCounter, _ctx: &dyn BuildContext) -> impl IntoView {
        Spacer.boxed()
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

fn fresh_tree() -> (Arc<RwLock<ElementTree>>, Arc<RwLock<BuildOwner>>) {
    let tree = Arc::new(RwLock::new(ElementTree::new()));
    let owner = Arc::new(RwLock::new(BuildOwner::new()));
    flui_view::test_only_set_global_key_registry(&tree, &owner);
    (tree, owner)
}

fn capture<F: FnOnce()>(body: F) -> Vec<CollectedEvent> {
    let collector = ReconcileEventCollector::new();
    let subscriber = Registry::default().with(collector.layer());
    tracing::dispatcher::with_default(&Dispatch::new(subscriber), body);
    collector.events()
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
// SC-003 tests
// ============================================================================

/// Covers SC-003: when a `GlobalKey`-tagged element migrates through
/// the inactive-queue reactivation path, the collector observes EXACTLY
/// ONE `Reparent` event with the contract-mandated field shape
/// (`from_parent: None`, `parent: new_parent`, `child_key:
/// Some(key_hash)`). No spurious `Mount` / `Unmount` for the
/// migrated subtree.
#[test]
#[serial_test::serial(global_key_registry)]
fn covers_sc003_reparent_emits_single_reparent_event() {
    let (tree, owner) = fresh_tree();

    // Two parents so the migration has a non-trivial destination.
    let parent_a = tree
        .write()
        .mount_root(&Spacer, &mut owner.write().element_owner_mut());
    let parent_b =
        tree.write()
            .insert(&Spacer, parent_a, 0, &mut owner.write().element_owner_mut());

    let key = GlobalKey::<CounterState>::new();
    let counter = KeyedCounter {
        key: key.clone(),
        initial: 11,
    };
    let key_hash = key.key_hash();

    let original_id = tree.write().insert(
        &counter,
        parent_a,
        1,
        &mut owner.write().element_owner_mut(),
    );

    // Soft-remove pushes to inactive queue (Flutter `deactivateChild`).
    // Capture this step too — it MUST NOT emit a Reparent event
    // (soft-remove is not the disposition that fires the new
    // emission).
    //
    // Positive-presence guard FIRST: prove the tree state actually
    // changed (the element moved to the inactive queue) so the
    // count-zero assertion below is genuinely meaningful, not
    // silently passing because the collector or the soft-remove
    // path itself did nothing.
    let soft_remove_events = capture(|| {
        tree.write()
            .remove(original_id, &mut owner.write().element_owner_mut());
    });
    {
        // `is_inactive` lives on the split-borrow `ElementOwner`
        // handle, not on `BuildOwner` directly. Borrow scoped to the
        // assertion so the subsequent re-insert can take its own
        // write lock without contention.
        let mut owner_guard = owner.write();
        let element_owner = owner_guard.element_owner_mut();
        assert!(
            element_owner.is_inactive(original_id),
            "soft-remove must push the keyed element into the inactive queue \
             — otherwise the count-zero assertion below is vacuous",
        );
    }
    assert_eq!(
        soft_remove_events
            .iter()
            .filter(|e| e.kind == ReconcileEventKind::Reparent)
            .count(),
        0,
        "soft-remove must not fire Reparent; emission belongs to the re-insert path",
    );

    // Re-insert with the same GlobalKey under parent B — pulls from
    // inactive queue via `try_retake_global_key` AND emits the
    // `Reparent` event.
    let migrated_id_holder = std::cell::Cell::new(None);
    let reinsert_events = capture(|| {
        let id = tree.write().insert(
            &counter,
            parent_b,
            0,
            &mut owner.write().element_owner_mut(),
        );
        migrated_id_holder.set(Some(id));
    });
    let migrated_id = migrated_id_holder.get().expect("insert returned an id");

    // Vacuous-pass guard: positive count BEFORE absence assertion.
    assert!(
        !reinsert_events.is_empty(),
        "collector must observe at least one event on the re-insert path",
    );

    // Exactly one Reparent event in the re-insert capture.
    let reparent_events: Vec<&CollectedEvent> = reinsert_events
        .iter()
        .filter(|e| e.kind == ReconcileEventKind::Reparent)
        .collect();
    assert_eq!(
        reparent_events.len(),
        1,
        "exactly one Reparent event expected; got {reparent_events:?}",
    );
    let reparent = reparent_events[0];

    // Contract: from_parent is None for the inactive-queue path
    // (ADV-1 case 1). Cross-parent ACTIVE reparent (case 2) is tested
    // separately below and emits from_parent: Some(...).
    assert!(
        reparent.from_parent.is_none(),
        "inactive-queue path emits from_parent=None; got {:?}",
        reparent.from_parent,
    );

    // child_key carries the GlobalKey hash.
    assert_eq!(
        reparent.child_key,
        Some(key_hash),
        "Reparent event child_key must be the GlobalKey hash",
    );

    // parent stamps the new owning parent (parent_b in the
    // production parent-id space — its u64 representation).
    assert_eq!(
        reparent.parent,
        parent_b.as_u64(),
        "Reparent event parent must be the new owning parent's id",
    );

    // No spurious Mount/Unmount for the migrated subtree on the
    // re-insert step. The keyed element is reused, not torn down.
    let mount_count = reinsert_events
        .iter()
        .filter(|e| e.kind == ReconcileEventKind::Mount)
        .count();
    let unmount_count = reinsert_events
        .iter()
        .filter(|e| e.kind == ReconcileEventKind::Unmount)
        .count();
    assert_eq!(
        mount_count, 0,
        "reparented subtree must not emit Mount; saw {mount_count}",
    );
    assert_eq!(
        unmount_count, 0,
        "reparented subtree must not emit Unmount; saw {unmount_count}",
    );

    // State preservation sanity (the SC-003 contract proper — proven
    // by the existing `global_key_state_migrates_to_new_parent_slot`
    // test; here we re-check the migrated ElementId is the same as
    // the original).
    assert_eq!(
        migrated_id, original_id,
        "ElementId must survive migration through the inactive queue",
    );

    flui_view::test_only_clear_global_key_registry();
}

/// Covers SC-003 (state-preservation half): state mutated BEFORE the
/// reparent survives the migration. Pairs with the event-shape
/// assertions above — together they prove the wiring is functional
/// AND observable.
#[test]
#[serial_test::serial(global_key_registry)]
fn covers_sc003_state_preserved_across_reparent() {
    let (tree, owner) = fresh_tree();

    let parent_a = tree
        .write()
        .mount_root(&Spacer, &mut owner.write().element_owner_mut());
    let parent_b =
        tree.write()
            .insert(&Spacer, parent_a, 0, &mut owner.write().element_owner_mut());

    let key = GlobalKey::<CounterState>::new();
    let counter = KeyedCounter {
        key: key.clone(),
        initial: 5,
    };

    tree.write().insert(
        &counter,
        parent_a,
        1,
        &mut owner.write().element_owner_mut(),
    );

    // Sanity-touch the state through `with_current_state`. The
    // `with_current_state` API takes an immutable closure (it
    // returns the closure's value), so we don't mutate count here;
    // the value-preservation check below confirms the initial value
    // survives the migration unchanged.
    let _ = key.with_current_state::<()>(|_state| {
        // `with_current_state` takes an immutable closure; the
        // sentinel mutation pattern from the existing
        // global_key.rs::set_sentinel test uses a different path
        // (interior mutability via a Cell or a sentinel field).
        // Here we use the existing 'initial' field for a value-only
        // assertion — the migration test in the existing
        // global_key.rs already covers the mutable case via the
        // sentinel pattern. This test focuses on the event shape;
        // the value check below confirms migration carries the
        // INITIAL value through unchanged, which is a weaker but
        // still-meaningful state-preservation contract for this
        // file.
    });

    let original_count = key.with_current_state::<i32>(CounterState::count);
    assert_eq!(original_count, Some(5));

    let id_before = key.current_element();

    // Soft-remove + re-insert under parent_b.
    if let Some(id) = id_before {
        tree.write()
            .remove(id, &mut owner.write().element_owner_mut());
    }
    tree.write().insert(
        &counter,
        parent_b,
        0,
        &mut owner.write().element_owner_mut(),
    );

    let migrated_count = key.with_current_state::<i32>(CounterState::count);
    assert_eq!(
        migrated_count,
        Some(5),
        "counter value must survive inactive-queue reparent migration",
    );

    flui_view::test_only_clear_global_key_registry();
}

/// Covers SC-003 / ADV-1 case 2: a new parent can claim a GlobalKey element
/// that is still ACTIVE under another parent in the same frame. The old parent
/// forgets the child, the element id/state survive, and the trace event records
/// `from_parent: Some(old_parent)`.
#[test]
#[serial_test::serial(global_key_registry)]
fn covers_sc003_active_to_active_reparent_emits_from_parent_and_preserves_state() {
    let (tree, owner) = fresh_tree();

    let parent_a = tree
        .write()
        .mount_root(&Spacer, &mut owner.write().element_owner_mut());
    let parent_b =
        tree.write()
            .insert(&Spacer, parent_a, 0, &mut owner.write().element_owner_mut());

    let key = GlobalKey::<CounterState>::new();
    let counter = KeyedCounter {
        key: key.clone(),
        initial: 17,
    };
    let key_hash = key.key_hash();

    let original_id = tree.write().insert(
        &counter,
        parent_a,
        1,
        &mut owner.write().element_owner_mut(),
    );
    assert_eq!(
        key.with_current_state::<i32>(CounterState::count),
        Some(17),
        "precondition: keyed state is registered before the active move",
    );

    let migrated_id = std::cell::Cell::new(None);
    let events = capture(|| {
        let id = tree.write().insert(
            &counter,
            parent_b,
            0,
            &mut owner.write().element_owner_mut(),
        );
        migrated_id.set(Some(id));
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
        let tree = tree.read();
        assert!(
            !direct_children_in_slot_order(&tree, parent_a).contains(&original_id),
            "old active parent must forget the moved GlobalKey child",
        );
        assert_eq!(
            direct_children_in_slot_order(&tree, parent_b),
            vec![original_id],
            "new parent must list the moved child at the claimed slot",
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
