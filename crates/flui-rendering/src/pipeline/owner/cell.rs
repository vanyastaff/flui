//! `PipelineCell` -- owner-local shared handle to one presentation's
//! [`PipelineOwner`].
//!
//! Replaces the historical `Arc<RwLock<PipelineOwner>>` shape: a
//! `PipelineOwner` belongs to exactly one presentation running on exactly
//! one thread, so the concurrency machinery an `RwLock` provided was never
//! load-bearing -- it only ever guarded a checkout slot (see
//! `BuildOwner::run_frame_with_layout_builders`, which `mem::take`s the
//! owner out, runs a typestate phase, and puts it back).

use std::{cell::RefCell, fmt, rc::Rc};

use super::PipelineOwner;

/// Owner-local shared handle to one presentation's [`PipelineOwner`].
///
/// `!Send + !Sync` by construction (via the inner `Rc<RefCell<_>>`) --
/// closure-scoped access only, no guard type. Cloning a `PipelineCell`
/// shares the same underlying owner (shallow share, like `Rc::clone`); it
/// does not copy the owner's state.
///
/// # Reentrancy
///
/// [`with`](Self::with) calls nest freely -- `RefCell` shared borrows
/// coexist. [`with_mut`](Self::with_mut) does **not** nest: calling it
/// while any `with`/`with_mut` borrow from the *same* cell is still on the
/// call stack panics with a `BUG:` message:
///
/// ```should_panic
/// use flui_rendering::pipeline::{PipelineCell, PipelineOwner};
///
/// let cell = PipelineCell::new(PipelineOwner::new());
/// cell.with_mut(|_owner| {
///     // Reentrant with_mut on the same cell -- panics:
///     // "BUG: PipelineCell::with_mut called reentrantly -- ..."
///     cell.with_mut(|_owner| {});
/// });
/// ```
///
/// # Leak hazard
///
/// Render objects must **not** store a `PipelineCell`. Nothing prevents it
/// at the type level -- the anti-cycle argument only closes the two
/// back-pointers that used to exist (`RenderTree::owner`,
/// `RenderView::owner`, both deleted as part of this same change). A render object holding a
/// `PipelineCell` would close `cell -> owner -> tree -> object -> cell`, an
/// `Rc` cycle nothing frees. Dirty-marking from inside a render object goes
/// through [`RepaintHandle`](crate::pipeline::RepaintHandle) instead -- a
/// weak, generational, least-privilege handle built for exactly this seam.
#[derive(Clone)]
pub struct PipelineCell(Rc<RefCell<PipelineOwner>>);

impl PipelineCell {
    /// Wraps a fresh, idle [`PipelineOwner`] in an owner-local cell.
    pub fn new(owner: PipelineOwner) -> Self {
        Self(Rc::new(RefCell::new(owner)))
    }

    /// Runs `f` with shared access to the owner.
    ///
    /// May be called from inside another `with` on the same cell (nesting
    /// is guaranteed); see the type-level docs for the `with_mut`
    /// reentrancy contract.
    pub fn with<R>(&self, f: impl FnOnce(&PipelineOwner) -> R) -> R {
        let owner = self.0.borrow();
        f(&owner)
    }

    /// Runs `f` with exclusive access to the owner.
    ///
    /// # Panics
    ///
    /// Panics if called while any `with`/`with_mut` borrow from this same
    /// cell is already live on the call stack (a reentrant checkout) -- see
    /// the type-level "Reentrancy" docs.
    pub fn with_mut<R>(&self, f: impl FnOnce(&mut PipelineOwner) -> R) -> R {
        let mut owner = self.0.try_borrow_mut().expect(
            "BUG: PipelineCell::with_mut called reentrantly -- a with/with_mut borrow from \
             this cell is already live on the call stack",
        );
        f(&mut owner)
    }

