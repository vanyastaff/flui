//! Flow delegate for custom flow layout algorithms.
//!
//! [`FlowDelegate`] allows users to implement custom flow layout behavior
//! with custom constraints and painting transforms.

use std::{any::Any, fmt::Debug};

use flui_foundation::Listenable;
use flui_foundation::Variable;
use flui_foundation::geometry::{Matrix4, Size};

use crate::{constraints::BoxConstraints, context::PaintCx};

/// A delegate that provides custom flow layout behavior.
///
/// Flow layout is a powerful layout algorithm that allows positioning
/// children with arbitrary transforms. Unlike other layout delegates,
/// flow delegates can also control painting with custom transforms.
///
/// # Example
///
/// ```ignore
/// use flui_rendering::delegates::{FlowDelegate, FlowPaintingContext};
/// use flui_rendering::constraints::BoxConstraints;
/// use flui_foundation::geometry::{Matrix4, Size};
///
/// #[derive(Debug)]
/// struct CircularFlowDelegate {
///     radius: f64,
/// }
///
/// impl FlowDelegate for CircularFlowDelegate {
///     fn get_size(&self, constraints: BoxConstraints) -> Size {
///         let diameter = self.radius * 2.0;
///         constraints.constrain(Size::new(diameter, diameter))
///     }
///
///     fn get_constraints_for_child(&self, _index: usize, _constraints: BoxConstraints) -> BoxConstraints {
///         BoxConstraints::loose(Size::new(100.0, 100.0))
///     }
///
///     fn paint_children(&self, context: &mut FlowPaintingContext<'_, '_>) {
///         let center_x = self.radius;
///         let center_y = self.radius;
///
///         for i in 0..context.child_count() {
///             let angle = 2.0 * std::f64::consts::PI * (i as f64) / (context.child_count() as f64);
///             let child_size = context.child_size(i);
///
///             let x = center_x + self.radius * angle.cos() - child_size.width / 2.0;
///             let y = center_y + self.radius * angle.sin() - child_size.height / 2.0;
///
///             let transform = Matrix4::from_translation(glam::vec3(x, y, 0.0));
///             context.paint_child(i, transform);
///         }
///     }
///
///     fn should_relayout(&self, old_delegate: &dyn FlowDelegate) -> bool {
///         if let Some(old) = old_delegate.as_any().downcast_ref::<Self>() {
///             self.radius != old.radius
///         } else {
///             true
///         }
///     }
///
///     fn should_repaint(&self, old_delegate: &dyn FlowDelegate) -> bool {
///         self.should_relayout(old_delegate)
///     }
/// }
/// ```
pub trait FlowDelegate: Debug {
    /// Get the size of the flow layout for the given constraints.
    ///
    /// # Arguments
    ///
    /// * `constraints` - The constraints from the parent
    ///
    /// # Returns
    ///
    /// The size of this render object.
    fn get_size(&self, constraints: BoxConstraints) -> Size;

    /// Get the constraints for a child at the given index.
    ///
    /// # Arguments
    ///
    /// * `index` - The index of the child
    /// * `constraints` - The constraints from the parent
    ///
    /// # Returns
    ///
    /// The constraints to pass to the child.
    fn get_constraints_for_child(
        &self,
        index: usize,
        constraints: BoxConstraints,
    ) -> BoxConstraints;

    /// Paint children with custom transforms.
    ///
    /// Use the context to paint each child with a specific transform matrix.
    ///
    /// Must be a pure function of `self` (plus each child's size, read via
    /// [`FlowPaintingContext::child_size`]): `RenderFlow` replays this call
    /// a second time, against a non-drawing context, to recover per-child
    /// transforms for hit testing (paint's `&self` gives it nowhere to
    /// cache them). A delegate that consults external mutable state or
    /// randomness here will make paint and hit-test silently disagree —
    /// the same implicit purity assumption [`Self::should_repaint`] and
    /// [`Self::should_relayout`] already rely on.
    ///
    /// # Arguments
    ///
    /// * `context` - The painting context providing child operations
    fn paint_children(&self, context: &mut FlowPaintingContext<'_, '_>);

    /// Whether to relayout when the delegate changes.
    ///
    /// # Arguments
    ///
    /// * `old_delegate` - The previous delegate
    ///
    /// # Returns
    ///
    /// `true` if layout should be recalculated, `false` otherwise.
    fn should_relayout(&self, old_delegate: &dyn FlowDelegate) -> bool;

    /// Whether to repaint when the delegate changes.
    ///
    /// # Arguments
    ///
    /// * `old_delegate` - The previous delegate
    ///
    /// # Returns
    ///
    /// `true` if painting should be redone, `false` otherwise.
    fn should_repaint(&self, old_delegate: &dyn FlowDelegate) -> bool;

    /// An optional repaint [`Listenable`]: when it notifies, the hosting
    /// `RenderFlow` marks itself needing paint, letting a flow driven by an
    /// animation repaint without a widget rebuild.
    ///
    /// Implementations that return `Some` MUST return the *same* instance
    /// across calls, so the host can unsubscribe on detach / delegate swap.
    /// Defaults to `None`.
    fn repaint(&self) -> Option<std::rc::Rc<dyn Listenable>> {
        None
    }

    /// Returns self as `Any` for downcasting.
    fn as_any(&self) -> &dyn Any;
}

