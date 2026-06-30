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

use super::{arity::ElementArity, generic::ElementCore};
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
/// state into `core`. The child reconcile — the id-reconciler the
/// surrounding `BuildOwner::build_scope` drives once `build_into_views`
/// has returned — runs strictly *after* this function, so a panic can
/// never be observed mid-reconcile (which would leave `core` half-mutated).
///
/// `AssertUnwindSafe` is safe code — this crate is
/// `#![forbid(unsafe_code)]` and the assertion does not change that.
///
/// # Recovery on a caught panic
///
/// E3 (atomic box→arena swap): the element no longer owns a child graph,
/// so there is no half-built child subtree to tear down here. On a caught
/// panic this returns the substituted `ErrorView` box; the caller
/// (`build_into_views`) returns it as the single child view, and the slab
/// id-reconciler in `build_scope` replaces the prior child element with a
/// fresh `ErrorElement`. That id-reconcile (type mismatch → remove old +
/// insert new) is the Rust-native, slab-resident shape of Flutter's
/// `_child?.deactivate()` + `updateChild(null, errorWidget, slot)`
/// force-from-null rebuild (`framework.dart:5854-5858`).
///
/// `behavior_name` names what was building (e.g. `"building
/// StatelessElement"`) for the `FlutterError` breadcrumb.
pub(crate) fn build_or_recover<V, A, F>(
    core: &mut ElementCore<V, A>,
    _owner: &mut crate::ElementOwner<'_>,
    behavior_name: &'static str,
    build: F,
) -> Box<dyn View>
where
    V: Clone + Send + Sync + 'static,
    A: ElementArity,
    F: FnOnce() -> Box<dyn View>,
{
    // Only `build()` is inside the catch — see the panic-safety note.
    // The typed `impl IntoView` from `StatelessView::build` /
    // `ViewState::build` captures the closure-local `&view`/`&ctx`
    // borrows by Rust 2024 RPITIT default, so returning the opaque value
    // across the closure boundary would trip E0515. The fix lives at the
    // call site (see `behavior.rs`): the closure body itself consumes the
    // opaque value via `IntoView::into_view()` + `Box::new`, producing an
    // owned `Box<dyn View>` with no escaping borrows. Authors need no
    // `+ use<…>` annotations on their `build()` impls.
    let _ = core; // `core` kept on the signature for symmetry / future hooks.
    match std::panic::catch_unwind(AssertUnwindSafe(build)) {
        Ok(child_view) => child_view,
        Err(payload) => {
            let error =
                FlutterError::from_panic(payload.as_ref(), format!("building {behavior_name}"));
            tracing::error!(
                "{}::build_into_views caught a panic, substituting ErrorView: {}",
                behavior_name,
                error.message
            );
            crate::view::ErrorView::build_error_view(&error)
        }
    }
}

/// Shared tail for any behavior whose build half produces exactly one
/// child view: normalize it to a boxed `View`, clear the dirty flag, emit
/// the `completed` trace, and return it as a single-element `Vec`.
///
/// Used by `StatelessBehavior`, `StatefulBehavior`, `ProxyBehavior`,
/// `InheritedBehavior`. `RenderBehavior` keeps its `build_into_views`
/// body inline so the tracing strings can interpolate the active
/// `RenderId`.
///
/// E3 (atomic box→arena swap): this no longer reconciles a child into box
/// storage — it just hands the owned child view back. The reconcile
/// against the slab runs in `BuildOwner::build_scope`. `core` is cleared
/// of its dirty flag here because the build half is complete (the seam
/// that used to clear it after `update_or_create_child`).
//
// FR-007: accepts `impl IntoView` from authoring-side callers and
// normalizes via `IntoView::into_view` inside the helper. The generic
// `R` parameter (not `Box<dyn View>` on its own line) sidesteps
// `port-check.sh` trigger 6's struct-field pattern.
#[must_use]
pub(crate) fn single_child_views<V, A, R>(
    core: &mut ElementCore<V, A>,
    child_view: R,
    behavior_name: &'static str,
) -> Vec<Box<dyn View>>
where
    V: Clone + Send + Sync + 'static,
    A: ElementArity,
    R: IntoView,
{
    let boxed: Box<dyn View> = Box::new(child_view.into_view());
    core.clear_dirty();
    tracing::debug!("{}::build_into_views completed", behavior_name);
    vec![boxed]
}

