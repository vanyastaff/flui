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
    InheritedElement, IntoView, Lifecycle, StatelessBehavior, StatelessElement, StatelessView,
    View, ViewExt, element::InheritedBehavior, view::InheritedView,
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
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        self.clone().boxed()
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

// ============================================================================
// U14 (R16, audit V-19): wire `did_change_dependencies` to inherited
// updates.
//
// When `InheritedView::update_should_notify` returns `true`, the
// dependent's typed `ViewState::did_change_dependencies` hook fires
// exactly once, BEFORE the dependent's `perform_build`. Mirrors Flutter
// `framework.dart:5977-5982` `StatefulElement.performRebuild` reading
// the `_didChangeDependencies` flag set at `framework.dart:6117`.
// ============================================================================

mod did_change_dependencies_on_inherited_update {
    use std::{
        any::TypeId,
        sync::{Arc, Mutex},
    };

    use flui_foundation::ElementId;
    use flui_view::{
        BuildContext, BuildOwner, ElementBase, ElementOwner, ElementTree, IntoView, Lifecycle,
        StatefulBehavior, StatefulElement, StatefulView, View, ViewExt, ViewState,
    };

    use super::{DummyChild, MyTheme};

    /// Shared probe recording lifecycle ordering. Each entry is one
    /// observed event tag ("dcd:N" for the Nth `did_change_dependencies`
    /// call, "build" for a `build` call).
    type Probe = Mutex<Vec<String>>;

    // ========================================================================
    // Terminal leaf — the build chain must terminate, so probe-stateful
    // views and the stateless dependent both return this leaf as their
    // child instead of self.
    // ========================================================================

    #[derive(Clone)]
    struct LeafView;

    impl View for LeafView {
        fn create_element(&self) -> Box<dyn ElementBase> {
            Box::new(LeafElement::new())
        }
    }

    struct LeafElement {
        depth: usize,
        lifecycle: Lifecycle,
    }

    impl LeafElement {
        fn new() -> Self {
            Self {
                depth: 0,
                lifecycle: Lifecycle::Initial,
            }
        }
    }

    impl ElementBase for LeafElement {
        fn view_type_id(&self) -> TypeId {
            TypeId::of::<LeafView>()
        }

        fn depth(&self) -> usize {
            self.depth
        }

        fn lifecycle(&self) -> Lifecycle {
            self.lifecycle
        }

        fn mount(
            &mut self,
            _parent: Option<ElementId>,
            slot: usize,
            _owner: &mut ElementOwner<'_>,
        ) {
            self.depth = slot;
            self.lifecycle = Lifecycle::Active;
        }

        fn unmount(&mut self, _owner: &mut ElementOwner<'_>) {
            self.lifecycle = Lifecycle::Defunct;
        }

        fn activate(&mut self) {
            self.lifecycle = Lifecycle::Active;
        }

        fn deactivate(&mut self) {
            self.lifecycle = Lifecycle::Inactive;
        }

        fn update(&mut self, _new_view: &dyn View, _owner: &mut ElementOwner<'_>) {}

        fn mark_needs_build(&mut self) {}

        fn build_into_views(&mut self, _owner: &mut ElementOwner<'_>) -> Vec<Box<dyn View>> {
            Vec::new()
        }
    }

    // ========================================================================
    // Stateful dependent that records `did_change_dependencies` + `build`
    // invocations into a shared probe.
    // ========================================================================

    #[derive(Clone)]
    struct ProbeDependent {
        probe: Arc<Probe>,
    }

    struct ProbeDependentState {
        probe: Arc<Probe>,
        dcd_calls: usize,
    }

    impl StatefulView for ProbeDependent {
        type State = ProbeDependentState;

        fn create_state(&self) -> Self::State {
            ProbeDependentState {
                probe: self.probe.clone(),
                dcd_calls: 0,
            }
        }
    }

    impl ViewState<ProbeDependent> for ProbeDependentState {
        fn did_change_dependencies(&mut self, _ctx: &dyn BuildContext) {
            self.dcd_calls += 1;
            self.probe
                .lock()
                .unwrap()
                .push(format!("dcd:{}", self.dcd_calls));
        }

