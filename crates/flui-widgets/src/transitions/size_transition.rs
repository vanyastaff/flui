//! [`SizeTransition`] — grows or shrinks its child's extent along one axis
//! from an [`Animation<f64>`], clipping what falls outside.

use std::rc::Rc;

use flui_animation::Animation;
use flui_foundation::geometry::Axis;
use flui_objects::AlignMotion;
use flui_painting::AlignmentDirectional;
use flui_painting::typography::TextDirection;
use flui_view::prelude::{BuildContext, StatefulView};
use flui_view::{BoxedView, IntoView, ViewExt, ViewState};

use super::align_view::AnimatedAlignView;
use super::proxy_slot::ProxySlot;
use crate::{ClipRect, Directionality};

/// Sizes its child's extent along [`axis`](Self::axis) to the child's own
/// extent times an [`Animation<f64>`] (the size factor), clipping the rest.
///
/// Backed by `RenderAnimatedAlign` under a `ClipRect`; the render object
/// listens to `size_factor` itself, so a tick relays out this node without
/// rebuilding the element tree. `1.0` is the child's full extent, `0.0`
/// collapses the axis; an overshoot below zero collapses it too.
///
/// [`axis_alignment`](Self::axis_alignment) places the child along the axis
/// while it is clipped: `-1.0` keeps its start edge, `0.0` (the default) its
/// centre, `1.0` its end edge. On the cross axis the box fills its
/// constraints, as an align does, with the child at the start edge
/// (mirrored under the ambient right-to-left [`Directionality`]);
/// [`fixed_cross_axis_size_factor`](Self::fixed_cross_axis_size_factor)
/// sizes the cross axis to the child's extent times that factor instead.
///
/// ```rust,ignore
/// let controller = AnimationController::builder(Duration::from_millis(200)).build();
/// let reveal = SizeTransition::new(Rc::new(controller.clone()), details());
/// controller.forward(); // the details unroll downward each frame
/// ```
#[derive(Clone, StatefulView)]
pub struct SizeTransition {
    size_factor: Rc<dyn Animation<f64>>,
    axis: Axis,
    axis_alignment: f64,
    fixed_cross_axis_size_factor: Option<f64>,
    child: BoxedView,
}

impl SizeTransition {
    /// A vertical size transition driven by `size_factor`, sizing `child`.
    pub fn new(size_factor: Rc<dyn Animation<f64>>, child: impl IntoView) -> Self {
        Self {
            size_factor,
            axis: Axis::Vertical,
            axis_alignment: 0.0,
            fixed_cross_axis_size_factor: None,
            child: child.into_view().boxed(),
        }
    }

    /// Sets the axis the factor sizes (default [`Axis::Vertical`]).
    #[must_use]
    pub fn axis(mut self, axis: Axis) -> Self {
        self.axis = axis;
        self
    }

    /// Sets where the child sits along the axis while clipped, `-1.0`
    /// (start) to `1.0` (end); default `0.0`.
    #[must_use]
    pub fn axis_alignment(mut self, axis_alignment: f64) -> Self {
        self.axis_alignment = axis_alignment;
        self
    }

    /// Sizes the cross axis to the child's extent times `factor` instead of
    /// filling the constraints.
    #[must_use]
    pub fn fixed_cross_axis_size_factor(mut self, factor: f64) -> Self {
        self.fixed_cross_axis_size_factor = Some(factor);
        self
    }
}

impl std::fmt::Debug for SizeTransition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SizeTransition")
            .field("size_factor", &self.size_factor.value())
            .field("axis", &self.axis)
            .field("axis_alignment", &self.axis_alignment)
            .field(
                "fixed_cross_axis_size_factor",
                &self.fixed_cross_axis_size_factor,
            )
            .finish_non_exhaustive()
    }
}

/// State for [`SizeTransition`]: the proxy the render object listens to,
/// kept across rebuilds so a new `size_factor` retargets it in place.
#[derive(Debug)]
pub struct SizeTransitionState {
    size_factor: ProxySlot<f64>,
}

impl ViewState<SizeTransition> for SizeTransitionState {
    fn build(&self, view: &SizeTransition, ctx: &dyn BuildContext) -> impl IntoView {
        let ltr = Directionality::maybe_of(ctx).unwrap_or(TextDirection::Ltr) == TextDirection::Ltr;
        let (alignment, width_factor, height_factor) = match view.axis {
            Axis::Vertical => (
                AlignmentDirectional::new(-1.0, view.axis_alignment),
                view.fixed_cross_axis_size_factor,
                None,
            ),
            Axis::Horizontal => (
                AlignmentDirectional::new(view.axis_alignment, -1.0),
                None,
                view.fixed_cross_axis_size_factor,
            ),
        };
        ClipRect::new().child(AnimatedAlignView {
            motion: AlignMotion::Factor {
                axis: view.axis,
                factor: self.size_factor.proxy(),
            },
            alignment: alignment.resolve(ltr),
            width_factor,
            height_factor,
            child: view.child.clone(),
        })
    }

    fn did_update_view(&mut self, _old_view: &SizeTransition, new_view: &SizeTransition) {
        self.size_factor.retarget(&new_view.size_factor);
    }
}

impl StatefulView for SizeTransition {
    type State = SizeTransitionState;

    fn create_state(&self) -> Self::State {
        SizeTransitionState {
            size_factor: ProxySlot::new(&self.size_factor),
        }
    }
}
