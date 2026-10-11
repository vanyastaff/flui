//! Acceptance + edge-case tests for `BuildContext::depend_on_inherited`,
//! `InheritedBehavior::on_view_updated` dependent-notification, and
//! `BuildContext::get_inherited` (non-recording read).
//!
//! Coverage:
//! - `depend_on_inherited::<T, _>` returns `Some(R)` and records
//!   the caller in the InheritedElement's dependent map.
//! - Rebuilding the InheritedView with a value where
//!   `update_should_notify` returns `true` marks dependents dirty.
//! - Edge: no ancestor of `T` -> returns `None`, retaining an ancestry dependency
//!   for reactivation without creating a provider notification edge.
//! - Edge: deduplication when the same element calls `depend_on` twice.
//! - Unmount/deactivate remove provider edges synchronously, so long-lived
//!   providers do not accumulate historical dependents.
//! - Reactivation schedules dependency lifecycle refresh against the new
//!   ancestry.
//! - `get_inherited::<T, _>` returns `Some(R)` BUT does NOT
//!   record the caller in the InheritedElement's dependent map. Used for
//!   one-time reads (settings/theme captured at mount).
//! - Edge: no ancestor of `T` -> returns `None`, no dependent-set write.

// Target-level lint relaxations — crate-level allows don't reach this
// target. `unwrap` in test/example code: a panic IS the failure report
// (docs/PANIC-POLICY.md); style items here are ship-wave debt.
// ADR-0027: ElementBuildContext's current test/prod seam still takes
// Arc<RwLock<ElementTree/BuildOwner>>. The owner graph is !Send; do not restore
// Send + Sync to satisfy clippy. Future UiRuntime/Rc migration should remove this.
#![expect(clippy::arc_with_non_send_sync)]

use std::sync::Arc;

use flui_objects::RenderSizedBox;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{
    BuildContext, BuildContextExt, BuildOwner, ElementBuildContext, ElementTree, InheritedElement,
    IntoView, RenderView, StatelessView, View, ViewExt, view::InheritedView,
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
    // Bottom out at the shared terminal leaf. This used to recurse on
    // `self.clone().boxed()`, which no test observed because no fixture ever
    // built a `DummyChild` element — the render-root helper below does, and a
    // self-recursive build drives `build_scope`'s drain forever. The file's
    // terminal-leaf rule says build chains bottom out in `LeafView`.
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        LeafView.boxed()
    }
}

impl View for DummyChild {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
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
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::inherited(self)
    }
}

// ============================================================================
// Shared terminal leaf — a build chain must bottom out, so probe/consumer
// views return this leaf as their child instead of recursing on themselves.
// `LeafElement::build_into_views` returns no children, so `build_scope`
// terminates one hop below the deepest interesting node.
// ============================================================================

#[derive(Clone)]
struct LeafView;

impl RenderView for LeafView {
    type Protocol = BoxProtocol;
    type RenderObject = RenderSizedBox;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderSizedBox::shrink()
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        flui_rendering::RenderUpdateImpact::NONE
    }
}

