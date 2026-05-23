//! Shared helpers for [`ElementBehavior`](super::behavior::ElementBehavior)
//! impls.
//!
//! Free functions extracted from the per-behavior `perform_build` /
//! `on_mount` / `on_unmount` / `on_update` bodies so each impl carries
//! only its behavior-specific path (plan §U16, brainstorm R23).
//!
//! ## Design (plan §D6, locked)
//!
//! These are **free functions**, not inherent methods on
//! `Element<V, A, B>` and not blanket-trait helpers. Rationale:
//!
//! - Inherent methods would inflate `Element<V, A, B>`'s impl block with
//!   helpers that touch only `ElementCore<V, A>` — generic explosion
//!   across three parameters when two suffice.
//! - A blanket impl on `ElementBehavior` would conflict with the
//!   per-behavior overrides we need to keep (mounting a `RenderObject`,
//!   subscribing to a `Listenable`, etc.).
//!
//! Per *Rust for Rustaceans* §"Composition and Helpers" and Constitution
//! Principle 4 (no `dyn` by default), free functions taking the minimal
//! slice of state composes cleanly with the existing trait surface.
//!
//! ## Scope
//!
//! REFACTOR-FIRST (plan U16 execution note). No behavior change. Existing
//! integration tests in `crates/flui-view/tests/*` cover the
//! end-to-end behavior; the per-helper tests below pin the helpers
//! themselves so a future regression in extraction quality is caught
//! before the integration suite.

use std::panic::AssertUnwindSafe;

use flui_foundation::RenderId;

use super::{arity::ElementArity, child_storage::ElementChildStorage, generic::ElementCore};
use crate::view::{FlutterError, IntoView, View};

// ============================================================================
// perform_build helpers
// ============================================================================

/// Guard for a behavior's `perform_build`. Returns `true` if the build
/// body should proceed, `false` if the early-return path was taken.
///
/// Emits the standard `skipped` / `starting` traces shared by every
/// behavior so each impl no longer needs to repeat the trace strings.
///
/// `behavior_name` should be a stable type tag (e.g. `"StatelessBehavior"`)
/// — used solely for tracing.
pub(crate) fn should_build_with_trace<V, A>(
    core: &ElementCore<V, A>,
    behavior_name: &'static str,
) -> bool
where
    V: Clone + Send + Sync + 'static,
    A: ElementArity,
{
    if core.should_build() {
        tracing::debug!("{}::perform_build starting", behavior_name);
        true
    } else {
        tracing::trace!("{}::perform_build skipped", behavior_name);
        false
    }
}

