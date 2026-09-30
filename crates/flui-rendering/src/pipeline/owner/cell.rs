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
/// let cell = PipelineCell::new(PipelineOwner::new(flui_rendering::TextContextHandle::standalone()));
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
/// through [`RenderInvalidationHandle`](crate::pipeline::RenderInvalidationHandle) instead -- a
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
    ///
    /// ```
    /// use flui_rendering::pipeline::{PipelineCell, PipelineOwner};
    ///
    /// let cell = PipelineCell::new(PipelineOwner::new(flui_rendering::TextContextHandle::standalone()));
    /// let dpr = cell.with(PipelineOwner::device_pixel_ratio);
    /// assert_eq!(dpr, 1.0, "a fresh PipelineOwner defaults to 1x");
    ///
    /// // Nesting is fine -- two coexisting shared borrows never panic.
    /// cell.with(|outer| {
    ///     let root = cell.with(|inner| inner.root_id());
    ///     assert_eq!(outer.root_id(), root);
    /// });
    /// ```
    pub fn with<R>(&self, f: impl FnOnce(&PipelineOwner) -> R) -> R {
        let owner = self.0.borrow();
        f(&owner)
    }

    /// A non-owning handle to this cell.
    ///
    /// For a holder that must not keep the render tree alive. A presentation's
    /// teardown drops its `PipelineCell`; anything still holding a strong
    /// clone silently keeps that whole tree — and its dirty-request receiver —
    /// alive past the close, which turns "fail closed" into "quietly keeps
    /// working on a presentation the app has shut".
    #[must_use]
    pub fn downgrade(&self) -> WeakPipelineCell {
        WeakPipelineCell(Rc::downgrade(&self.0))
    }

    /// [`Self::with`], but answers `None` instead of panicking when a
    /// `with_mut` borrow is live.
    ///
    /// For callers that legitimately might run while a frame holds the tree
    /// checked out, and for whom "the tree is busy" is a real answer rather
    /// than a bug. A fresh hit test is the case: the capability is
    /// lifecycle-acquired, but nothing stops the *call* landing in a callback
    /// that a frame phase happens to drive, and reporting a busy tree beats
    /// both panicking and inventing an empty result.
    ///
    /// Do not reach for this to paper over a genuine reentrancy bug —
    /// [`Self::with_mut`]'s panic is load-bearing for the frame pipeline.
    pub fn try_with<R>(&self, f: impl FnOnce(&PipelineOwner) -> R) -> Option<R> {
        self.0.try_borrow().ok().map(|owner| f(&owner))
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
    ///
    /// ```
    /// use flui_rendering::pipeline::{PipelineCell, PipelineOwner};
    ///
    /// let cell = PipelineCell::new(PipelineOwner::new(flui_rendering::TextContextHandle::standalone()));
    /// assert!(cell.is_free());
    ///
    /// cell.with_mut(|_owner| {
    ///     assert!(!cell.is_free(), "checked out for the duration of with_mut");
    /// });
    ///
    /// assert!(cell.is_free(), "free again once with_mut returns");
    /// ```
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

/// A non-owning [`PipelineCell`] handle, from [`PipelineCell::downgrade`].
///
/// `upgrade` answers `None` once every strong holder is gone — which for a
/// presentation's pipeline means the presentation has closed.
#[derive(Clone, Debug)]
pub struct WeakPipelineCell(std::rc::Weak<RefCell<PipelineOwner>>);

impl WeakPipelineCell {
    /// Reclaim a strong handle, or `None` if the tree is gone.
    #[must_use]
    pub fn upgrade(&self) -> Option<PipelineCell> {
        self.0.upgrade().map(PipelineCell)
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

    // ========================================================================
    // Owner-local traversal — full `run_frame` through a `PipelineCell`
    // checkout (miri coverage: `cargo xtask miri`, whose `pipeline::owner`
    // filter includes this module).
    // ========================================================================
    //
    // Everything above this point exercises the checkout mechanism in
    // isolation (never touches a real `RenderTree`). This test drives the
    // exact production pattern `BuildOwner::run_frame_with_layout_builders`
    // uses (`flui-view/src/owner/layout_builder.rs`): `with_mut` ->
    // `mem::take` the owner out -> `run_frame` (layout -> compositing ->
    // paint -> semantics, all real `NodePtr` reborrows inside
    // `layout_dirty_root`) -> write the returned owner back -- so miri
    // interprets the whole frame through the checkout seam, not just the
    // raw `layout_dirty_root` call the older `subtree_arena` walks use.
}