        fn build(&self, _view: &ProbeDependent, _ctx: &dyn BuildContext) -> impl IntoView {
            self.probe.lock().unwrap().push("build".to_string());
            LeafView.boxed()
        }
    }

    impl View for ProbeDependent {
        fn create_element(&self) -> Box<dyn ElementBase> {
            Box::new(StatefulElement::new(self, StatefulBehavior::new(self)))
        }
    }

    // ========================================================================
    // Stateless dependent — exercises the default no-op
    // `ElementBase::notify_dependency_change` path. Build returns a true
    // leaf so build_scope terminates.
    // ========================================================================

    #[derive(Clone)]
    struct StatelessProbeDependent;

    impl flui_view::StatelessView for StatelessProbeDependent {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            LeafView.boxed()
        }
    }

    impl View for StatelessProbeDependent {
        fn create_element(&self) -> Box<dyn ElementBase> {
            Box::new(flui_view::StatelessElement::new(
                self,
                flui_view::StatelessBehavior,
            ))
        }
    }

    /// Helper: mount `ThemeProvider(color) -> dependent` and register
    /// `dependent` as an inherited dependent via a `depend_on` call.
    fn mount_provider_and_record_dependency(
        tree: &mut ElementTree,
        owner: &mut BuildOwner,
        color: u32,
        dependent_view: &dyn View,
    ) -> (flui_foundation::ElementId, flui_foundation::ElementId) {
        let provider = super::ThemeProvider {
            theme: MyTheme { color },
            child: DummyChild,
        };

        let provider_id = tree.mount_root(&provider, &mut owner.element_owner_mut());

        let dep_id = tree.insert(
            dependent_view,
            provider_id,
            0,
            &mut owner.element_owner_mut(),
        );

        // Register the dependent via the actual access protocol so we
        // exercise the same code path production uses. We can't pass an
        // ElementBuildContext here because the test holds direct `&mut`
        // borrows on tree+owner — instead, call `record_dependent`
        // through the `InheritedElementAccess` trait on the provider.
        {
            use flui_view::InheritedElement;
            let provider_node = tree.get_mut(provider_id).expect("provider exists");
            let dep_depth = 1;
            let element = provider_node
                .element_mut()
                .downcast_mut::<InheritedElement<super::ThemeProvider>>()
                .expect("provider is InheritedElement<ThemeProvider>");
            element.behavior_mut().add_dependent(dep_id, dep_depth);
        }

        (provider_id, dep_id)
    }

