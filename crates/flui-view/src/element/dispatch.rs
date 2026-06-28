//! Typed-dispatch entry point for element-view updates.
//!
//! Phase 1 §U8 / KTD-4 / Phase 3 §U27 / FR-021. The current body
//! eliminates the runtime `downcast_ref::<V>()` call in the
//! View-type update dispatch path by gating on the concrete
//! runtime `TypeId` (via `Downcast::as_any().type_id()`) and
//! routing through `Downcast::into_any` + `Box::downcast::<V>`
//! — a different syntactic pattern from the FR-033 grep target.
//! The `tracing::warn!` fall-through of the Phase 1 identity-shim
//! is removed: after unwrapping any `BoxedView` (below), a genuine
//! concrete-type mismatch returns `false`, the caller (`Phase 2
//! reconciler`) replaces the element, and no silent stale state
//! remains in the tree (Flutter-correct).
//!
//! ## `BoxedView` unwrap, then `as_any().type_id()`
//!
//! Two facts make a raw concrete-`TypeId` comparison wrong on its
//! own. (1) `View::view_type_id()` is **overridable**, and
//! `BoxedView` (`view/into_view.rs`) forwards it to its inner, so
//! the reconciler's `can_update_by_id` treats `Inner.boxed()` as
//! type-`Inner` and *reuses* the element rather than replacing it.
//! (2) The single-child build path boxes a child through
//! `Box<dyn View>: IntoView` (yielding a `BoxedView`), and user
//! code routinely returns `child.boxed()` — yet the mounted
//! element is `Element<Inner>` (`BoxedView::create_element`
//! delegates to the inner). So the value reaching this dispatch is
//! frequently a `BoxedView` whose **concrete** runtime TypeId is
//! `BoxedView`, not `V`.
//!
//! Therefore the dispatch first **unwraps** nested `BoxedView`
//! wrappers to the inner `&dyn View`, then compares its concrete
//! `Downcast::as_any().type_id()` (non-overridable — the blanket
//! `impl<T: 'static + ?Sized> Any for T` reads the discriminant
//! from the vtable) against `TypeId::of::<V>()`. On match the inner
//! is cloned and `Box::downcast::<V>` succeeds (so the element
//! updates in place — consistent with `can_update_by_id` and with
//! Flutter's update-in-place); on a real mismatch it returns
//! `false` and the reconciler replaces the element. Without the
//! unwrap a boxed rebuild was neither updated (TypeId saw
//! `BoxedView`) nor replaced (`can_update` saw `Inner`) — a
//! silently stale render object after every `setState`.
//!
//! The dispatch function is `pub(crate)` because it is not meant
//! to be called from outside `flui-view` (the call site is
//! [`ElementCore::update_view`] inside [`super::generic`]).

use std::any::TypeId;

use super::{arity::ElementArity, generic::ElementCore};
use crate::view::{BoxedView, View};

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
    // Unwrap framework/user `BoxedView` wrappers so an `Inner.boxed()` rebuild
    // updates the `Inner` element in place. The single-child build path boxes
    // a child through `Box<dyn View>: IntoView` (→ `BoxedView`), and user code
    // routinely returns `child.boxed()`; in both cases the mounted element is
    // `Element<Inner>` (BoxedView::create_element delegates to the inner). The
    // raw `as_any().type_id()` below sees `BoxedView`, not `V`, while the
    // reconciler's `can_update_by_id` matched on the BoxedView-forwarded
    // `view_type_id()` (the inner type) — so without this unwrap such a rebuild
    // is neither updated (TypeId mismatch here) nor replaced (can_update said
    // reuse), leaving the element silently stale. Unwrapping is strictly safer
    // than the old bail-to-`false`: the `Box::downcast::<V>` below now runs on
    // the inner concrete value and cannot panic.
    let mut effective: &dyn View = new_view;
    loop {
        // Each hop strips one BoxedView layer. The FR-033 whitelist marker is
        // co-located: this downcasts to the `BoxedView` *wrapper* to unwrap it,
        // not the `downcast_ref::<V>()` view-type smuggling FR-033 bans.
        let wrapper = effective.as_any().downcast_ref::<BoxedView>(); // PORT-CHECK-OK-DOWNCAST: unwrap BoxedView wrapper, not V-type smuggling
        match wrapper {
            Some(boxed) => effective = &*boxed.0,
            None => break,
        }
    }
    if effective.as_any().type_id() != TypeId::of::<V>() {
        // Genuinely different concrete type → caller (`Phase 2 reconciler`)
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
    let nv = effective.as_any().downcast_ref::<V>(); // PORT-CHECK-OK-DOWNCAST: type-id guarded
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
    let cloned: Box<dyn View> = dyn_clone::clone_box(effective);
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
