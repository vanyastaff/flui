//! Acceptance + edge-case tests for `BuildContext::depend_on_inherited`,
//! `InheritedBehavior::on_view_updated` dependent-notification, and
//! `BuildContext::get_inherited` (non-recording read).
//!
//! Plan U9 coverage:
//! - AE1 (R4): `depend_on_inherited::<T, _>` returns `Some(R)` and records
//!   the caller in the InheritedElement's dependent map.
//! - AE2 (R16): rebuilding the InheritedView with a value where
//!   `update_should_notify` returns `true` marks dependents dirty.
//! - Edge: no ancestor of `T` -> returns `None`, no dependent-set write.
//! - Edge: deduplication when the same element calls `depend_on` twice.
//! - Edge: an unmounted dependent's `ElementId` does not panic when
//!   `schedule_build_for` is invoked.
//!
//! Plan U10 coverage:
//! - AE3 (R5): `get_inherited::<T, _>` returns `Some(R)` BUT does NOT
//!   record the caller in the InheritedElement's dependent map. Used for
//!   one-time reads (settings/theme captured at mount).
//! - Edge: no ancestor of `T` -> returns `None`, no dependent-set write.
//!
//! Flutter parity: `framework.dart:5081` (`dependOnInheritedWidgetOfExactType`),
//! `framework.dart:5092` (`getInheritedWidgetOfExactType`, the
//! non-recording read), and `framework.dart:6414`
//! (`InheritedElement.notifyClients`).

use std::sync::Arc;

use flui_view::{
    BuildContext, BuildContextExt, BuildOwner, ElementBase, ElementBuildContext, ElementTree,
    InheritedElement, Lifecycle, StatelessBehavior, StatelessElement, StatelessView, View,
    element::InheritedBehavior, view::InheritedView,
};
use parking_lot::RwLock;

// ============================================================================
// Test fixtures: a simple `MyTheme` InheritedView and a leaf dependent View
// ============================================================================

#[derive(Clone, Debug, PartialEq)]
struct MyTheme {
    color: u32,
}

#[derive(Clone)]
struct DummyChild;

impl StatelessView for DummyChild {
    fn build(&self, _ctx: &dyn BuildContext) -> Box<dyn View> {
        Box::new(self.clone())
    }
}

impl View for DummyChild {
    fn create_element(&self) -> Box<dyn ElementBase> {
        Box::new(StatelessElement::new(self, StatelessBehavior))
    }
}

/// Test InheritedView providing `MyTheme` to descendants.
#[derive(Clone)]
struct ThemeProvider {
    theme: MyTheme,
    child: DummyChild,
}

impl InheritedView for ThemeProvider {
    type Data = MyTheme;

    fn data(&self) -> &Self::Data {
        &self.theme
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        self.theme != old.theme
    }
}

impl View for ThemeProvider {
    fn create_element(&self) -> Box<dyn ElementBase> {
        Box::new(InheritedElement::new(self, InheritedBehavior::new(self)))
    }
}

// ============================================================================
// Helpers
// ============================================================================

fn create_tree_and_owner() -> (Arc<RwLock<ElementTree>>, Arc<RwLock<BuildOwner>>) {
    let tree = Arc::new(RwLock::new(ElementTree::new()));
    let owner = Arc::new(RwLock::new(BuildOwner::new()));
    (tree, owner)
}

// ============================================================================
// AE1: depend_on returns Some(value) and records the dependent
// ============================================================================

#[test]
fn depend_on_returns_value_and_records_dependent() {
    // Tree: ThemeProvider (root) -> DummyChild (the dependent)
    let (tree, owner) = create_tree_and_owner();

    let provider = ThemeProvider {
        theme: MyTheme { color: 0x00FF_0000 },
        child: DummyChild,
    };

    let provider_id = tree
        .write()
        .mount_root(&provider, &mut owner.write().element_owner_mut());

    let child_id = tree.write().insert(
        &DummyChild,
        provider_id,
        0,
        &mut owner.write().element_owner_mut(),
    );

    let ctx = ElementBuildContext::for_element(child_id, tree.clone(), owner.clone()).unwrap();

    // Call depend_on::<ThemeProvider, _>(|view| view.theme.color)
    let color = ctx.depend_on::<ThemeProvider, u32>(|view| view.theme.color);
    assert_eq!(
        color,
        Some(0x00FF_0000),
        "depend_on should return the captured value"
    );

    // Verify the InheritedElement now lists child_id as a dependent.
    let tree_guard = tree.read();
    let provider_node = tree_guard.get(provider_id).expect("provider exists");
    let dependents_contains_child = {
        // ElementBase: Downcast (downcast-rs) — downcast directly via the
        // trait object to the concrete InheritedElement<ThemeProvider>.
        let elem = provider_node
            .element()
            .downcast_ref::<InheritedElement<ThemeProvider>>()
            .expect("provider is InheritedElement<ThemeProvider>");
        elem.dependents().contains_key(&child_id)
    };
    assert!(
        dependents_contains_child,
        "InheritedElement should record the caller in its dependent map"
    );
}

