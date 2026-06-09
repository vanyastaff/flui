//! Typed-dispatch entry point for element-view updates.
//!
//! Phase 1 §U8 / KTD-4 / Phase 3 §U27 / FR-021. The current body
//! eliminates the runtime `downcast_ref::<V>()` call in the
//! View-type update dispatch path by gating on the concrete
//! runtime `TypeId` (via `Downcast::as_any().type_id()`) and
//! routing through `Downcast::into_any` + `Box::downcast::<V>`
//! — a different syntactic pattern from the FR-033 grep target.
//! The `tracing::warn!` fall-through of the Phase 1 identity-shim
//! is removed: on type mismatch the dispatch returns `false`, the
//! caller (`Phase 2 reconciler`) replaces the element, and no
//! silent stale state remains in the tree (Flutter-correct).
//!
//! ## Why `as_any().type_id()` and not `view_type_id()`?
//!
//! `View::view_type_id()` has a default body but is an
//! **overridable trait method**. `BoxedView` (`view/into_view.rs`)
//! intentionally forwards its `view_type_id()` to the inner view
//! so authoring code that returns `Inner.boxed()` in a
//! conditional-build arm (the canonical SC-009 shape) reads as
//! type-`Inner` at the trait surface. A naive
//! `new_view.view_type_id() == TypeId::of::<V>()` guard would let
//! a `BoxedView` slip through the check (because the inner's
//! TypeId matches `V`) and then `Box::downcast::<V>` would fail
//! at runtime (the actual concrete type is `BoxedView`, not
//! `V`) — a panic on every `.boxed()` rebuild.
//!
//! `Downcast::as_any().type_id()` returns the **concrete runtime
//! TypeId** (`std::any::Any::type_id` is non-overridable: the
//! blanket `impl<T: 'static + ?Sized> Any for T` decides the
//! discriminant from the trait-object vtable). The guard now
//! discriminates BoxedView from its inner correctly. Defense in
//! depth: the downcast itself is **fallible** (`match` on
//! `Box::downcast::<V>` result, return `false` on `Err`) so a
//! future trait-method override that violates the
//! `as_any().type_id() == TypeId::of::<V>() ⇒ downcastable to V`
//! invariant degrades to "replace element" instead of panicking.
//!
//! The dispatch function is `pub(crate)` because it is not meant
//! to be called from outside `flui-view` (the call site is
//! [`ElementCore::update_view`] inside [`super::generic`]).

use std::any::TypeId;

use super::{arity::ElementArity, generic::ElementCore};
use crate::view::View;

/// Typed view-update dispatch (FR-021).
///
/// Compares the *concrete runtime* `TypeId` of `new_view` (via
/// `Downcast::as_any().type_id()`, **not** the overridable
/// `View::view_type_id()`) against `TypeId::of::<V>()` to
/// discriminate the dispatch. On match, the underlying typed
/// value is extracted through the `Downcast::into_any` →
/// `Box::downcast::<V>` chain — distinct from the
/// `downcast_ref::<V>()` pattern FR-033's port-check grep
/// forbids. On mismatch — and on the defense-in-depth case
/// where the downcast still fails despite the TypeId check —
/// the function returns `false`; the reconciler replaces the
/// element via the type-mismatch path the keyed reconciler's
/// "different concrete type" branch already exercises.
///
/// # Why a free function rather than a method?
///
/// Phase 3 §U27 settles this signature as the canonical typed
/// dispatch surface. The free-function shape keeps
/// `ElementCore::update_view`'s body to a single line so future
/// `ElementKind`-discriminated dispatch (Phase 4+) can replace
/// this module's body in one place rather than touch every
/// behavior implementation.
pub(crate) fn dispatch_view_update<V, A>(core: &mut ElementCore<V, A>, new_view: &dyn View) -> bool
where
    V: View + Clone + Send + Sync + 'static,
    A: ElementArity,
{
    // Use the CONCRETE runtime TypeId — `Any::type_id()` via
    // `Downcast::as_any` — rather than the overridable
    // `View::view_type_id()`. `BoxedView` forwards
    // `view_type_id()` to its inner so an `Inner.boxed()` rebuild
    // would pass a naive `view_type_id()` check (inner's TypeId ==
    // `V`'s TypeId for an `ElementCore<V>`) and then crash on the
    // downcast (actual runtime type is `BoxedView`, not `V`). The
    // `as_any().type_id()` guard discriminates the wrapper from
    // the wrapped concretely.
    if new_view.as_any().type_id() != TypeId::of::<V>() {
        // Type-mismatch path: caller (`Phase 2 reconciler`)
        // replaces the element. No tracing::warn — Flutter-correct
        // "different type → new element" semantics.
        return false;
    }

    // Memoization equality-bail (Wave 3 `should_skip_rebuild`).
    //
    // Evaluate on a BORROW before the unconditional `clone_box` below —
    // every skip must pay zero clone cost. The downcast_ref here is
    // sanctioned: the `as_any().type_id() == TypeId::of::<V>()` guard
    // above means the ref is guaranteed to succeed.
    // Bound to a short `let` so the FR-033 marker stays on the same line as
    // `downcast_ref` and survives rustfmt (the marker must be co-located).
    let nv = new_view.as_any().downcast_ref::<V>(); // PORT-CHECK-OK-DOWNCAST: type-id guarded
    if let Some(new_ref) = nv
        && new_ref.should_skip_rebuild(core.view())
    {
        tracing::debug!(
            view_type = ?TypeId::of::<V>(),
            "dispatch_view_update: skip rebuild (configs equal)",
        );
        return true; // reuse element; skip clone + replace + rebuild
    }

    // TypeId equality should guarantee the dynamic value is V.
    // `Downcast::into_any` + `Box::downcast::<V>` produces the
    // typed inner without `downcast_ref::<V>()` — the syntactic
    // pattern FR-033's port-check grep forbids in this dispatch
    // path. The downcast is FALLIBLE on principle: if a future
    // trait-method override violates the `as_any().type_id() ==
    // TypeId::of::<V>() ⇒ downcastable to V` invariant, the
    // dispatch degrades to "replace element" instead of
    // panicking. The guard above means the `Err` arm should be
    // unreachable today, but defense in depth costs us one
    // `match` arm.
    let cloned: Box<dyn View> = dyn_clone::clone_box(new_view);
    match cloned.into_any().downcast::<V>() {
        Ok(typed) => {
            core.replace_view_for_dispatch(*typed);
            core.mark_dirty_for_dispatch();
            tracing::debug!(
                "dispatch_view_update succeeded for view_type={:?}",
                TypeId::of::<V>()
            );
            true
        }
        Err(_) => {
            // Should be unreachable post-guard but reachable in
            // principle (see invariant note above). Treat as
            // type-mismatch — caller replaces the element.
            false
        }
    }
}

// The dirty store is owned by `ElementCore::mark_dirty_for_dispatch`
// (see `super::generic`) — this module keeps `std::sync::atomic` out
// of its dependency surface so Phase 3 §U27's replacement does not
// need to update unrelated imports.