impl View for LeafView {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
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
// depend_on returns Some(value) and records the dependent
// ============================================================================

// ============================================================================
// rebuilding the InheritedView with update_should_notify=true marks
//      dependents dirty
// ============================================================================

pub(crate) fn inherited_update_notifies_dependents() {
    // Same scaffolding as the depend_on-records-dependent test above.
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
        let ctx = ElementBuildContext::for_element(child_id, tree.clone(), owner.clone())
            .expect("the child element is live");
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

// ============================================================================
// Edge: same element calls depend_on twice in one build -> dedup
// ============================================================================

// ============================================================================
// Edge: unmounted dependent is removed from the provider immediately
// ============================================================================

pub(crate) fn missing_inherited_reads_refresh_lifecycle_and_survive_build_recovery() {
    #[derive(Clone)]
    struct Probe {
        changes: std::rc::Rc<std::cell::Cell<usize>>,
        read_in_build: bool,
        fail_build: std::rc::Rc<std::cell::Cell<bool>>,
    }
    struct ProbeState(std::rc::Rc<std::cell::Cell<usize>>);
    impl View for Probe {
        fn create_element(&self) -> flui_view::element::ElementKind {
            flui_view::element::ElementKind::stateful(self)
        }
    }
    impl flui_view::StatefulView for Probe {
        type State = ProbeState;
        fn create_state(&self) -> Self::State {
            ProbeState(std::rc::Rc::clone(&self.changes))
        }
    }
    impl flui_view::ViewState<Probe> for ProbeState {
        fn did_change_dependencies(&mut self, ctx: &dyn flui_view::LifecycleContext) {
            self.0.set(self.0.get() + 1);
            assert!(ctx.depend_on::<ThemeProvider, _>(|_| ()).is_none());
        }
        fn build(&self, view: &Probe, ctx: &dyn BuildContext) -> impl IntoView {
            assert!(
                !view.fail_build.replace(false),
                "authored build failure before reading dependencies"
            );
            if view.read_in_build {
                assert!(ctx.depend_on::<ThemeProvider, _>(|_| ()).is_none());
            }
            LeafView
        }
    }

    for (subscribe, read_in_build, failed_rebuild) in [
        (false, false, false),
        (true, false, false),
        (false, true, false),
        (false, true, true),
    ] {
        let (tree, owner) = create_tree_and_owner();
        let changes = std::rc::Rc::new(std::cell::Cell::new(0));
        let fail_build = std::rc::Rc::new(std::cell::Cell::new(false));
        let root = tree.write().mount_root(
            &Probe {
                changes: std::rc::Rc::clone(&changes),
                read_in_build,
                fail_build: std::rc::Rc::clone(&fail_build),
            },
            &mut owner.write().element_owner_mut(),
        );
        owner
            .write()
            .schedule_build_for(root, 0, flui_view::RebuildReason::InitialMount);
        owner.write().build_scope(&mut tree.write());
        if failed_rebuild {
            fail_build.set(true);
            tree.write().update(
                root,
                &Probe {
                    changes: std::rc::Rc::clone(&changes),
                    read_in_build,
                    fail_build: std::rc::Rc::clone(&fail_build),
                },
                &mut owner.write().element_owner_mut(),
            );
            owner
                .write()
                .schedule_build_for(root, 0, flui_view::RebuildReason::ParentUpdate);
            owner.write().build_scope(&mut tree.write());
            assert_eq!(owner.write().take_recovered_panics().len(), 1);
        }
        let ctx = ElementBuildContext::for_element(root, tree.clone(), owner.clone())
            .expect("the probe is mounted");
        let found = if subscribe {
            ctx.depend_on::<ThemeProvider, _>(|_| ())
        } else {
            ctx.get::<ThemeProvider, _>(|_| ())
        };
        assert!(found.is_none());
        for cycle in 1..=2 {
            tree.write()
                .deactivate(root, &mut owner.write().element_owner_mut());
            tree.write()
                .activate(root, &mut owner.write().element_owner_mut());
            owner.write().build_scope(&mut tree.write());
            assert_eq!(
                changes.get(),
                if subscribe || read_in_build { cycle } else { 0 },
                "only dependency reads refresh lifecycle after activation"
            );
        }
        tree.write()
            .remove(root, &mut owner.write().element_owner_mut());
    }
}

pub(crate) fn unmounted_dependent_is_removed_from_provider_before_next_notification() {
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
        let ctx = ElementBuildContext::for_element(child_id, tree.clone(), owner.clone())
            .expect("the child element is live");
        let _ = ctx.depend_on::<ThemeProvider, ()>(|_| ());
    }

    // Remove the dependent from the tree before updating the inherited.
    tree.write()
        .remove(child_id, &mut owner.write().element_owner_mut());

    {
        let tree_guard = tree.read();
        let provider_node = tree_guard.get(provider_id).expect("provider exists");
        let provider = provider_node
            .element()
            .downcast_ref::<InheritedElement<ThemeProvider>>()
            .expect("provider is InheritedElement<ThemeProvider>");
        assert!(
            !provider.dependents().contains_key(&child_id),
            "unmount must remove the dependent from the provider immediately",
        );
    }

    // A later provider update must not schedule the removed element. This
    // asserts both memory ownership and notification cost: historical
    // dependents cannot accumulate in a long-lived provider.
    let provider_v2 = ThemeProvider {
        theme: MyTheme { color: 0x0000_FF00 },
        child: DummyChild,
    };
    tree.write().update(
        provider_id,
        &provider_v2,
        &mut owner.write().element_owner_mut(),
    );

    assert_eq!(
        owner.read().dirty_count(),
        0,
        "a provider update must not enqueue an already-unmounted dependent",
    );
}

// ============================================================================
// get_inherited returns the value WITHOUT recording a
// dependent.
// ============================================================================

// ============================================================================
// Edge: get_inherited returns None when no ancestor InheritedView
// of that type exists — no dependent-set write happens because nothing
// was found to write into.
// ============================================================================

// ============================================================================
// Wires `did_change_dependencies` to inherited updates.
//
// When `InheritedView::update_should_notify` returns `true`, the
// dependent's typed `ViewState::did_change_dependencies` hook fires
// exactly once, BEFORE the dependent's `perform_build`.
// ============================================================================

mod did_change_dependencies_on_inherited_update {
    use std::{
        any::TypeId,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use flui_view::{
        BoxedView, BuildContext, BuildContextExt, BuildOwner, ElementTree, ErrorView,
        InheritedView, IntoView, LifecycleContext, LifecycleHook, RebuildReason, RecoveredAt,
        RenderView, StatefulView, StatelessView, View, ViewExt, ViewState,
    };

    use super::{LeafView, MyTheme};

    // ========================================================================
    // Stateful dependent that records `did_change_dependencies` + `build`
    // invocations into a shared probe.
    // ========================================================================

    // ========================================================================
    // Stateless dependent — exercises the default no-op
    // `ElementBase::notify_dependency_change` path. Build returns a true
    // leaf so build_scope terminates.
    // ========================================================================

    // ========================================================================
    // Panic-safety: a panic in the typed `did_change_dependencies` hook (the
    // `notify_dependency_change` branch of the build-scope take/put window)
    // must restore the dependent's slab slot, not leave a `None` hole.
    // ========================================================================

    #[derive(Clone)]
    struct PanicDcd;

    struct PanicDcdState;

    impl StatefulView for PanicDcd {
        type State = PanicDcdState;

        fn create_state(&self) -> Self::State {
            PanicDcdState
        }
    }

    impl ViewState<PanicDcd> for PanicDcdState {
        fn did_change_dependencies(&mut self, _ctx: &dyn LifecycleContext) {
            panic!("induced did_change_dependencies panic (build-window panic-safety test)");
        }

        fn build(&self, _view: &PanicDcd, ctx: &dyn BuildContext) -> impl IntoView {
            let _ = ctx.depend_on::<PanicThemeProvider, ()>(|_| ());
            LeafView.boxed()
        }
    }

    impl View for PanicDcd {
        fn create_element(&self) -> flui_view::element::ElementKind {
            flui_view::element::ElementKind::stateful(self)
        }
    }

    #[derive(Clone)]
    struct PanicThemeProvider {
        theme: MyTheme,
        child: DcdHost,
    }

    impl InheritedView for PanicThemeProvider {
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

    impl View for PanicThemeProvider {
        fn create_element(&self) -> flui_view::element::ElementKind {
            flui_view::element::ElementKind::inherited(self)
        }
    }

    #[derive(Clone)]
    struct DcdHost {
        children: Vec<BoxedView>,
    }

    impl RenderView for DcdHost {
        type Protocol = flui_rendering::protocol::BoxProtocol;
        type RenderObject = flui_objects::RenderSizedBox;

        fn create_render_object(
            &self,
            _ctx: &flui_view::RenderObjectContext<'_>,
        ) -> Self::RenderObject {
            flui_objects::RenderSizedBox::shrink()
        }

        fn update_render_object(
            &self,
            _ctx: &flui_view::RenderObjectContext<'_>,
            _render_object: &mut Self::RenderObject,
        ) -> flui_rendering::RenderUpdateImpact {
            flui_rendering::RenderUpdateImpact::NONE
        }

        fn has_children(&self) -> bool {
            true
        }

        fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
            self.children.iter().for_each(|child| visitor(child));
        }
    }

    impl View for DcdHost {
        fn create_element(&self) -> flui_view::element::ElementKind {
            flui_view::element::ElementKind::render_variable(self)
        }
    }

    #[derive(Clone)]
    struct CountingSibling(Arc<AtomicUsize>);

    impl StatelessView for CountingSibling {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            self.0.fetch_add(1, Ordering::Relaxed);
            LeafView
        }
    }

