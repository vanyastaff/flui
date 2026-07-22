//! Acceptance + edge-case tests for `GlobalKey` register / unregister,
//! same-frame state migration, and `current_element` / `current_state`
//! lookup.
//!
//! Covers: registering on mount, pushing to the inactive queue on
//! unmount with same-frame state migration on remount (finalized at
//! end-of-frame), and the lookup methods.
//!
//! Flutter parity:
//! - `framework.dart:3148`  — `_globalKeyRegistry` (BuildOwner.globalKeyRegistry).
//! - `framework.dart:4571`  — `_retakeInactiveElement` (pull keyed element back).
//! - `framework.dart:4636`  — `deactivateChild` (push onto `_inactiveElements`).
//! - `framework.dart:2099`  — `_InactiveElements` queue + finalization ordering.
//!
//! These tests are written TEST-FIRST: before this wiring lands,
//! `current_element`/`current_state` return `None` and reconciliation
//! creates fresh state on remount; the implementation below makes them
//! pass.

// ADR-0027: ElementBuildContext's current test/prod seam still takes
// Arc<RwLock<ElementTree/BuildOwner>>. The owner graph is !Send; do not restore
// Send + Sync to satisfy clippy. Future UiRealm/Rc migration should remove this.
#![allow(clippy::arc_with_non_send_sync)]

use std::sync::Arc;

use flui_foundation::ViewKey;
use flui_view::{
    BuildContext, BuildOwner, ElementBase, ElementTree, GlobalKey, IntoView, StatefulElement,
    StatefulView, StatelessView, View, ViewExt, ViewState,
};
use parking_lot::RwLock;

// ============================================================================
// Test fixtures
// ============================================================================

/// A leaf StatelessView used as a child / spacer.
#[derive(Clone)]
struct Spacer;

impl StatelessView for Spacer {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        self.clone().boxed()
    }
}

impl View for Spacer {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

/// A StatefulView carrying a `GlobalKey` and an initial counter value.
///
/// `key` is wrapped in `Option` because `Default` is not derived (and a
/// global key counter would otherwise tick on every construction).
#[derive(Clone)]
struct KeyedCounter {
    key: GlobalKey<KeyedCounterState>,
    initial: i32,
}

struct KeyedCounterState {
    /// Persists across rebuilds so we can detect state-recreation.
    count: i32,

    /// Sentinel value seeded on `init_state` then mutated by tests. If
    /// the state migrates, the sentinel survives; if it's a fresh state,
    /// the default value `0` is observed.
    sentinel: u64,
}

impl KeyedCounterState {
    fn count(&self) -> i32 {
        self.count
    }

    fn sentinel(&self) -> u64 {
        self.sentinel
    }
}

impl StatefulView for KeyedCounter {
    type State = KeyedCounterState;