    #[test]
    fn fires_typed_hook_exactly_once_before_rebuild() {
        let mut tree = ElementTree::new();
        let mut owner = BuildOwner::new();
        let probe: std::sync::Arc<Probe> = std::sync::Arc::new(Mutex::new(Vec::new()));

        let dependent_view = ProbeDependent {
            probe: probe.clone(),
        };

        let (provider_id, dep_id) = mount_provider_and_record_dependency(
            &mut tree,
            &mut owner,
            0x00FF_0000,
            &dependent_view,
        );

        // Sanity: no events recorded yet — probe vec is empty because
        // mount runs init_state but not the build proper (perform_build
        // is only driven via build_scope or an explicit rebuild trigger).
        probe.lock().unwrap().clear();

        // Update the InheritedView with a new color so update_should_notify
        // returns true.
        let provider_v2 = super::ThemeProvider {
            theme: MyTheme { color: 0x0000_FF00 },
            child: DummyChild,
        };
        tree.update(provider_id, &provider_v2, &mut owner.element_owner_mut());

        // Pre-build invariants:
        // - dependent is scheduled for rebuild
        // - pending_dependency_change flag is set on the dependent
        assert_eq!(
            owner.dirty_count(),
            1,
            "dependent should be scheduled for rebuild"
        );
        assert!(
            owner
                .element_owner_mut()
                .has_pending_dependency_change(dep_id),
            "InheritedBehavior::on_view_updated should mark the dependent for a typed-hook dispatch"
        );

        // Nothing fires until the build phase runs — the flag is set
        // but `did_change_dependencies` itself hasn't been called yet.
        assert!(
            probe.lock().unwrap().is_empty(),
            "did_change_dependencies must not fire until perform_build runs (Flutter parity: \
             flag-then-fire, framework.dart:5977-5982)"
        );

        // Drive the build phase. `build_scope` reads the
        // `pending_dependency_changes` flag, fires
        // `notify_dependency_change` BEFORE `perform_build`, then runs
        // the build itself.
        owner.build_scope(&mut tree);

        // Inspect the recorded sequence. The integration contract:
        // `dcd:1` (the typed hook) appears BEFORE `build` (the rebuild).
        let events = probe.lock().unwrap().clone();
        assert!(
            events.iter().any(|e| e == "dcd:1"),
            "ViewState::did_change_dependencies must fire exactly once. recorded: {:?}",
            events
        );
        // Exactly once: the typed hook must not have fired a second
        // time. `ProbeDependentState.dcd_calls` increments on every
        // call, so a second invocation would surface as `dcd:2` in the
        // probe vec.
        assert!(
            !events.iter().any(|e| e == "dcd:2"),
            "ViewState::did_change_dependencies fired more than once. recorded: {:?}",
            events
        );
        let dcd_idx = events
            .iter()
            .position(|e| e == "dcd:1")
            .expect("dcd:1 present");
        let build_idx = events
            .iter()
            .position(|e| e == "build")
            .expect("build present (perform_build must run after notify_dependency_change)");
        assert!(
            dcd_idx < build_idx,
            "did_change_dependencies must fire BEFORE build. recorded: {:?}",
            events
        );
        // Sequencing contract: the recorded order is exactly
        // [dcd:1, build] — `dcd` immediately precedes `build` with no
        // intervening events for this dependent. Pinning the full
        // vector also asserts "exactly two events" (no spurious extras).
        assert_eq!(
            events,
            vec!["dcd:1".to_string(), "build".to_string()],
            "expected recorded order [dcd:1, build]"
        );

        // After the build, the flag must be cleared so a subsequent
        // unrelated rebuild does NOT re-fire the typed hook.
        assert!(
            !owner
                .element_owner_mut()
                .has_pending_dependency_change(dep_id),
            "pending_dependency_changes must be cleared after build_scope dispatches the hook"
        );
    }

    /// E3 regression: a dependent that has ALREADY built once (and is
    /// therefore clean) still rebuilds when its inherited dependency
    /// changes.
    ///
    /// `InheritedBehavior::on_view_updated` schedules the dependent and
    /// records a pending dependency change, but it cannot set the
    /// dependent's own dirty flag (it has no slab access). `build_scope`'s
    /// dirty guard skips any popped entry whose `is_dirty()` is false, so
    /// without the guard promoting a pending dependency change to a dirty
    /// mark, a clean dependent would be popped, skipped, and never observe
    /// the change. The sibling `fires_typed_hook_exactly_once_before_rebuild`
    /// does NOT catch this: there the dependent is dirty-from-birth (it has
    /// never built), so the guard passes regardless.
    #[test]
    fn clean_dependent_rebuilds_on_dependency_change() {
        let mut tree = ElementTree::new();
        let mut owner = BuildOwner::new();
        let probe: std::sync::Arc<Probe> = std::sync::Arc::new(Mutex::new(Vec::new()));

        let dependent_view = ProbeDependent {
            probe: probe.clone(),
        };

        let (provider_id, dep_id) = mount_provider_and_record_dependency(
            &mut tree,
            &mut owner,
            0x00FF_0000,
            &dependent_view,
        );

        // Build #1: drive the dependent's first build so it transitions
        // from dirty-from-birth to CLEAN. (Insert does not schedule, so the
        // dependent must be scheduled explicitly here — production schedules
        // it from the parent's reconcile.)
        owner.schedule_build_for(dep_id, 1);
        owner.build_scope(&mut tree);
        assert_eq!(
            probe.lock().unwrap().clone(),
            vec!["build".to_string()],
            "first build runs once (no dependency change pending yet, so no dcd)",
        );
        probe.lock().unwrap().clear();

        // Now change the inherited value. The dependent is clean (its dirty
        // flag was cleared by build #1), so this is exactly the bug
        // condition: scheduled + pending-change, but not self-dirty.
        let provider_v2 = super::ThemeProvider {
            theme: MyTheme { color: 0x0000_FF00 },
            child: DummyChild,
        };
        tree.update(provider_id, &provider_v2, &mut owner.element_owner_mut());
        assert!(
            owner
                .element_owner_mut()
                .has_pending_dependency_change(dep_id),
            "the dependency change marks the clean dependent for a typed-hook dispatch",
        );

        // Build #2: the clean dependent MUST rebuild — dcd:1 then build.
        owner.build_scope(&mut tree);
        let events = probe.lock().unwrap().clone();
        assert_eq!(
            events,
            vec!["dcd:1".to_string(), "build".to_string()],
            "a clean dependent must observe the dependency change: \
             expected [dcd:1, build], got {events:?}",
        );
    }

