//! Typed-dispatch entry point for element-view updates.
//!
//! The dispatch gates on the concrete runtime `TypeId` of the offered view,
//! read by upcasting the `&dyn View` to `&dyn Any` (`View: Any`); only once
//! that guard has matched `V` does it downcast to `V` and compare through it.
//! The element then takes the offered view's shared allocation when it came
//! in a `BoxedView`, and copies `V` into an allocation of its own only when
//! the view was offered by reference alone. No downcast runs on a value the
//! guard has not matched.
//! After unwrapping any `BoxedView` (below), a genuine concrete-type
//! mismatch is [`ViewUpdate::Mismatch`]: the reconciler replaces the
//! element, and no silent stale state remains in the tree.
//!
//! ## `BoxedView` unwrap, then the upcast's `type_id()`
//!
//! Two facts make a raw concrete-`TypeId` comparison wrong on its
//! own. (1) `View::view_type_id()` is **overridable**, and
//! `BoxedView` (`view/into_view.rs`) forwards it to its inner, so
//! the reconciler's `can_update_by_id` treats `Inner.boxed()` as
//! type-`Inner` and *reuses* the element rather than replacing it.
//! (2) Every child the reconciler offers is held in a `BoxedView`
//! (`ChildViews`), and user code routinely returns `child.boxed()` —
//! yet the mounted element is `Element<Inner>` (`BoxedView::create_element`
//! delegates to the inner). So the value reaching this dispatch is
//! usually a `BoxedView` whose **concrete** runtime TypeId is
//! `BoxedView`, not `V`.
//!
//! Therefore the dispatch first **unwraps** nested `BoxedView`
//! wrappers to the inner `&dyn View`, then upcasts that trait object to
//! `&dyn Any` and compares its `type_id()` (non-overridable — it is read
//! from the vtable of the concrete type) against `TypeId::of::<V>()`. The
//! upcast is of the trait object itself: `type_id()` on a `&&dyn View`
//! would name the reference type. On match the element
//! takes the `BoxedView`'s shared `Rc<V>`, or a copy of a view offered by
//! reference alone (so the element
//! updates in place — consistent with `can_update_by_id`); on a real mismatch it returns
//! [`ViewUpdate::Mismatch`] and the reconciler replaces the element. Without the
//! unwrap a boxed rebuild was neither updated (TypeId saw
//! `BoxedView`) nor replaced (`can_update` saw `Inner`) — a
//! silently stale render object after every `setState`.
//!
//! The dispatch function is `pub(crate)` because it is not meant
//! to be called from outside `flui-view` (the call site is
//! [`ElementCore::update_view`] inside [`super::generic`]).

use std::any::{Any, TypeId};
use std::rc::Rc;

use super::{arity::ElementArity, generic::ElementCore};
use crate::view::{BoxedView, View};