/// Context for flow painting operations.
///
/// Carries a delegate's [`FlowDelegate::paint_children`] call through to
/// either a live [`PaintCx`] (real paint — [`Self::for_paint`]) or a
/// recording-only replay with nowhere to draw (hit-test — [`Self::for_replay`]).
/// `RenderFlow::paint` and `RenderFlow::hit_test` are both `&self`, so
/// there is nowhere on the render object to cache "what transform did
/// paint assign to child N" for hit-test to read back later. Hit-test
/// instead re-invokes
/// [`FlowDelegate::paint_children`] a second time against a `for_replay`
/// context: same recorded `(index -> transform)` data, no live [`PaintCx`]
/// to draw into.
pub struct FlowPaintingContext<'ctx, 'cx> {
    size: Size,
    child_sizes: &'ctx [Size],
    /// `Some` in paint mode (drives the real paint pipeline); `None`
    /// during a hit-test replay (recording only, nothing is drawn).
    live: Option<&'ctx mut PaintCx<'cx, Variable>>,
    /// Child indices in the order `paint_child` was called. Hit-testing walks
    /// this in reverse (top-most painted first).
    paint_order: &'ctx mut Vec<usize>,
    /// Every child's most recently recorded transform, indexed by child
    /// index. Recorded in both modes so hit-test's replay pass produces
    /// the identical data paint's real pass would have.
    transforms: &'ctx mut Vec<Option<Matrix4>>,
    /// Per-child dup-paint guard: no child may be painted twice in one
    /// `paint_children` call.
    painted: &'ctx mut Vec<bool>,
}

impl std::fmt::Debug for FlowPaintingContext<'_, '_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `live` holds a &mut PaintCx (the live recording canvas); report
        // the replayable recording state instead.
        f.debug_struct("FlowPaintingContext")
            .field("size", &self.size)
            .field("child_sizes", &self.child_sizes)
            .field("live", &self.live.is_some())
            .field("paint_order", &self.paint_order)
            .field("painted", &self.painted)
            .finish_non_exhaustive()
    }
}

impl<'ctx, 'cx> FlowPaintingContext<'ctx, 'cx> {
    /// Builds a paint-mode context that forwards each [`Self::paint_child`]
    /// call to the live `ctx` via [`PaintCx::with_transform`].
    ///
    /// Reads `ctx.size()` before moving `ctx` into `live` — ordering
    /// matters, since `size()` borrows `ctx` immutably and `live` then
    /// takes it by unique reference.
    pub fn for_paint(
        ctx: &'ctx mut PaintCx<'cx, Variable>,
        child_sizes: &'ctx [Size],
        paint_order: &'ctx mut Vec<usize>,
        transforms: &'ctx mut Vec<Option<Matrix4>>,
        painted: &'ctx mut Vec<bool>,
    ) -> Self {
        let size = ctx.size();
        Self {
            size,
            child_sizes,
            live: Some(ctx),
            paint_order,
            transforms,
            painted,
        }
    }

    /// Builds a replay-only context for hit-testing: records the same
    /// `(paint_order, transforms)` a real paint pass would have produced,
    /// but draws nothing (there is no live [`PaintCx`] to draw into).
    pub fn for_replay(
        size: Size,
        child_sizes: &'ctx [Size],
        paint_order: &'ctx mut Vec<usize>,
        transforms: &'ctx mut Vec<Option<Matrix4>>,
        painted: &'ctx mut Vec<bool>,
    ) -> Self {
        Self {
            size,
            child_sizes,
            live: None,
            paint_order,
            transforms,
            painted,
        }
    }

    /// The size of the flow layout.
    pub fn size(&self) -> Size {
        self.size
    }

    /// Returns the number of children.
    pub fn child_count(&self) -> usize {
        self.child_sizes.len()
    }

    /// Returns the size of the child at the given index.
    ///
    /// # Panics
    ///
    /// Panics if the index is out of bounds.
    pub fn child_size(&self, index: usize) -> Size {
        self.child_sizes[index]
    }

    /// Paints a child with the given transform.
    ///
    /// In paint mode ([`Self::for_paint`]), forwards to the live
    /// [`PaintCx`] via [`PaintCx::with_transform`] so the transform
    /// actually reaches the paint pipeline. In replay mode
    /// ([`Self::for_replay`]), only the bookkeeping below runs — nothing
    /// is drawn.
    ///
    /// Always records: `index` into [`Self`]'s paint order and `transform`
    /// into the per-child transform table, both consumed later by
    /// `RenderFlow::hit_test`.
    ///
    /// # Arguments
    ///
    /// * `index` - The index of the child to paint
    /// * `transform` - The transform matrix to apply
    ///
    /// # Panics
    ///
    /// Panics if the index is out of bounds, or if this child was already
    /// painted earlier in the same `paint_children` call (the
    /// double-paint assert).
    pub fn paint_child(&mut self, index: usize, transform: Matrix4) {
        assert!(index < self.child_sizes.len(), "Child index out of bounds");
        assert!(
            !self.painted[index],
            "paint_child called twice for child {index} in one paint_children pass"
        );
        self.painted[index] = true;
        self.paint_order.push(index);
        self.transforms[index] = Some(transform);
        if let Some(ctx) = self.live.as_deref_mut() {
            ctx.with_transform(transform, |ctx| ctx.paint_child(index));
        }
    }

    /// Returns whether all children have been painted.
    pub fn all_children_painted(&self) -> bool {
        self.painted.iter().all(|&p| p)
    }
}