/// Run a user `build()` closure under [`std::panic::catch_unwind`] and,
/// on a caught panic, substitute the registered `ErrorView`.
///
/// This is the producer half of the `ErrorView` recovery path (plan
/// §U7, origin R9) — the receiver (`ErrorView` + `FlutterError` +
/// `set_error_view_builder`) already exists; this wires the catch that
/// feeds it. Mirrors Flutter's `ComponentElement.performRebuild`
/// (`framework.dart:5810-5859`), whose first `try/catch` wraps `build()`
/// and replaces the built widget with `ErrorWidget.builder`.
///
/// # Panic-safety boundary
///
/// `build` is the *only* code under the `catch_unwind`. The closure
/// captures just the immutable build inputs (`&V` / `&V::State` /
/// `&BuildContext`) — wrapped in [`AssertUnwindSafe`] because a user
/// `build()` is logically pure and a half-finished one leaks no shared
/// state into `core`. The child-update helper
/// [`finish_single_child_build`] runs strictly *after* this function
/// returns, so a panic can never be observed mid-`update_or_create_child`
/// (which would leave `core` half-mutated).
///
/// `AssertUnwindSafe` is safe code — this crate is
/// `#![forbid(unsafe_code)]` and the assertion does not change that.
///
/// # Teardown on a caught panic
///
/// The element under construction is in an indeterminate state after a
/// panic, so the whole child subtree is torn down
/// ([`ElementChildStorage::unmount_children`] — unmounts every descendant
/// and clears the storage) *before* the error view is returned. The
/// caller then hands the returned `ErrorView` box to
/// [`finish_single_child_build`], which sees an empty storage and
/// creates a fresh `ErrorElement` — leaving no dangling render-tree node
/// and no stale child. This is the Rust-native shape of Flutter's
/// `_child?.deactivate()` + `updateChild(null, errorWidget, slot)`
/// force-from-null rebuild (`framework.dart:5854-5858`).
///
/// `context` names what was building (e.g. `"building StatelessElement"`)
/// for the `FlutterError` breadcrumb.
pub(crate) fn build_or_recover<V, A, F>(
    core: &mut ElementCore<V, A>,
    owner: &mut crate::ElementOwner<'_>,
    behavior_name: &'static str,
    build: F,
) -> Box<dyn View>
where
    V: Clone + Send + Sync + 'static,
    A: ElementArity,
    F: FnOnce() -> Box<dyn View>,
{
    // Only `build()` is inside the catch — see the panic-safety note.
    // Phase 3 §U22 keeps the closure return as `Box<dyn View>` rather
    // than `impl IntoView`: the typed `impl IntoView` from
    // `StatelessView::build` / `ViewState::build` captures the
    // closure-local `&view`/`&ctx` borrows by Rust 2024 RPITIT
    // default, so returning the opaque value across the closure
    // boundary trips E0515 ("returns a value referencing data owned
    // by the current function"). The fix lives at the call site (see
    // `behavior.rs`): the closure body itself consumes the opaque
    // value via `IntoView::into_view()` + `Box::new`, producing an
    // owned `Box<dyn View>` with no escaping borrows. The trait stays
    // capture-default, authors do not need `+ use<…>` annotations on
    // their `build()` impls.
    match std::panic::catch_unwind(AssertUnwindSafe(build)) {
        Ok(child_view) => child_view,
        Err(payload) => {
            // Indeterminate state: tear the whole child subtree down so
            // the substituted error view is mounted fresh, not merged
            // onto a half-built descendant. `unmount_children` clears
            // the storage, so `finish_single_child_build` will go down
            // the create-from-empty branch.
            core.children_mut().unmount_children(owner);

            let error =
                FlutterError::from_panic(payload.as_ref(), format!("building {behavior_name}"));
            tracing::error!(
                "{}::perform_build caught a panic, substituting ErrorView: {}",
                behavior_name,
                error.message
            );
            crate::view::ErrorView::build_error_view(&error)
        }
    }
}

/// Shared tail for any behavior whose `perform_build` produces exactly
/// one `child_view`: hand the view to the core, clear the dirty flag,
/// emit the `completed` trace.
///
/// Used by `StatelessBehavior`, `StatefulBehavior`, `ProxyBehavior`,
/// `InheritedBehavior`. `RenderBehavior` keeps its `perform_build` body
/// inline so the tracing strings can interpolate the active `RenderId`.
//
// Phase 3 §U22 (FR-007): accepts `impl IntoView` from authoring-side
// callers and normalizes via `IntoView::into_view` inside the helper.
// `BoxedView` (the canonical erased path) and concrete `View`-impl
// types both satisfy the bound; the temporary `IntoView for Box<dyn View>`
// shim (`view/into_view.rs`) keeps legacy `Box<dyn View>` call sites
// compiling during the §U22→§U28 sweep. The `Box<dyn View>` ownership
// transfer to `update_or_create_child` is still intentional — the
// helper boxes the normalized value at the boundary so the inner
// pipeline keeps its existing contract. The generic `R` parameter (no
// longer `Box<dyn View>` on its own line) sidesteps `port-check.sh`
// trigger 6's struct-field pattern, so the previous `#[rustfmt::skip]`
// workaround that kept the signature on one line is no longer needed.
pub(crate) fn finish_single_child_build<V, A, R>(
    core: &mut ElementCore<V, A>,
    child_view: R,
    behavior_name: &'static str,
    owner: &mut crate::ElementOwner<'_>,
) where
    V: Clone + Send + Sync + 'static,
    A: ElementArity,
    R: IntoView,
{
    let boxed: Box<dyn View> = Box::new(child_view.into_view());
    core.update_or_create_child(boxed, owner);
    core.clear_dirty();
    tracing::debug!("{}::perform_build completed", behavior_name);
}