    fn create_state(&self) -> Self::State {
        KeyedCounterState {
            count: self.initial,
            sentinel: 0,
        }
    }
}

impl ViewState<KeyedCounter> for KeyedCounterState {
    fn build(&self, _view: &KeyedCounter, _ctx: &dyn BuildContext) -> impl IntoView {
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
    (
        Arc::new(RwLock::new(ElementTree::new())),
        Arc::new(RwLock::new(BuildOwner::new())),
    )
}

/// Mutate the sentinel on the matching keyed element's state. Returns
/// `true` if the state was reachable + mutated.
fn set_sentinel(
    tree: &Arc<RwLock<ElementTree>>,
    key: &GlobalKey<KeyedCounterState>,
    value: u64,
) -> bool {
    let Some(id) = key.current_element() else {
        return false;
    };
    let mut tree_guard = tree.write();
    let Some(node) = tree_guard.get_mut(id) else {
        return false;
    };
    let element = node.element_mut();
    let downcast: &mut StatefulElement<KeyedCounter> =
        match (element as &mut dyn ElementBase).downcast_mut::<StatefulElement<KeyedCounter>>() {
            Some(e) => e,
            None => return false,
        };
    downcast.state_mut().sentinel = value;
    true
}

// ============================================================================
// Happy path — `current_state` after mount
// ============================================================================

/// After a keyed stateful element is mounted under a root,
/// `key.current_element()` returns the element's id and `key.current_state()`
/// surfaces the same `&KeyedCounterState` the framework stored on the
/// element.
#[test]
#[serial_test::serial(global_key_registry)]
fn global_key_current_state_after_mount() {
    let (tree, owner) = fresh_tree();

    let key = GlobalKey::<KeyedCounterState>::new();
    let counter = KeyedCounter {
        key: key.clone(),
        initial: 7,
    };

    let id = tree
        .write()
        .mount_root(&counter, &mut owner.write().element_owner_mut());

    // Install the registry handle so `current_*` finds this tree.
    flui_view::test_only_set_global_key_registry(&tree, &owner);

    let resolved_id = key.current_element();
    assert_eq!(
        resolved_id,
        Some(id),
        "current_element should map back to the mounted element id"
    );

    let snapshot = key.with_current_state::<i32>(KeyedCounterState::count);
    assert_eq!(
        snapshot,
        Some(7),
        "current_state surfaces the live KeyedCounterState"
    );

    flui_view::test_only_clear_global_key_registry();
}

// ============================================================================
// State migration — same-frame reparenting preserves state identity
// ============================================================================

/// The keyed element is unmounted from one parent
/// slot and re-inserted under a different parent (same frame). The
/// underlying state survives the move — `count` AND a sentinel mutated
/// after the first mount stay intact, proving the state was migrated,
/// not recreated.
///
/// Flutter parity: `framework.dart:4571` `_retakeInactiveElement` pulls
/// the previously-keyed element out of `_inactiveElements` and re-mounts
/// it under the new parent.
#[test]
#[serial_test::serial(global_key_registry)]
fn global_key_state_migrates_to_new_parent_slot() {
    let (tree, owner) = fresh_tree();

    // Two distinct parents to migrate between.
    let parent_a = tree
        .write()
        .mount_root(&Spacer, &mut owner.write().element_owner_mut());
    let parent_b =
        tree.write()
            .insert(&Spacer, parent_a, 0, &mut owner.write().element_owner_mut());

    let key = GlobalKey::<KeyedCounterState>::new();
    let counter = KeyedCounter {
        key: key.clone(),
        initial: 11,
    };

    // Mount under parent A.
    let original_id = tree.write().insert(
        &counter,
        parent_a,
        1,
        &mut owner.write().element_owner_mut(),
    );

    // Make state mutation observable via sentinel.
    flui_view::test_only_set_global_key_registry(&tree, &owner);
    assert!(set_sentinel(&tree, &key, 0xCAFE_BABE));

    // Soft-remove from parent A — should push to inactive (Flutter
    // `deactivateChild`), not slab-remove.
    tree.write()
        .remove(original_id, &mut owner.write().element_owner_mut());

    // Re-insert with the same GlobalKey under parent B. The keyed
    // element must be pulled back from the inactive queue instead of a
    // fresh element being created.
    let migrated_id = tree.write().insert(
        &counter,
        parent_b,
        0,
        &mut owner.write().element_owner_mut(),
    );

    assert_eq!(
        migrated_id, original_id,
        "state migration should reuse the same ElementId across the move"
    );

    // State is still reachable and the sentinel survived the migration.
    let count = key.with_current_state::<i32>(KeyedCounterState::count);
    let sentinel = key.with_current_state::<u64>(KeyedCounterState::sentinel);
    assert_eq!(
        count,
        Some(11),
        "count should be preserved across migration"
    );
    assert_eq!(
        sentinel,
        Some(0xCAFE_BABE),
        "sentinel mutation written before migration must survive"
    );

    flui_view::test_only_clear_global_key_registry();
}

// ============================================================================
// Cleanup — full unmount drops the GlobalKey registration
// ============================================================================

/// After a keyed element is unmounted and the end-of-frame
/// `finalize_tree` drains the inactive queue, `current_element` and
/// `current_state` must return `None`. The registry entry is cleared
/// once the element is truly gone.
#[test]
#[serial_test::serial(global_key_registry)]
fn global_key_returns_none_after_full_unmount() {
    let (tree, owner) = fresh_tree();

    let key = GlobalKey::<KeyedCounterState>::new();
    let counter = KeyedCounter {
        key: key.clone(),
        initial: 99,
    };

    let id = tree
        .write()
        .mount_root(&counter, &mut owner.write().element_owner_mut());

    flui_view::test_only_set_global_key_registry(&tree, &owner);
    assert_eq!(key.current_element(), Some(id));

    // Soft-remove (push to inactive).
    tree.write()
        .remove(id, &mut owner.write().element_owner_mut());

    // End-of-frame finalize — no remount happened, so the element is
    // unregistered + removed.
    owner.write().finalize_tree(&mut tree.write());

    assert_eq!(
        key.current_element(),
        None,
        "after finalize_tree drains inactive, registry should be empty"
    );
    assert_eq!(
        key.with_current_state::<i32>(KeyedCounterState::count),
        None,
        "current_state returns None once the element is finalized"
    );

    flui_view::test_only_clear_global_key_registry();
}

// ============================================================================
// Duplicate GlobalKey — debug panic + release diagnostic
// ============================================================================

/// Two active elements cannot share the same `GlobalKey`. Flutter panics in
/// debug; FLUI mirrors that with a debug panic before a duplicate child can
/// enter the element tree.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "duplicate GlobalKey children are not allowed")]
fn duplicate_global_key_panics_in_debug() {
    let (tree, owner) = fresh_tree();

    // Forge two `KeyedCounter`s that share the same GlobalKey id by
    // cloning. Cloning a GlobalKey preserves the id by design — that's
    // what tests-of-key-equality rely on. Mounting two distinct active
    // elements with the same key is the duplicate-GlobalKey defect we catch.
    let key = GlobalKey::<KeyedCounterState>::new();
    let counter_a = KeyedCounter {
        key: key.clone(),
        initial: 1,
    };
    let counter_b = KeyedCounter {
        key: key.clone(),
        initial: 2,
    };

    let root_id = tree
        .write()
        .mount_root(&Spacer, &mut owner.write().element_owner_mut());

    // First mount registers the key.
    let _ = tree.write().insert(
        &counter_a,
        root_id,
        0,
        &mut owner.write().element_owner_mut(),
    );

    // Second mount with the same GlobalKey should hit the debug-panic.
    let _ = tree.write().insert(
        &counter_b,
        root_id,
        1,
        &mut owner.write().element_owner_mut(),
    );
}

// ============================================================================
// Type-bound enforcement — non-`StatefulView` types compile-only check
// ============================================================================

/// Compile-time check: `GlobalKey<T>` doesn't require `T: StatefulView`
/// at construction. Lookup is the place the constraint should be felt —
/// `with_current_state<R>` only matches when the underlying element's
/// `state_as_any()` downcasts back to the GlobalKey's `T`. A `GlobalKey`
/// instantiated over a non-`State` type (e.g. `i32`) simply never finds
/// a matching state. This is the same shape as Flutter, where
/// `GlobalKey<S extends State>` is the typed surface and the registry
/// is hash-keyed regardless.
///
/// This test holds the line so a future refactor doesn't accidentally
/// require `T: StatefulView` on construction. The compile would fail
/// here if that ever changed.
#[test]
fn global_key_construction_accepts_any_type() {
    let _: GlobalKey<i32> = GlobalKey::new();
    let _: GlobalKey<String> = GlobalKey::new();
    let _: GlobalKey<KeyedCounterState> = GlobalKey::new();
}

// ============================================================================
// Stress — state preserved across 100 reparents
// ============================================================================

/// Stress test. Mount a keyed element under a sequence of parents and
/// reparent it 100 times via the soft-remove / re-insert dance.
/// The element's state identity (id + sentinel) must survive every
/// reparent — no fresh state is ever created. The registry stays at
/// exactly one entry throughout.
#[test]
#[serial_test::serial(global_key_registry)]
fn global_key_state_preserved_across_100_reparents() {
    let (tree, owner) = fresh_tree();
    flui_view::test_only_set_global_key_registry(&tree, &owner);

    // Two parents we alternate between.
    let parent_a = tree
        .write()
        .mount_root(&Spacer, &mut owner.write().element_owner_mut());
    let parent_b =
        tree.write()
            .insert(&Spacer, parent_a, 0, &mut owner.write().element_owner_mut());

    let key = GlobalKey::<KeyedCounterState>::new();
    let counter = KeyedCounter {
        key: key.clone(),
        initial: 42,
    };

    let original_id = tree.write().insert(
        &counter,
        parent_a,
        1,
        &mut owner.write().element_owner_mut(),
    );

    // Seed a sentinel that must survive the entire reparent storm.
    let sentinel_value: u64 = 0xDEAD_BEEF_FEED_FACE;
    assert!(set_sentinel(&tree, &key, sentinel_value));

    for cycle in 0_usize..100 {
        let target_parent = if cycle % 2 == 0 { parent_b } else { parent_a };

        // Soft-remove (push to inactive).
        tree.write()
            .remove(original_id, &mut owner.write().element_owner_mut());

        // Re-insert under the alternating parent.
        let id_now = tree.write().insert(
            &counter,
            target_parent,
            cycle % 4,
            &mut owner.write().element_owner_mut(),
        );

        assert_eq!(
            id_now, original_id,
            "cycle {cycle}: reparent should reuse the same ElementId"
        );

        assert_eq!(
            key.current_element(),
            Some(original_id),
            "cycle {cycle}: registry should still resolve to the migrated element",
        );

        assert_eq!(
            key.with_current_state::<u64>(KeyedCounterState::sentinel),
            Some(sentinel_value),
            "cycle {cycle}: sentinel must survive every reparent",
        );

        // Registry never grows past one entry.
        assert_eq!(
            owner.read().global_keys_len(),
            1,
            "cycle {cycle}: registry should hold exactly one entry",
        );
    }

    flui_view::test_only_clear_global_key_registry();
}