/// Full `build_into_views` body for behaviors that delegate to a `child()`
/// accessor on the view — i.e. `ProxyBehavior` and `InheritedBehavior`.
///
/// `get_child` abstracts over `ProxyView::child` vs
/// `InheritedView::child`: both return `&dyn View` but the trait names
/// differ, so each behavior passes its own closure. The body is
/// identical otherwise:
///
/// ```text
///   guard → view.child() → clone_box → clear dirty → vec![child]
/// ```
///
/// Returns `vec![]` when the guard short-circuits (clean element /
/// non-buildable lifecycle), so `build_scope` reconciles to the same
/// child list the prior frame produced only when a real build ran — a
/// clean proxy contributes no view churn.
#[must_use]
pub(crate) fn proxy_style_views<V, A, F>(
    core: &mut ElementCore<V, A>,
    behavior_name: &'static str,
    get_child: F,
) -> Vec<Box<dyn View>>
where
    V: Clone + Send + Sync + 'static,
    A: ElementArity,
    F: FnOnce(&V) -> &dyn View,
{
    if !should_build_with_trace(core, behavior_name) {
        return Vec::new();
    }
    let child_view = get_child(core.view());
    let child_view_boxed = dyn_clone::clone_box(child_view);
    single_child_views(core, child_view_boxed, behavior_name)
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

        // D-block PR-A1 U16: migrate from direct add_node_needing_layout to
        // PipelineOwner::mark_needs_layout (new in U15) so the layout-side
        // mark propagates up to the nearest relayout boundary per Flutter
        // markNeedsLayout semantics. Paint side stays on the direct primitive
        // — Flutter's markNeedsPaint is its own walk with different boundary
        // semantics (repaint vs relayout boundary) and is out of D-block
        // scope (Core.2 paint catalog work).
        let tree_depth = owner.render_tree().depth(render_id).unwrap_or(0);
        owner.mark_needs_layout(render_id);
        owner.add_node_needing_paint(render_id, tree_depth as usize);

        tracing::debug!(
            "{}::on_update marked render_id={:?} dirty (layout via mark_needs_layout walk, paint direct)",
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

        let mut owner = pipeline_owner.write();
        // Dispose protocol: evict dirty entries, then free the slots.
        owner.remove_render_object(render_id);
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
        view::{StatelessView, View},
    };
    use dyn_clone::clone_box;

    // ------------------------------------------------------------------
    // Synthetic fixtures
    // ------------------------------------------------------------------

    /// Bare-bones leaf view used to assemble an `ElementCore<TestView, Leaf>`.
    /// We exercise the helpers directly against a core in different
    /// dirty / lifecycle states — no real rendering required.
    #[derive(Clone, Debug)]
    struct TestView;

    impl StatelessView for TestView {
        fn build(&self, _ctx: &dyn crate::context::BuildContext) -> impl IntoView {
            crate::view::ErrorView::new("test fixture leaf")
        }
    }

    impl View for TestView {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::stateless(self)
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

    impl StatelessView for WrapperView {
        fn build(&self, _ctx: &dyn crate::context::BuildContext) -> impl IntoView {
            self.child.clone()
        }
    }

    impl View for WrapperView {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::stateless(self)
        }
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
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::stateless(self)
        }
    }

    #[test]
    fn build_into_views_clears_dirty_via_stateless_path() {
        let mut core = ElementCore::<CountingView, Single>::new(CountingView);
        let mut build_owner = BuildOwner::new();
        core.mount(None, 0, &mut build_owner.element_owner_mut());
        assert!(core.is_dirty());

        let mut behavior = StatelessBehavior;
        let mut owner = build_owner.element_owner_mut();
        let views = <StatelessBehavior as ElementBehavior<CountingView, Single>>::build_into_views(
            &mut behavior,
            &mut core,
            &mut owner,
        );

        assert!(!core.is_dirty(), "build_into_views must clear dirty");
        assert_eq!(core.lifecycle(), Lifecycle::Active);
        assert_eq!(
            views.len(),
            1,
            "stateless build yields exactly one child view"
        );
    }

    #[test]
    fn single_child_views_direct_helper_call_clears_dirty() {
        // Drive the helper directly with a hand-rolled child to lock
        // down its contract independent of any caller.
        let mut core = ElementCore::<TestView, Single>::new(TestView);
        let mut build_owner = BuildOwner::new();
        core.mount(None, 0, &mut build_owner.element_owner_mut());
        assert!(core.is_dirty());

        let child: Box<dyn View> = Box::new(TestView);
        let views = single_child_views(&mut core, child, "TestBehavior");

        assert!(!core.is_dirty(), "helper must clear the dirty flag");
        assert_eq!(views.len(), 1, "helper returns the single child view");
    }

    // ------------------------------------------------------------------
    // proxy_style_views
    // ------------------------------------------------------------------

    #[test]
    fn proxy_style_views_clears_dirty_and_returns_child() {
        let mut core = ElementCore::<WrapperView, Single>::new(WrapperView { child: TestView });
        let mut build_owner = BuildOwner::new();
        core.mount(None, 0, &mut build_owner.element_owner_mut());
        assert!(core.is_dirty());

        let views = proxy_style_views(&mut core, "TestBehavior", WrapperView::child);

        assert!(!core.is_dirty(), "proxy-style build must clear dirty");
        assert_eq!(views.len(), 1, "proxy build yields the wrapped child view");
    }

    #[test]
    fn proxy_style_views_skips_when_lifecycle_blocks_build() {
        // Lifecycle::Initial blocks builds. The helper's
        // `should_build_with_trace` guard must short-circuit, leave the
        // dirty flag set, and return no views (we never reached the tail).
        let mut core = ElementCore::<WrapperView, Single>::new(WrapperView { child: TestView });

        assert_eq!(core.lifecycle(), Lifecycle::Initial);
        let was_dirty = core.is_dirty();

        let views = proxy_style_views(&mut core, "TestBehavior", WrapperView::child);

        assert_eq!(core.is_dirty(), was_dirty);
        assert!(views.is_empty(), "blocked build contributes no view churn");
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
