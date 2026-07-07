//! Flow delegate for custom flow layout algorithms.
//!
//! [`FlowDelegate`] allows users to implement custom flow layout behavior
//! with custom constraints and painting transforms.

use std::{any::Any, fmt::Debug, sync::Arc};

use flui_foundation::Listenable;
use flui_tree::Variable;
use flui_types::{Matrix4, Size};

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
/// use flui_types::{BoxConstraints, Matrix4, Size};
///
/// #[derive(Debug)]
/// struct CircularFlowDelegate {
///     radius: f32,
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
///             let angle = 2.0 * std::f32::consts::PI * (i as f32) / (context.child_count() as f32);
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
pub trait FlowDelegate: Send + Sync + Debug {
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
    /// the same implicit purity assumption Flutter's own `RenderFlow` and
    /// [`Self::should_repaint`]/[`Self::should_relayout`] already rely on.
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
    /// `RenderFlow` marks itself needing paint — the FLUI equivalent of
    /// Flutter's `Flow(delegate:)` `repaint:` listenable, letting a flow
    /// driven by an [`Animation`] repaint without a widget rebuild.
    ///
    /// Implementations that return `Some` MUST return the *same* instance
    /// across calls, so the host can unsubscribe on detach / delegate swap.
    /// Defaults to `None`.
    ///
    /// [`Animation`]: https://api.flutter.dev/flutter/animation/Animation-class.html
    fn repaint(&self) -> Option<Arc<dyn Listenable>> {
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
/// paint assign to child N" for hit-test to read back later (Flutter's
/// `RenderFlow` gets away with this because `paint()` isn't `const` and
/// mutates `FlowParentData._transform`). Hit-test instead re-invokes
/// [`FlowDelegate::paint_children`] a second time against a `for_replay`
/// context: same recorded `(index -> transform)` data, no live [`PaintCx`]
/// to draw into.
pub struct FlowPaintingContext<'ctx, 'cx> {
    size: Size,
    child_sizes: &'ctx [Size],
    /// `Some` in paint mode (drives the real paint pipeline); `None`
    /// during a hit-test replay (recording only, nothing is drawn).
    live: Option<&'ctx mut PaintCx<'cx, Variable>>,
    /// Child indices in the order `paint_child` was called — Flutter's
    /// `_lastPaintOrder`. Hit-testing walks this in reverse (top-most
    /// painted first).
    paint_order: &'ctx mut Vec<usize>,
    /// Every child's most recently recorded transform, indexed by child
    /// index. Recorded in both modes so hit-test's replay pass produces
    /// the identical data paint's real pass would have.
    transforms: &'ctx mut Vec<Option<Matrix4>>,
    /// Per-child dup-paint guard (oracle `paintChild` asserts no child is
    /// painted twice in one `paintChildren` call).
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
    /// Always records: `index` into [`Self`]'s paint order (Flutter's
    /// `_lastPaintOrder`) and `transform` into the per-child transform
    /// table, both consumed later by `RenderFlow::hit_test`.
    ///
    /// # Arguments
    ///
    /// * `index` - The index of the child to paint
    /// * `transform` - The transform matrix to apply
    ///
    /// # Panics
    ///
    /// Panics if the index is out of bounds, or if this child was already
    /// painted earlier in the same `paint_children` call (oracle
    /// `paintChild`'s double-paint assert).
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

#[cfg(test)]
mod tests {
    use flui_types::geometry::px;

    use super::*;

    #[derive(Debug)]
    struct LinearFlowDelegate {
        spacing: f32,
    }

    impl FlowDelegate for LinearFlowDelegate {
        fn get_size(&self, constraints: BoxConstraints) -> Size {
            constraints.biggest()
        }

        fn get_constraints_for_child(
            &self,
            _index: usize,
            _constraints: BoxConstraints,
        ) -> BoxConstraints {
            BoxConstraints::loose(Size::new(px(100.0), px(50.0)))
        }

        fn paint_children(&self, context: &mut FlowPaintingContext<'_, '_>) {
            let mut x: f32 = 0.0;
            for i in 0..context.child_count() {
                let transform = Matrix4::translation(x, 0.0, 0.0);
                context.paint_child(i, transform);
                x += context.child_size(i).width.get() + self.spacing;
            }
        }

        fn should_relayout(&self, old_delegate: &dyn FlowDelegate) -> bool {
            if let Some(old) = old_delegate.as_any().downcast_ref::<Self>() {
                (self.spacing - old.spacing).abs() > f32::EPSILON
            } else {
                true
            }
        }

        fn should_repaint(&self, old_delegate: &dyn FlowDelegate) -> bool {
            self.should_relayout(old_delegate)
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    /// Bare-bones fixture for constructing a [`FlowPaintingContext`] without
    /// hand-writing the three parallel bookkeeping `Vec`s at every call site.
    struct ContextFixture {
        child_sizes: Vec<Size>,
        paint_order: Vec<usize>,
        transforms: Vec<Option<Matrix4>>,
        painted: Vec<bool>,
    }

    impl ContextFixture {
        fn new(child_sizes: Vec<Size>) -> Self {
            let n = child_sizes.len();
            Self {
                child_sizes,
                paint_order: Vec::with_capacity(n),
                transforms: vec![None; n],
                painted: vec![false; n],
            }
        }

        fn replay_context(&mut self, size: Size) -> FlowPaintingContext<'_, '_> {
            FlowPaintingContext::for_replay(
                size,
                &self.child_sizes,
                &mut self.paint_order,
                &mut self.transforms,
                &mut self.painted,
            )
        }
    }

    #[test]
    fn for_replay_records_the_real_transform_not_just_a_painted_flag() {
        let mut fixture = ContextFixture::new(vec![
            Size::new(px(50.0), px(30.0)),
            Size::new(px(60.0), px(40.0)),
            Size::new(px(70.0), px(50.0)),
        ]);
        let flow_size = Size::new(px(300.0), px(100.0));
        let t0 = Matrix4::translation(10.0, 0.0, 0.0);
        let t1 = Matrix4::translation(60.0, 0.0, 0.0);
        let t2 = Matrix4::translation(130.0, 0.0, 0.0);

        {
            let mut context = fixture.replay_context(flow_size);
            assert_eq!(context.size(), flow_size);
            assert_eq!(context.child_count(), 3);
            assert_eq!(context.child_size(0), Size::new(px(50.0), px(30.0)));
            assert!(!context.all_children_painted());

            context.paint_child(0, t0);
            context.paint_child(1, t1);
            assert!(!context.all_children_painted());
            context.paint_child(2, t2);
            assert!(context.all_children_painted());
        }

        assert_eq!(
            fixture.paint_order,
            vec![0, 1, 2],
            "paint order must record the exact call sequence"
        );
        assert_eq!(
            fixture.transforms,
            vec![Some(t0), Some(t1), Some(t2)],
            "the CONTEXT must carry the real matrix each child was painted with, \
             not merely a painted/not-painted flag"
        );
    }

    #[test]
    #[should_panic(expected = "called twice")]
    fn paint_child_twice_in_one_pass_panics() {
        let mut fixture = ContextFixture::new(vec![Size::new(px(10.0), px(10.0))]);
        let mut context = fixture.replay_context(Size::ZERO);
        context.paint_child(0, Matrix4::IDENTITY);
        context.paint_child(0, Matrix4::IDENTITY);
    }

    #[test]
    fn for_paint_forwards_the_transform_to_the_live_paint_cx() {
        use flui_types::Offset;

        use crate::context::{FragmentOp, FragmentRecorder};

        let mut fixture = ContextFixture::new(vec![Size::new(px(20.0), px(20.0))]);
        let transform = Matrix4::translation(5.0, 7.0, 0.0);

        let mut rec = FragmentRecorder::new(Offset::ZERO, 1.0);
        let mut paint_cx = PaintCx::<Variable>::new(&mut rec, 1, Size::new(px(100.0), px(100.0)));
        {
            let mut context = FlowPaintingContext::for_paint(
                &mut paint_cx,
                &fixture.child_sizes,
                &mut fixture.paint_order,
                &mut fixture.transforms,
                &mut fixture.painted,
            );
            assert_eq!(context.size(), Size::new(px(100.0), px(100.0)));
            context.paint_child(0, transform);
        }

        assert_eq!(fixture.paint_order, vec![0]);
        assert_eq!(fixture.transforms, vec![Some(transform)]);

        let frag = rec.finish();
        assert!(
            matches!(
                frag.ops.as_slice(),
                [
                    FragmentOp::PushTransform(m),
                    FragmentOp::Child { index: 0, .. },
                    FragmentOp::Pop,
                ] if **m == transform,
            ),
            "for_paint's paint_child must forward through PaintCx::with_transform \
             (PushTransform(matrix) / Child / Pop), not just record bookkeeping; got {:?}",
            frag.ops,
        );
    }

    #[test]
    fn test_linear_flow_delegate() {
        let delegate = LinearFlowDelegate { spacing: 10.0 };
        let constraints = BoxConstraints::new(px(0.0), px(500.0), px(0.0), px(200.0));

        let size = delegate.get_size(constraints);
        assert_eq!(size, Size::new(px(500.0), px(200.0)));

        let child_constraints = delegate.get_constraints_for_child(0, constraints);
        assert_eq!(child_constraints.max_width, px(100.0));
        assert_eq!(child_constraints.max_height, px(50.0));
    }

    #[test]
    fn test_should_relayout() {
        let delegate1 = LinearFlowDelegate { spacing: 10.0 };
        let delegate2 = LinearFlowDelegate { spacing: 10.0 };
        let delegate3 = LinearFlowDelegate { spacing: 20.0 };

        assert!(!delegate1.should_relayout(&delegate2));
        assert!(delegate1.should_relayout(&delegate3));
    }
}