// ============================================================================
// AE2: rebuilding the InheritedView with update_should_notify=true marks
//      dependents dirty
// ============================================================================

#[test]
fn inherited_update_notifies_dependents() {
    // Same scaffolding as AE1
    let (tree, owner) = create_tree_and_owner();

    let provider_v1 = ThemeProvider {
        theme: MyTheme { color: 0x00FF_0000 },
        child: DummyChild,
    };

    let provider_id = tree
        .write()
        .mount_root(&provider_v1, &mut owner.write().element_owner_mut());

    let child_id = tree.write().insert(
        &DummyChild,
        provider_id,
        0,
        &mut owner.write().element_owner_mut(),
    );

    // Record dependency via depend_on
    {
        let ctx = ElementBuildContext::for_element(child_id, tree.clone(), owner.clone()).unwrap();
        let _ = ctx.depend_on::<ThemeProvider, ()>(|_| ());
    }

    // Confirm the dirty list is currently empty (registration alone does
    // not schedule a build).
    assert_eq!(
        owner.read().dirty_count(),
        0,
        "no dirty elements pre-update"
    );

    // Now rebuild the InheritedView with a different MyTheme.
    let provider_v2 = ThemeProvider {
        theme: MyTheme { color: 0x0000_FF00 },
        child: DummyChild,
    };
    tree.write().update(
        provider_id,
        &provider_v2,
        &mut owner.write().element_owner_mut(),
    );

    // The dependent (child_id) should now be marked dirty.
    assert_eq!(
        owner.read().dirty_count(),
        1,
        "dependent should be scheduled for rebuild"
    );
}

// ============================================================================
// Edge: no ancestor InheritedView -> returns None, no dependent-set write
// ============================================================================

#[test]
fn depend_on_returns_none_when_no_ancestor() {
    let (tree, owner) = create_tree_and_owner();

    // Tree: DummyChild (root) — NO ThemeProvider above
    let root_id = tree
        .write()
        .mount_root(&DummyChild, &mut owner.write().element_owner_mut());

    let ctx = ElementBuildContext::for_element(root_id, tree.clone(), owner.clone()).unwrap();
    let result = ctx.depend_on::<ThemeProvider, u32>(|view| view.theme.color);
    assert_eq!(result, None, "no ThemeProvider ancestor -> None");
}

// ============================================================================
// Edge: same element calls depend_on twice in one build -> dedup
// ============================================================================

#[test]
fn depend_on_deduplicates_per_dependent() {
    let (tree, owner) = create_tree_and_owner();

    let provider = ThemeProvider {
        theme: MyTheme { color: 0x00FF_0000 },
        child: DummyChild,
    };

    let provider_id = tree
        .write()
        .mount_root(&provider, &mut owner.write().element_owner_mut());

    let child_id = tree.write().insert(
        &DummyChild,
        provider_id,
        0,
        &mut owner.write().element_owner_mut(),
    );

    let ctx = ElementBuildContext::for_element(child_id, tree.clone(), owner.clone()).unwrap();

    // Call twice in the same "build"
    let _ = ctx.depend_on::<ThemeProvider, ()>(|_| ());
    let _ = ctx.depend_on::<ThemeProvider, ()>(|_| ());

    // Dependent map should have exactly one entry for child_id.
    let tree_guard = tree.read();
    let provider_node = tree_guard.get(provider_id).expect("provider exists");
    let elem = provider_node
        .element()
        .downcast_ref::<InheritedElement<ThemeProvider>>()
        .expect("provider is InheritedElement<ThemeProvider>");
    assert_eq!(
        elem.dependents().len(),
        1,
        "duplicate depend_on calls should not create a second dependent entry"
    );
    assert!(elem.dependents().contains_key(&child_id));
}

// ============================================================================
// Edge: unmounted dependent — schedule_build_for is a no-op (no panic)
// ============================================================================