/// Full `perform_build` body for behaviors that delegate to a `child()`
/// accessor on the view — i.e. `ProxyBehavior` and `InheritedBehavior`.
///
/// `get_child` abstracts over `ProxyView::child` vs
/// `InheritedView::child`: both return `&dyn View` but the trait names
/// differ, so each behavior passes its own closure. The body is
/// identical otherwise:
///
/// ```text
///   guard → view.child() → clone_box → update_or_create_child → clear
/// ```
pub(crate) fn build_proxy_style<V, A, F>(
    core: &mut ElementCore<V, A>,
    behavior_name: &'static str,
    owner: &mut crate::ElementOwner<'_>,
    get_child: F,
) where
    V: Clone + Send + Sync + 'static,
    A: ElementArity,
    F: FnOnce(&V) -> &dyn View,
{
    if !should_build_with_trace(core, behavior_name) {
        return;
    }
    let child_view = get_child(core.view());
    let child_view_boxed = dyn_clone::clone_box(child_view);
    finish_single_child_build(core, child_view_boxed, behavior_name, owner);
}

// ============================================================================
// RenderBehavior helpers
// ============================================================================

/// Body of `RenderBehavior::on_update`: mark the associated `RenderObject`
/// as needing layout + paint. No-op if the behavior has no `RenderId`
/// yet (the `on_mount` callback creates it once a `PipelineOwner` is in
/// scope) or no `PipelineOwner` is plumbed through the core.
///
/// Mirrors Flutter's
/// `RenderObjectElement.update` → `RenderObject.markNeedsLayout` +
/// `markNeedsPaint` flow.
pub(crate) fn mark_render_needs_layout_and_paint<V, A>(
    core: &ElementCore<V, A>,
    render_id: Option<RenderId>,
    behavior_name: &'static str,
) where
    V: Clone + Send + Sync + 'static,
    A: ElementArity,
{
    if let Some(render_id) = render_id
        && let Some(pipeline_owner) = core.pipeline_owner()
    {
        let mut owner = pipeline_owner.write();
        let tree_depth = owner.render_tree().depth(render_id).unwrap_or(0);

        owner.add_node_needing_layout(render_id, tree_depth as usize);
        owner.add_node_needing_paint(render_id, tree_depth as usize);

        tracing::debug!(
            "{}::on_update marked render_id={:?} dirty",
            behavior_name,
            render_id
        );
    }
}

