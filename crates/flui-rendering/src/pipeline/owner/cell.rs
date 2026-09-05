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
    /// let cell = PipelineCell::new(PipelineOwner::new());
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
    /// let cell = PipelineCell::new(PipelineOwner::new());
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

    // ========================================================================
    // Owner-local traversal — full `run_frame` through a `PipelineCell`
    // checkout (miri coverage: root `AGENTS.md`'s miri-scope line, justfile
    // `miri` recipe).
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

    use flui_tree::Exact;
    use flui_types::{Color, Point, Rect, Size, geometry::px};

    use crate::{
        constraints::BoxConstraints,
        context::{BoxLayoutContext, PaintCx},
        parent_data::BoxParentData,
        pipeline::Paint,
        protocol::BoxProtocol,
        traits::{RenderBox, RenderObject},
    };

    /// Leaf that reports a fixed size and paints a solid rect -- exercises
    /// both the layout and paint phases of `run_frame` for a real `NodePtr`.
    #[derive(Debug)]
    struct FrameLeaf {
        size: Size,
    }

    impl flui_foundation::Diagnosticable for FrameLeaf {}

    impl RenderBox for FrameLeaf {
        type Arity = flui_tree::Leaf;
        type ParentData = BoxParentData;

        fn perform_layout(
            &mut self,
            ctx: &mut BoxLayoutContext<'_, Self::Arity, Self::ParentData>,
        ) -> Size {
            ctx.constraints().constrain(self.size)
        }

        fn paint(&self, ctx: &mut PaintCx<'_, Self::Arity>) {
            let rect = Rect::from_origin_size(Point::ZERO, ctx.size());
            ctx.canvas().draw_rect(
                rect,
                &Paint::fill(Color::from_rgba_f32_array([1.0, 0.0, 0.0, 1.0])),
            );
        }
    }

    /// Two-child parent: lays out and paints both children in order. Root +
    /// 2 leaves = 3 real `NodePtr`s, kept small so the miri wall-time for
    /// this test stays in budget.
    #[derive(Debug)]
    struct FrameParent;

    impl flui_foundation::Diagnosticable for FrameParent {}

    impl RenderBox for FrameParent {
        type Arity = Exact<2>;
        type ParentData = BoxParentData;

        fn perform_layout(
            &mut self,
            ctx: &mut BoxLayoutContext<'_, Self::Arity, Self::ParentData>,
        ) -> Size {
            let constraints = *ctx.constraints();
            ctx.layout_child(0, constraints);
            ctx.layout_child(1, constraints);
            constraints.biggest()
        }

        fn paint(&self, ctx: &mut PaintCx<'_, Self::Arity>) {
            ctx.paint_children_in_order();
        }
    }

    #[test]
    fn real_node_ptr_frame_runs_through_pipeline_cell_checkout() {
        let mut owner = PipelineOwner::new();
        let root = owner.insert(Box::new(FrameParent) as Box<dyn RenderObject<BoxProtocol>>);
        owner
            .insert_child_render_object(
                root,
                Box::new(FrameLeaf {
                    size: Size::new(px(10.0), px(10.0)),
                }),
            )
            .expect("first leaf child insert");
        owner
            .insert_child_render_object(
                root,
                Box::new(FrameLeaf {
                    size: Size::new(px(10.0), px(10.0)),
                }),
            )
            .expect("second leaf child insert");
        owner.set_root_id(Some(root));
        owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(20.0), px(20.0)))));

        let cell = PipelineCell::new(owner);

        // The exact `with_mut { mem::take -> run_frame -> write back }`
        // pattern `layout_builder.rs` drives in production.
        let frame_result = cell.with_mut(|pipeline_owner| {
            let (returned, result) = std::mem::take(pipeline_owner).run_frame();
            *pipeline_owner = returned;
            result
        });

        let layer_tree = frame_result
            .expect("owner-local traversal: a full run_frame over a real 3-node tree must succeed under miri");
        assert!(
            layer_tree.is_some(),
            "a successful frame with a live root must produce a layer tree"
        );
        assert!(
            cell.is_free(),
            "the cell must be free again once with_mut returns"
        );
    }
}
