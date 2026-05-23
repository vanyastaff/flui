//! Typed-dispatch entry point for element-view updates.
//!
//! Phase 1 §U8 / KTD-4 / FR-021. This module is the **long-lived
//! future home** of typed `ElementKind`-discriminated view-update
//! dispatch — Phase 3 §U27 replaces the body of
//! [`dispatch_view_update`] with a real outer match over the closed
//! `ElementKind` variant set, eliminating the `downcast_ref::<V>()`
//! call entirely.
//!
//! ## Phase 1 status: identity-shim
//!
//! The current body produces the same observable behavior as the
//! pre-FR-021 `ElementCore::update_view` body — a runtime downcast
//! that succeeds on type match and falls through with a
//! `tracing::warn!` on miss. Routing through this dedicated
//! function gives Phase 2 and Phase 3 a single replacement point
//! without forcing a touchy edit on every behavior implementation.
//!
//! The dispatch function is `pub(crate)` because it is not meant to
//! be called from outside `flui-view` (the call site is
//! [`ElementCore::update_view`] inside [`super::generic`]).

use std::any::TypeId;

use super::{arity::ElementArity, generic::ElementCore};
use crate::view::View;

/// Phase 1 identity-shim dispatch.
///
/// Replaces the inline `downcast_ref::<V>()` body in
/// [`ElementCore::update_view`] under default features. Phase 3 §U27
/// rewrites the body to dispatch through the typed `ElementKind`
/// variant — at that point the runtime downcast disappears entirely
/// (FR-021 closes) and the `tracing::warn!` fall-through becomes
/// unreachable.
///
/// # Why a free function rather than a method?
///
/// The function takes a fully-typed `&mut ElementCore<V, A>` and a
/// `&dyn View`. Phase 3 will introduce overloads that take
/// `&mut ElementKind` directly, at which point this signature is
/// retired. Keeping it as a free function (not a method on
/// `ElementCore`) makes the replacement a single-import change,
/// not a touch on every behavior that consumes the result.
pub(crate) fn dispatch_view_update<V, A>(core: &mut ElementCore<V, A>, new_view: &dyn View) -> bool
where
    V: Clone + Send + Sync + 'static,
    A: ElementArity,
{
    // Identity-shim body — mirrors the legacy path 1:1 so default
    // builds preserve behavior while the dispatch boundary moves to
    // this module. Phase 3 §U27 replaces this body with the typed
    // `ElementKind` match.
    if let Some(v) = new_view.as_any().downcast_ref::<V>() {
        core.replace_view_for_dispatch(v.clone());
        core.mark_dirty_for_dispatch();
        tracing::debug!(
            "dispatch::dispatch_view_update succeeded for view_type={:?}",
            TypeId::of::<V>()
        );
        true
    } else {
        // Phase 1 retains the warn-then-continue behavior of the
        // pre-FR-021 path. Phase 3 §U27 removes this branch — the
        // typed `ElementKind` match makes the case unreachable at
        // compile time.
        tracing::warn!(
            "dispatch::dispatch_view_update failed to downcast for view_type={:?}",
            TypeId::of::<V>()
        );
        false
    }
}

// The dirty store is owned by `ElementCore::mark_dirty_for_dispatch`
// (see `super::generic`) — this module keeps `std::sync::atomic` out
// of its dependency surface so Phase 3 §U27's replacement does not
// need to update unrelated imports.