#[test]
fn unmounted_dependent_no_op_on_schedule() {
    let (tree, owner) = create_tree_and_owner();

    let provider_v1 = ThemeProvider {
        theme: MyTheme { color: 0x00FF_0000 },
        child: DummyChild,
    };

    let provider_id = tree
        .write()
        .mount_root(&provider_v1, &mut owner.write().element_owner_mut());

    let child_id = tree.write().insert(
        &DummyChild,
        provider_id,
        0,
        &mut owner.write().element_owner_mut(),
    );

    // Register as dependent.
    {
        let ctx = ElementBuildContext::for_element(child_id, tree.clone(), owner.clone()).unwrap();
        let _ = ctx.depend_on::<ThemeProvider, ()>(|_| ());
    }

    // Remove the dependent from the tree before updating the inherited.
    tree.write()
        .remove(child_id, &mut owner.write().element_owner_mut());

    // Now update the provider with a different value. The on-view-updated
    // path will walk the (stale) dependent set and call
    // schedule_build_for for an ElementId no longer in the tree. This
    // must not panic — it is allowed to push a stale id to the heap;
    // BuildOwner::build_scope tolerates missing ids.
    let provider_v2 = ThemeProvider {
        theme: MyTheme { color: 0x0000_FF00 },
        child: DummyChild,
    };
    tree.write().update(
        provider_id,
        &provider_v2,
        &mut owner.write().element_owner_mut(),
    );

    // Lifecycle of the now-removed child can no longer be inspected, but
    // the test passes if the update path did not panic.
    let _ = Lifecycle::Defunct; // suppress unused-import lint if any
}

// ============================================================================
// AE3 (U10 / R5): get_inherited returns the value WITHOUT recording a
// dependent — Flutter parity framework.dart:5092
// `getInheritedWidgetOfExactType` (no `updateDependencies` call).
// ============================================================================

#[test]
fn get_inherited_returns_value_without_recording_dependent() {
    // Tree: ThemeProvider (root) -> DummyChild (would-be dependent, but
    // calls `get` not `depend_on`, so it must NOT be recorded).
    let (tree, owner) = create_tree_and_owner();

    let provider = ThemeProvider {
        theme: MyTheme { color: 0x00FF_0000 },
        child: DummyChild,
    };

    let provider_id = tree
        .write()
        .mount_root(&provider, &mut owner.write().element_owner_mut());

    let child_id = tree.write().insert(
        &DummyChild,
        provider_id,
        0,
        &mut owner.write().element_owner_mut(),
    );

    let ctx = ElementBuildContext::for_element(child_id, tree.clone(), owner.clone()).unwrap();

    // Sibling assertion to AE1: same tree shape, same closure, but `get`
    // instead of `depend_on`. The value is returned identically; only
    // the dependent-set side-effect differs.
    let color = ctx.get::<ThemeProvider, u32>(|view| view.theme.color);
    assert_eq!(
        color,
        Some(0x00FF_0000),
        "get should return the captured value (same as depend_on)"
    );

    // Critical assertion: the dependent map is EMPTY. If `get_inherited`
    // were ever to call `record_dependent`, this would fail with
    // `dependents().len() == 1`. The parallel to AE1
    // (`depend_on_returns_value_and_records_dependent`) where the same
    // tree shape yields `dependents().contains_key(&child_id) == true`
    // is what locks down the non-recording semantic.
    let tree_guard = tree.read();
    let provider_node = tree_guard.get(provider_id).expect("provider exists");
    let elem = provider_node
        .element()
        .downcast_ref::<InheritedElement<ThemeProvider>>()
        .expect("provider is InheritedElement<ThemeProvider>");
    assert!(
        elem.dependents().is_empty(),
        "get_inherited must NOT record the caller in the dependent map \
         (Flutter parity framework.dart:5092 — getInheritedWidgetOfExactType \
         does not call updateDependencies). Found {} entries: {:?}",
        elem.dependents().len(),
        elem.dependents().keys().collect::<Vec<_>>(),
    );
}

// ============================================================================
// Edge (U10): get_inherited returns None when no ancestor InheritedView
// of that type exists — no dependent-set write happens because nothing
// was found to write into.
// ============================================================================

#[test]
fn get_inherited_returns_none_when_no_ancestor() {
    let (tree, owner) = create_tree_and_owner();

    // Tree: DummyChild (root) — NO ThemeProvider above
    let root_id = tree
        .write()
        .mount_root(&DummyChild, &mut owner.write().element_owner_mut());

    let ctx = ElementBuildContext::for_element(root_id, tree.clone(), owner.clone()).unwrap();
    let result = ctx.get::<ThemeProvider, u32>(|view| view.theme.color);
    assert_eq!(result, None, "no ThemeProvider ancestor -> None");
}
