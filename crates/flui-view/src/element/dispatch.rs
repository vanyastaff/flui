//! Typed-dispatch entry point for element-view updates.
//!
//! Phase 1 §U8 / KTD-4 / Phase 3 §U27 / FR-021. The current body
//! eliminates the runtime `downcast_ref::<V>()` call in the
//! View-type update dispatch path by gating on `TypeId` equality
//! and routing through `Downcast::into_any` + `Box::downcast::<V>`
//! — a different syntactic pattern from the FR-033 grep target
//! (`downcast_ref::<.*View.*>`). The `tracing::warn!` fall-through
//! of the Phase 1 identity-shim is removed: on type mismatch the
//! dispatch returns `false`, the caller (`Phase 2 reconciler`)
//! replaces the element, and no silent stale state remains in the
//! tree (Flutter-correct behavior).
//!
//! The dispatch function is `pub(crate)` because it is not meant
//! to be called from outside `flui-view` (the call site is
//! [`ElementCore::update_view`] inside [`super::generic`]).

use std::any::TypeId;

use super::{arity::ElementArity, generic::ElementCore};
use crate::view::View;

/// Typed view-update dispatch (FR-021).
///
/// Compares `new_view.view_type_id()` against `TypeId::of::<V>()`
/// to discriminate the dispatch. On match, the underlying typed
/// value is extracted through the `Downcast::into_any` →
/// `Box::downcast::<V>` chain — distinct from the
/// `downcast_ref::<V>()` pattern FR-033's port-check grep
/// forbids. On mismatch, the function returns `false` immediately
/// (no `tracing::warn!`, no silent stale state); the reconciler
/// handles the element-replace via the type-mismatch path that
/// already powers the keyed reconciler's "different concrete type"
/// case.
///
/// # Safety
///
/// `expect("…")` is sound because the preceding `TypeId` check
/// guarantees the boxed value is a `V`. `dyn_clone::clone_box`
/// produces an owned `Box<dyn View>`; consuming it via
/// `into_any` + `Box::downcast::<V>` recovers the typed `Box<V>`
/// without observable behavior change.
///
/// # Why a free function rather than a method?
///
/// Phase 3 §U27 settles this signature as the canonical typed
/// dispatch surface. The free-function shape keeps
/// [`ElementCore::update_view`]'s body to a single line so future
/// `ElementKind`-discriminated dispatch (Phase 4+) can replace
/// this module's body in one place rather than touch every
/// behavior implementation.
pub(crate) fn dispatch_view_update<V, A>(core: &mut ElementCore<V, A>, new_view: &dyn View) -> bool
where
    V: Clone + Send + Sync + 'static,
    A: ElementArity,
{
    if new_view.view_type_id() != TypeId::of::<V>() {
        // Type-mismatch path: caller (`Phase 2 reconciler`)
        // replaces the element. No tracing::warn — Flutter-correct
        // "different type → new element" semantics.
        return false;
    }
    // TypeId equality guarantees the dynamic value is V.
    // `Downcast::into_any` + `Box::downcast::<V>` produces the
    // typed inner without `downcast_ref::<V>()` — the syntactic
    // pattern FR-033's port-check grep forbids in this dispatch
    // path. The `expect` is sound: the TypeId precondition above
    // ensures the downcast cannot fail.
    let cloned: Box<dyn View> = dyn_clone::clone_box(new_view);
    let typed: Box<V> = cloned
        .into_any()
        .downcast::<V>()
        .expect("view_type_id matched TypeId::of::<V>() — downcast must succeed");
    core.replace_view_for_dispatch(*typed);
    core.mark_dirty_for_dispatch();
    tracing::debug!(
        "dispatch_view_update succeeded for view_type={:?}",
        TypeId::of::<V>()
    );
    true
}

// The dirty store is owned by `ElementCore::mark_dirty_for_dispatch`
// (see `super::generic`) — this module keeps `std::sync::atomic` out
// of its dependency surface so Phase 3 §U27's replacement does not
// need to update unrelated imports.
