//! Shared helpers for [`ElementBehavior`](super::behavior::ElementBehavior)
//! impls.
//!
//! Free functions extracted from the per-behavior `perform_build` /
//! `on_mount` / `on_unmount` / `on_update` bodies so each impl carries
//! only its behavior-specific path.
//!
//! ## Design (locked)
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
//! ## Recovery ownership
//!
//! Build recovery classifies the original panic before retaining its opaque
//! payload. Factories keep their own failure authority, and diagnostic failure
//! cannot discard a successful recovery view or its attributed record. Public
//! integration rows exercise these boundaries through actual element builds.

use std::{any::TypeId, panic::AssertUnwindSafe};

use flui_foundation::RenderId;

use super::{arity::ElementArity, generic::ElementCore};
use crate::{
    owner::{LifecycleHook, RecoveredAt, RecoveredPanic},
    view::{FrameworkError, IntoView, View},
};

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
    V: Clone + 'static,
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

/// Stamp `render_id`'s node with `SliverMultiBoxAdaptorParentData { index }`
/// so its lazy-sliver parent can map `logical -> dense slot` from parent
/// data alone.
///
/// Two callers, one invariant ("every direct render child of a sparse
/// sliver host carries its logical index"): `RenderBehavior::on_mount`
/// stamps at adoption for freshly-mounted render objects, however deep
/// below the sparse child they sit; `SparseChildren::ensure` stamps the
/// first render descendants of a subtree that arrived by GlobalKey
/// relocation, which never re-mounts.
///
/// A node that already carries `SliverMultiBoxAdaptorParentData` — a
/// relocated child that was laid out under its previous host — has only its
/// `index` rewritten: `layout_offset` is the sliver's own state, and replacing
/// the box would reset it until the next layout recomputed it. A fresh node
/// (no parent data) or one carrying another type gets a new box.
///
/// Keep-alive is deliberately absent from this: a hold lives on the element
/// side, keyed by element identity, so relocation carries it with no help from
/// the stamp (ADR-0056).
pub(crate) fn stamp_sliver_slot(
    owner: &mut flui_rendering::pipeline::PipelineOwner,
    render_id: flui_foundation::RenderId,
    slot: flui_rendering::parent_data::SliverSlot,
) {
    use flui_rendering::parent_data::SliverMultiBoxAdaptorParentData;
    let Some(node) = owner.render_tree_mut().get_mut(render_id) else {
        return;
    };
    // Both halves, always together. Writing only the logical index on the
    // update path would leave a stale semantic position beside a fresh logical
    // one, which is the exact drift `SliverSlot` exists to make impossible.
    if let Some(existing) = node
        .parent_data_mut()
        .and_then(|pd| pd.downcast_mut::<SliverMultiBoxAdaptorParentData>())
    {
        existing.index = slot.logical;
        existing.semantic_index = slot.semantic;
        existing.semantic_set_size = slot.set_size;
        return;
    }
    let mut fresh =
        SliverMultiBoxAdaptorParentData::with_semantic_index(slot.logical, slot.semantic);
    fresh.semantic_set_size = slot.set_size;
    node.set_parent_data(Box::new(fresh));
}