    #[test]
    fn no_notify_means_no_typed_hook_dispatch() {
        let mut tree = ElementTree::new();
        let mut owner = BuildOwner::new();
        let probe: std::sync::Arc<Probe> = std::sync::Arc::new(Mutex::new(Vec::new()));

        let dependent_view = ProbeDependent {
            probe: probe.clone(),
        };

        // Mount with color 0xAA so the next update with the SAME color
        // returns `update_should_notify == false`.
        let (provider_id, dep_id) = mount_provider_and_record_dependency(
            &mut tree,
            &mut owner,
            0x00AA_BBCC,
            &dependent_view,
        );
        probe.lock().unwrap().clear();

        // Update with the same MyTheme value — update_should_notify is
        // `self.theme != old.theme`, which is false here.
        let provider_v2 = super::ThemeProvider {
            theme: MyTheme { color: 0x00AA_BBCC },
            child: DummyChild,
        };
        tree.update(provider_id, &provider_v2, &mut owner.element_owner_mut());

        // No dependent should be scheduled, no pending typed-hook flag.
        assert_eq!(
            owner.dirty_count(),
            0,
            "update_should_notify=false must not schedule the dependent"
        );
        assert!(
            !owner
                .element_owner_mut()
                .has_pending_dependency_change(dep_id),
            "update_should_notify=false must not set the pending dependency-change flag"
        );

        // Even if a build runs (it has nothing to do), no typed hook
        // fires.
        owner.build_scope(&mut tree);
        let events = probe.lock().unwrap().clone();
        assert!(
            events.iter().all(|e| e != "dcd:1"),
            "did_change_dependencies must NOT fire when update_should_notify=false. recorded: {:?}",
            events
        );
    }

    #[test]
    fn dependent_with_default_hook_is_unaffected() {
        let mut tree = ElementTree::new();
        let mut owner = BuildOwner::new();

        // Use the existing stateless `DummyChild` as the dependent — its
        // ElementBase uses the default empty `notify_dependency_change`
        // (no `ViewState` to forward to). The update path must run
        // cleanly: no panic, dependent is scheduled, the typed-hook
        // dispatch is a no-op for this dependent type.
        let dependent = StatelessProbeDependent;
        let (provider_id, dep_id) =
            mount_provider_and_record_dependency(&mut tree, &mut owner, 0x00FF_0000, &dependent);

        // Update so update_should_notify returns true.
        let provider_v2 = super::ThemeProvider {
            theme: MyTheme { color: 0x0000_FF00 },
            child: DummyChild,
        };
        tree.update(provider_id, &provider_v2, &mut owner.element_owner_mut());

        // The dependent is still scheduled for rebuild (same as the
        // status-quo behavior), and the pending-flag is set on it. The
        // distinction is purely behavioral: when the typed-hook
        // dispatch fires inside `build_scope`, it calls the default
        // empty `ElementBase::notify_dependency_change` — a clean no-op.
        assert_eq!(
            owner.dirty_count(),
            1,
            "dependent should be scheduled for rebuild"
        );
        assert!(
            owner
                .element_owner_mut()
                .has_pending_dependency_change(dep_id),
            "pending-flag is set on a stateless dependent (the typed-hook dispatch is a no-op \
             for this dependent's behavior, but the flag accounting is uniform)"
        );

        // Build phase — must run without panic. Stateless dependents use
        // the default no-op `notify_dependency_change`, so the call is
        // a clean dispatch with no observable effect beyond clearing
        // the pending flag.
        owner.build_scope(&mut tree);

        assert!(
            !owner
                .element_owner_mut()
                .has_pending_dependency_change(dep_id),
            "pending-flag must be cleared after build_scope drains it (even for stateless \
             dependents whose hook is a no-op)"
        );
    }
}