/// Typed view-update dispatch.
///
/// Downcasts `new_view`, once unwrapped of its `BoxedView` layers, through
/// its upcast to `&dyn Any` — the *concrete runtime* type, **not** the
/// overridable `View::view_type_id()` — to `V`. On match the element takes
/// the shared allocation of the `BoxedView` that offered it, or a copy when it
/// was offered by reference alone. On mismatch the function returns
/// [`ViewUpdate::Mismatch`]; the reconciler replaces the element via the
/// type-mismatch path the keyed reconciler's "different concrete type" branch
/// already exercises.
///
/// # Why a free function rather than a method?
///
/// This settles the signature as the canonical typed
/// dispatch surface. The free-function shape keeps
/// `ElementCore::update_view`'s body to a single line so future
/// `ElementKind`-discriminated dispatch can replace
/// this module's body in one place rather than touch every
/// behavior implementation.
pub(crate) fn dispatch_view_update<V, A>(
    core: &mut ElementCore<V, A>,
    new_view: &dyn View,
) -> ViewUpdate<V>
where
    V: View + Clone + 'static,
    A: ElementArity,
{
    // Unwrap framework/user `BoxedView` wrappers so an `Inner.boxed()` rebuild
    // updates the `Inner` element in place. Every child the reconciler offers
    // is a `BoxedView`, and user code routinely returns `child.boxed()`; in
    // both cases the mounted element is `Element<Inner>`
    // (`BoxedView::create_element` delegates to the inner). The upcast's
    // `type_id()` sees `BoxedView`, not `V`, while the reconciler's
    // `can_update_by_id` matched on the BoxedView-forwarded `view_type_id()`
    // (the inner type) — so without this unwrap such a rebuild is neither
    // updated (type mismatch here) nor replaced (`can_update` said reuse),
    // leaving the element silently stale.
    let (effective, shared) = unwrap_boxed(new_view);
    // The upcast of the trait object itself reads the concrete type from its
    // vtable; `View::view_type_id` is overridable and is not consulted.
    let Some(new_ref) = (effective as &dyn Any).downcast_ref::<V>() else {
        // Genuinely different concrete type → the reconciler replaces the
        // element. No tracing::warn — "different type → new element" is the
        // normal reconciliation outcome.
        return ViewUpdate::Mismatch;
    };

    // The very view the element was last given, offered again: a parent that
    // rebuilt handed its child back untouched. Nothing about the element's
    // configuration changed, so it is neither cloned, replaced nor marked
    // dirty; what changed for it (a state change, an inherited value) reaches
    // it by its own schedule. Checked ahead of `should_skip_rebuild`, which
    // compares contents and so costs a comparison this does not.
    if let Some(shared) = shared
        && core.is_taken_from(shared)
    {
        return ViewUpdate::Unchanged;
    }

    // Memoization equality-bail (`should_skip_rebuild`), evaluated on the
    // borrow before the element takes anything, so a skip costs no clone.
    if new_ref.should_skip_rebuild(core.view()) {
        trace_skipped(TypeId::of::<V>());
        return ViewUpdate::Equal;
    }

    // The element takes the offered view's shared allocation when it came in
    // a `BoxedView`, so the update copies nothing; a view offered by
    // reference alone is copied into an allocation of its own. The shared
    // allocation holds the `V` just downcast by reference, so its own
    // downcast cannot fail; should it, the element is replaced instead of the
    // dispatch panicking.
    let next = match shared {
        Some(shared) => (Rc::clone(shared) as Rc<dyn Any>).downcast::<V>().ok(),
        None => Some(Rc::new(new_ref.clone())),
    };
    match next {
        Some(next) => {
            let previous = core.replace_view_for_dispatch(next);
            core.mark_dirty_for_dispatch();
            trace_replaced(TypeId::of::<V>());
            ViewUpdate::Replaced { previous }
        }
        None => ViewUpdate::Mismatch,
    }
}

/// `view` with every `BoxedView` layer stripped, and the shared allocation of
/// the innermost layer when there was one. Each hop downcasts to the wrapper
/// to unwrap it, not to the concrete view type.
#[inline(never)]
fn unwrap_boxed(view: &dyn View) -> (&dyn View, Option<&Rc<dyn View>>) {
    let mut effective = view;
    let mut shared = None;
    while let Some(boxed) = (effective as &dyn Any).downcast_ref::<BoxedView>() {
        shared = Some(boxed.shared());
        effective = boxed.as_view();
    }
    (effective, shared)
}

#[inline(never)]
fn trace_skipped(view_type: TypeId) {
    tracing::debug!(
        ?view_type,
        "dispatch_view_update: skip rebuild (configs equal)",
    );
}

#[inline(never)]
fn trace_replaced(view_type: TypeId) {
    tracing::debug!(
        "dispatch_view_update succeeded for view_type={:?}",
        view_type
    );
}

/// What offering a view to an element did.
pub(crate) enum ViewUpdate<V> {
    /// The view is of another type: the caller replaces the element.
    Mismatch,
    /// The element already holds this very view: nothing was touched.
    Unchanged,
    /// The view compares equal to the one held, under
    /// [`View::should_skip_rebuild`]: the element keeps the view it has and is
    /// not marked dirty, and its behavior still hears of the update, with the
    /// held view as the previous one.
    Equal,
    /// The element took the view and its behavior reacts to the update, given
    /// the view it held before.
    Replaced {
        /// The configuration the element held before the update.
        previous: Rc<V>,
    },
}

// The dirty store is owned by `ElementCore::mark_dirty_for_dispatch`
// (see `super::generic`) — this module keeps `std::sync::atomic` out
// of its dependency surface so a future replacement of this dispatch
// does not need to update unrelated imports.