/// Run a user `build()` closure under [`std::panic::catch_unwind`] and,
/// on a caught panic, substitute the registered `ErrorView`.
///
/// This is the producer half of the `ErrorView` recovery path — the
/// receiver (`ErrorView` + `FrameworkError` +
/// `set_error_view_builder`) already exists; this wires the catch that
/// feeds it. The catch wraps `build()` and replaces the built view with the
/// registered error view.
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
/// insert new) is the slab-resident form of a force-from-null rebuild: the
/// old child is deactivated and the error view mounted fresh.
///
/// `behavior_name` names what was building (e.g. `"building
/// StatelessElement"`) for the `FrameworkError` breadcrumb.
///
/// # Recording the panic
///
/// A caught panic is also recorded as a [`RecoveredPanic`] through
/// `owner`, so a later drain (`BuildOwner::take_recovered_panics`) can
/// forward it instead of it vanishing into `tracing` alone — see
/// `crate::owner::recovered_panic`. When `core.self_id()` is `None` (a
/// hand-rolled element that bypassed `ElementTree::insert`; test fixtures
/// only), there is no slab id to attach the record to, so the push is
/// skipped and a `tracing::error!` is the only signal.
pub(crate) fn build_or_recover<V, A, F>(
    core: &mut ElementCore<V, A>,
    owner: &mut crate::ElementOwner<'_>,
    behavior_name: &'static str,
    build: F,
) -> Box<dyn View>
where
    V: Clone + 'static,
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
    // ADR-0074: every element kind builds through this one function, so this
    // is where "an element is building" is armed for the ui_runtime's reactive
    // graph — reads re-derive the reader set, writes and slot creations are
    // refused. Disarmed on both exits (the panic path below included).
    let building = core.self_id();
    if let Some(id) = building {
        owner.reactive.begin_element_build(id);
    }
    match std::panic::catch_unwind(AssertUnwindSafe(build)) {
        Ok(child_view) => {
            if let Some(id) = building {
                owner.reactive.end_element_build(id, true);
            }
            child_view
        }
        Err(payload) => {
            // Classification borrows the original; retire it before recovery
            // invokes user factories or diagnostics. Opaque aggregate drop glue
            // could otherwise abort even after successful ErrorView substitution.
            let Some(element) = building else {
                let error = FrameworkError::from_panic(
                    payload.as_ref(),
                    format!("building {behavior_name}"),
                );
                flui_foundation::panic::retain_opaque_payload(payload);
                if let Err(payload) = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    tracing::error!(
                        behavior_name,
                        panic_message = %error.message,
                        "lifecycle hook panicked outside the slab; substituting ErrorView \
                         (no element id to record)"
                    );
                })) {
                    flui_foundation::panic::retain_opaque_payload(payload);
                }
                return crate::view::ErrorView::build_error_view(&error);
            };
            let panic = RecoveredPanic::from_payload(
                RecoveredAt::Element {
                    element,
                    parent: None,
                },
                TypeId::of::<V>(),
                LifecycleHook::Build,
                payload.as_ref(),
                format!("building {behavior_name}"),
            );
            flui_foundation::panic::retain_opaque_payload(payload);
            owner.reactive.end_element_build(element, false);
            // A recovery factory failure retains its existing authority. No
            // successful recovery record is published until it returns a view.
            let error_view = crate::view::ErrorView::build_error_view(&panic.error);
            if let Some(flag) = owner.build_recovered {
                flag.set(true);
            }
            owner.push_recovered_panic(panic);
            error_view
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
// `R` parameter keeps the child type static rather than a `Box<dyn View>`.
#[must_use]
pub(crate) fn single_child_views<V, A, R>(
    core: &mut ElementCore<V, A>,
    child_view: R,
    behavior_name: &'static str,
) -> Vec<Box<dyn View>>
where
    V: Clone + 'static,
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
    V: Clone + 'static,
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

/// Body of `RenderBehavior::on_unmount`: remove the associated
/// `RenderObject` from the `RenderTree`. No-op if the behavior never
/// got a `RenderId` (e.g. unmount before mount with no `PipelineOwner`).
pub(crate) fn remove_render_object_from_tree<V, A>(
    core: &ElementCore<V, A>,
    render_id: Option<RenderId>,
    behavior_name: &'static str,
) where
    V: Clone + 'static,
    A: ElementArity,
{
    if let Some(render_id) = render_id
        && let Some(pipeline_owner) = core.pipeline_owner()
    {
        // `remove_render_object` cascades; the non-cascading primitive is
        // `RenderTree::remove_shallow`. Element unmount wants the cascade —
        // when a parent element unmounts, all descendant render objects must
        // come down with it.

        // Dispose protocol: evict dirty entries, then free the slots.
        pipeline_owner.with_mut(|owner| owner.remove_render_object(render_id));
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