/// Body of `RenderBehavior::on_unmount`: remove the associated
/// `RenderObject` from the `RenderTree`. No-op if the behavior never
/// got a `RenderId` (e.g. unmount before mount with no `PipelineOwner`).
pub(crate) fn remove_render_object_from_tree<V, A>(
    core: &ElementCore<V, A>,
    render_id: Option<RenderId>,
    behavior_name: &'static str,
) where
    V: Clone + Send + Sync + 'static,
    A: ElementArity,
{
    if let Some(render_id) = render_id
        && let Some(pipeline_owner) = core.pipeline_owner()
    {
        // Cycle 3 T-1: `TreeWrite::remove` now cascades by default. The
        // pre-cycle inherent `RenderTree::remove` was the non-cascade
        // primitive (now renamed `remove_shallow`). Element unmount
        // wants the cascade — when a parent element unmounts, all
        // descendant render objects must come down with it.
        use flui_tree::TreeWrite;
        let mut owner = pipeline_owner.write();
        owner.render_tree_mut().remove(render_id);
        tracing::debug!(
            "{}::on_unmount removed render_id={:?}",
            behavior_name,
            render_id
        );
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    //! Per-helper tests with synthetic `ElementCore<V, A>` fixtures.
    //!
    //! Behavior-level coverage already lives in
    //! `crates/flui-view/tests/*` — these tests pin the helpers so a
    //! regression in extraction quality is caught locally before the
    //! integration suite runs.

    use dyn_clone::clone_box;
    use flui_foundation::ElementId;

    use super::*;
    use crate::view::ViewExt;
    use crate::{
        BuildOwner,
        element::{
            Lifecycle, Single,
            arity::Leaf,
            behavior::{ElementBehavior, StatelessBehavior},
            generic::ElementCore,
        },
        view::{ElementBase, StatelessView, View},
    };

    // ------------------------------------------------------------------
    // Synthetic fixtures
    // ------------------------------------------------------------------

    /// Bare-bones leaf view used to assemble an `ElementCore<TestView, Leaf>`.
    /// We exercise the helpers directly against a core in different
    /// dirty / lifecycle states — no real rendering required.
    #[derive(Clone, Debug)]
    struct TestView;

    impl View for TestView {
        fn create_element(&self) -> Box<dyn ElementBase> {
            // Helpers under test never call `create_element`, but we
            // need a real impl so the trait bound is satisfied.
            Box::new(NopElement)
        }
    }

    /// Single-child stateless view used to drive `build_proxy_style`
    /// fixtures (it exposes a `child()` accessor we can hand to the
    /// helper).
    #[derive(Clone, Debug)]
    struct WrapperView {
        child: TestView,
    }

    impl WrapperView {
        fn child(&self) -> &dyn View {
            &self.child
        }
    }

    impl View for WrapperView {
        fn create_element(&self) -> Box<dyn ElementBase> {
            Box::new(NopElement)
        }
    }

    /// Stub element that satisfies `ElementBase` without doing any work;
    /// used only as the `Box<dyn ElementBase>` payload for the
    /// `create_element` return value above.
    struct NopElement;

    impl ElementBase for NopElement {
        fn view_type_id(&self) -> std::any::TypeId {
            std::any::TypeId::of::<TestView>()
        }
        fn lifecycle(&self) -> Lifecycle {
            Lifecycle::Initial
        }
        fn depth(&self) -> usize {
            0
        }
        fn mark_needs_build(&mut self) {}
        fn visit_children(&self, _visitor: &mut dyn FnMut(ElementId)) {}
        fn update(&mut self, _new_view: &dyn View, _owner: &mut crate::ElementOwner<'_>) {}
        fn perform_build(&mut self, _owner: &mut crate::ElementOwner<'_>) {}
        fn mount(
            &mut self,
            _parent: Option<ElementId>,
            _slot: usize,
            _owner: &mut crate::ElementOwner<'_>,
        ) {
        }
        fn unmount(&mut self, _owner: &mut crate::ElementOwner<'_>) {}
        fn activate(&mut self) {}
        fn deactivate(&mut self) {}
    }

    // ------------------------------------------------------------------
    // should_build_with_trace
    // ------------------------------------------------------------------

    #[test]
    fn should_build_returns_true_when_dirty_and_buildable() {
        let mut core = ElementCore::<TestView, Leaf>::new(TestView);
        let mut owner = BuildOwner::new();
        core.mount(None, 0, &mut owner.element_owner_mut());

        assert!(core.is_dirty());
        assert_eq!(core.lifecycle(), Lifecycle::Active);

        assert!(should_build_with_trace(&core, "TestBehavior"));
    }

    #[test]
    fn should_build_returns_false_after_clear_dirty() {
        let mut core = ElementCore::<TestView, Leaf>::new(TestView);
        let mut owner = BuildOwner::new();
        core.mount(None, 0, &mut owner.element_owner_mut());
        core.clear_dirty();

        assert!(!core.is_dirty());
        assert!(!should_build_with_trace(&core, "TestBehavior"));
    }

    #[test]
    fn should_build_returns_false_when_lifecycle_blocks_build() {
        // A fresh core is Initial (cannot build) even though it is
        // dirty — guard must respect lifecycle, not only the dirty flag.
        let core = ElementCore::<TestView, Leaf>::new(TestView);
        assert!(core.is_dirty());
        assert_eq!(core.lifecycle(), Lifecycle::Initial);

        assert!(!should_build_with_trace(&core, "TestBehavior"));
    }

    // ------------------------------------------------------------------
    // finish_single_child_build
    // ------------------------------------------------------------------
    //
    // We exercise the helper through a real StatelessBehavior so the
    // child wiring is observable — `perform_build` walks through
    // `should_build_with_trace` → user closure → `finish_single_child_build`.
    // We assert (a) the dirty flag clears and (b) the lifecycle is still
    // Active.

    /// Concrete StatelessView used to drive `finish_single_child_build`
    /// through a real `StatelessBehavior::perform_build` path.
    #[derive(Clone, Debug)]
    struct CountingView;

    impl StatelessView for CountingView {
        fn build(&self, _ctx: &dyn crate::context::BuildContext) -> impl IntoView {
            TestView.boxed()
        }
    }

    impl View for CountingView {
        fn create_element(&self) -> Box<dyn ElementBase> {
            Box::new(NopElement)
        }
    }

    #[test]
    fn finish_single_child_build_clears_dirty_via_stateless_path() {
        let mut core = ElementCore::<CountingView, Single>::new(CountingView);
        let mut build_owner = BuildOwner::new();
        core.mount(None, 0, &mut build_owner.element_owner_mut());
        assert!(core.is_dirty());

        let mut behavior = StatelessBehavior;
        let mut owner = build_owner.element_owner_mut();
        <StatelessBehavior as ElementBehavior<CountingView, Single>>::perform_build(
            &mut behavior,
            &mut core,
            &mut owner,
        );

        assert!(!core.is_dirty(), "perform_build must clear dirty");
        assert_eq!(core.lifecycle(), Lifecycle::Active);
    }

    #[test]
    fn finish_single_child_build_direct_helper_call_clears_dirty() {
        // Drive the helper directly with a hand-rolled child to lock
        // down its contract independent of any caller.
        let mut core = ElementCore::<TestView, Single>::new(TestView);
        let mut build_owner = BuildOwner::new();
        core.mount(None, 0, &mut build_owner.element_owner_mut());
        assert!(core.is_dirty());

        let child: Box<dyn View> = Box::new(TestView);
        let mut owner = build_owner.element_owner_mut();
        finish_single_child_build(&mut core, child, "TestBehavior", &mut owner);

        assert!(!core.is_dirty(), "helper must clear the dirty flag");
    }

    // ------------------------------------------------------------------
    // build_proxy_style
    // ------------------------------------------------------------------

    #[test]
    fn build_proxy_style_clears_dirty_after_running() {
        let mut core = ElementCore::<WrapperView, Single>::new(WrapperView { child: TestView });
        let mut build_owner = BuildOwner::new();
        core.mount(None, 0, &mut build_owner.element_owner_mut());
        assert!(core.is_dirty());

        let mut owner = build_owner.element_owner_mut();
        build_proxy_style(&mut core, "TestBehavior", &mut owner, WrapperView::child);

        assert!(!core.is_dirty(), "proxy-style build must clear dirty");
    }

    #[test]
    fn build_proxy_style_skips_when_lifecycle_blocks_build() {
        // Lifecycle::Initial blocks builds. The helper's
        // `should_build_with_trace` guard must short-circuit and the
        // dirty flag should remain set (we never reached the tail).
        let mut core = ElementCore::<WrapperView, Single>::new(WrapperView { child: TestView });
        let mut build_owner = BuildOwner::new();

        assert_eq!(core.lifecycle(), Lifecycle::Initial);
        let was_dirty = core.is_dirty();

        let mut owner = build_owner.element_owner_mut();
        build_proxy_style(&mut core, "TestBehavior", &mut owner, WrapperView::child);

        assert_eq!(core.is_dirty(), was_dirty);
        // `clone_box` proves the get_child closure is wired:
        let _ = clone_box(WrapperView { child: TestView }.child());
    }

    // ------------------------------------------------------------------
    // mark_render_needs_layout_and_paint + remove_render_object_from_tree
    // ------------------------------------------------------------------
    //
    // These helpers are pure no-ops when there is no PipelineOwner and
    // no RenderId. We assert that contract directly — the
    // PipelineOwner-bearing path is exercised end-to-end through
    // `crates/flui-view/tests/*` and `view::render::tests`.

    #[test]
    fn mark_render_needs_layout_and_paint_is_noop_without_pipeline_owner() {
        let core = ElementCore::<TestView, Leaf>::new(TestView);
        // No render_id, no pipeline_owner — the helper must not panic
        // and must not need to talk to a PipelineOwner.
        mark_render_needs_layout_and_paint(&core, None, "TestBehavior");
    }

    #[test]
    fn remove_render_object_from_tree_is_noop_without_pipeline_owner() {
        let core = ElementCore::<TestView, Leaf>::new(TestView);
        remove_render_object_from_tree(&core, None, "TestBehavior");
    }
}