    /// Reports whether the owner is currently free for exclusive access.
    ///
    /// Pinned to the strict form -- `try_borrow_mut().is_ok()`, not a
    /// shared-borrow check. The literal translation of the historical
    /// `try_read().is_some()` would be write-only detection, but the actual
    /// precondition callers care about (e.g. "build is about to mount a
    /// child via `with_mut`") is "the owner is free for exclusive access",
    /// and under closure-scoped access no `with` borrow can legally still
    /// be live at any call site that would ask this question.
    pub fn is_free(&self) -> bool {
        self.0.try_borrow_mut().is_ok()
    }

    /// Whether `self` and `other` share the same underlying owner (a shallow
    /// clone of one `Rc`), as opposed to two distinct owners that merely
    /// hold equal-looking state. Mirrors [`Rc::ptr_eq`] without exposing the
    /// private `Rc` field.
    #[must_use]
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }

    /// Test-only weak probe: lets a leak test confirm the owner is freed
    /// (no outstanding strong clone) once every `PipelineCell` referencing
    /// it has dropped.
    #[cfg(any(test, feature = "testing"))]
    pub fn downgrade_for_test(&self) -> std::rc::Weak<RefCell<PipelineOwner>> {
        Rc::downgrade(&self.0)
    }
}

impl fmt::Debug for PipelineCell {
    /// Manual, minimal impl: a derived `Debug` would route through
    /// `RefCell`'s `try_borrow` representation (printing the borrowed
    /// owner's own `Debug`, or panicking/showing a placeholder while
    /// checked out), which is noisy and can itself observably contend with
    /// an in-flight `with_mut`. A `PipelineCell` is a handle, not the data
    /// -- print it as one.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PipelineCell")
            .field("is_free", &self.is_free())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use static_assertions::assert_not_impl_any;

    use super::*;

    // A manual `impl Send`/`Sync` reappearing on either type would silently
    // reopen the cross-thread aliasing hazard `PipelineCell` exists to close
    // (a `PipelineOwner` sent across threads while another thread holds a
    // `with`/`with_mut` borrow is a data race the old `RwLock` masked as a
    // deadlock instead). Both must stay `!Send + !Sync` for as long as
    // `PipelineCell` wraps `Rc<RefCell<_>>`.
    assert_not_impl_any!(PipelineCell: Send, Sync);
    assert_not_impl_any!(PipelineOwner: Send, Sync);

    #[test]
    fn with_nests_freely() {
        let cell = PipelineCell::new(PipelineOwner::new());
        cell.with(|_outer| {
            cell.with(|_inner| {
                // Two coexisting shared borrows must not panic.
            });
        });
    }

    #[test]
    fn with_mut_reentry_panics() {
        let cell = PipelineCell::new(PipelineOwner::new());
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            cell.with_mut(|_outer| {
                cell.with_mut(|_inner| {});
            });
        }));
        let err = result.expect_err("reentrant with_mut must panic");
        let message = err
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| err.downcast_ref::<String>().map(String::as_str))
            .expect("panic payload must be a string");
        assert!(
            message.contains("BUG: PipelineCell::with_mut called reentrantly"),
            "unexpected panic message: {message}"
        );
    }

    #[test]
    fn with_mut_inside_with_panics() {
        let cell = PipelineCell::new(PipelineOwner::new());
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            cell.with(|_outer| {
                cell.with_mut(|_inner| {});
            });
        }));
        result.expect_err("with_mut nested inside with must panic");
    }

    #[test]
    fn is_free_reflects_checkout_state() {
        let cell = PipelineCell::new(PipelineOwner::new());
        assert!(cell.is_free());
        cell.with_mut(|_owner| {
            assert!(!cell.is_free());
        });
        assert!(cell.is_free());
    }

    #[test]
    fn clone_shares_the_same_owner() {
        let cell = PipelineCell::new(PipelineOwner::new());
        let shared = cell.clone();
        cell.with_mut(|_owner| {
            assert!(!shared.is_free(), "clone must observe the same checkout");
        });
    }
}