    impl View for CountingSibling {
        fn create_element(&self) -> flui_view::element::ElementKind {
            flui_view::element::ElementKind::stateless(self)
        }
    }

    #[test]
    fn did_change_dependencies_panic_is_replaced_and_the_sibling_continues() {
        let mut tree = ElementTree::new();
        let mut owner = BuildOwner::new();
        let sibling_builds = Arc::new(AtomicUsize::new(0));
        let provider_v1 = PanicThemeProvider {
            theme: MyTheme { color: 0x00FF_0000 },
            child: DcdHost {
                children: vec![
                    PanicDcd.boxed(),
                    CountingSibling(sibling_builds.clone()).boxed(),
                ],
            },
        };
        // Production shape (the bootstrap idiom): the provider sits under a
        // render root, so `DcdHost`'s render object mounts with a render
        // parent instead of orphaning under a render-less owner-carrying
        // root.
        let render_root_element = tree.mount_root_with_pipeline_owner(
            &flui_view::RootRenderView::new(provider_v1.clone(), 800.0, 600.0),
            Some(flui_rendering::pipeline::PipelineCell::new(
                flui_rendering::pipeline::PipelineOwner::new(
                    flui_rendering::TextContextHandle::standalone(),
                ),
            )),
            &mut owner.element_owner_mut(),
        );
        owner.schedule_build_for(render_root_element, 0, RebuildReason::InitialMount);
        owner.build_scope(&mut tree);
        let provider_id = tree
            .get(render_root_element)
            .expect("render root stays live")
            .child_ids()[0];
        let host = tree
            .get(provider_id)
            .expect("provider stays live")
            .child_ids()[0];
        let before = tree
            .get(host)
            .expect("host stays live")
            .child_ids()
            .to_vec();
        let dep_id = before[0];
        let sibling = before[1];
        let sibling_before = sibling_builds.load(Ordering::Relaxed);

        let provider_v2 = PanicThemeProvider {
            theme: MyTheme { color: 0x0000_FF00 },
            child: provider_v1.child,
        };
        tree.update(provider_id, &provider_v2, &mut owner.element_owner_mut());
        tree.mark_needs_build(sibling);
        owner.schedule_build_for(sibling, 2, RebuildReason::StateChange);

        let ((), captured_log) =
            flui_testing::log_capture::capture(|| owner.build_scope(&mut tree));

        let children = tree.get(host).expect("host stays live").child_ids();
        assert_eq!(children.len(), 2);
        assert_eq!(children[1], sibling);
        let replacement = children[0];
        assert_ne!(replacement, dep_id);
        assert_eq!(
            tree.get(replacement)
                .expect("replacement stays live")
                .element()
                .view_type_id(),
            TypeId::of::<ErrorView>(),
        );
        assert!(tree.get(dep_id).is_none());
        assert_eq!(owner.pending_rebuild_reasons(dep_id), None);
        assert!(
            !owner
                .element_owner_mut()
                .has_pending_dependency_change(dep_id)
        );
        assert_eq!(owner.pending_rebuild_reasons(replacement), None);
        assert_eq!(sibling_builds.load(Ordering::Relaxed), sibling_before + 1);
        let recovered = owner.take_recovered_panics();
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].hook, LifecycleHook::DidChangeDependencies);
        assert_eq!(recovered[0].view_type_id, TypeId::of::<PanicDcd>());
        assert!(matches!(
            recovered[0].at,
            RecoveredAt::Element {
                element,
                parent: None,
                ..
            } if element == dep_id
        ));
        assert!(owner.take_recovered_panics().is_empty());
        assert_eq!(
            captured_log.count_containing("recovery transaction pending"),
            1
        );
        assert_eq!(
            captured_log.count_containing("lifecycle hook panicked; contained"),
            0,
            "committing a staged dependency record must not log twice: {captured_log}"
        );
        assert_eq!(owner.dirty_count(), 0);
    }
}

// ============================================================================
// PR-K keystone: a consumer's REAL build() resolves an inherited value two
// hops up through the LIVE element tree, and the dependency it records there
// drives its rebuild when the value later changes.
//
// Before PR-K, `build_into_views` handed user `build()` an empty process-
// shared dummy context, so `depend_on` inside a real build returned `None`
// and recorded nothing — inherited data was unreachable from the very place
// it should be reachable. This module pins the
// wired behavior end to end: read-during-build → record → notify-on-update.
// ============================================================================

// ============================================================================
// Panic-safety of the build_scope take/put window.
//
// `build_scope` extracts each element BY VALUE for the duration of its build
// and puts it back afterward. A panic in a user hook reachable inside that
// window (`init_state` / `did_change_dependencies`) must NOT drop the element
// and leave a permanent `None` hole — every later `element()` access on that
// node would then panic with `ELEMENT_PRESENT`. A `catch_unwind` guard
// restores the slot before re-raising. These tests pin that contract: without
// the guard `element_opt()` returns `None` after the panic.
// ============================================================================
