//! [`ScaleTransition`] — animates its child's scale from an [`Animation<f32>`].

use std::sync::Arc;

use flui_animation::Animation;
use flui_foundation::Listenable;
use flui_view::prelude::BuildContext;
use flui_view::{
    AnimatedView, BoxedView, IntoView, StatefulView, ViewExt, ViewState, impl_animated_view,
};

use crate::Transform;

/// Scales its child about its center as an [`Animation<f32>`] (the scale factor)
/// changes.
///
/// Flutter parity: `widgets/transitions.dart` `ScaleTransition` — an
/// `AnimatedWidget` wrapping a center-aligned `Transform.scale`. `1.0` is the
/// child's natural size, `0.0` collapses it to a point. Scaling is paint-only;
/// the child is laid out as if untransformed.
#[derive(Clone)]
pub struct ScaleTransition {
    scale: Arc<dyn Animation<f32>>,
    child: BoxedView,
}

impl ScaleTransition {
    /// A scale driven by `scale`, transforming `child`.
    pub fn new(scale: Arc<dyn Animation<f32>>, child: impl IntoView) -> Self {
        Self {
            scale,
            child: child.into_view().boxed(),
        }
    }
}

impl std::fmt::Debug for ScaleTransition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScaleTransition")
            .field("scale", &self.scale.value())
            .finish_non_exhaustive()
    }
}

/// State for [`ScaleTransition`] — the scale lives on the animation, not here.
#[derive(Debug)]
pub struct ScaleTransitionState;

impl ViewState<ScaleTransition> for ScaleTransitionState {
    fn build(&self, view: &ScaleTransition, _ctx: &dyn BuildContext) -> impl IntoView {
        let scale = view.scale.value();
        // `Transform` applies the matrix about its alignment, which defaults to
        // the center — Flutter's `ScaleTransition` center-scale.
        Transform::scale(scale, scale).child(view.child.clone())
    }
}

impl StatefulView for ScaleTransition {
    type State = ScaleTransitionState;

    fn create_state(&self) -> Self::State {
        ScaleTransitionState
    }
}

impl AnimatedView for ScaleTransition {
    fn listenable(&self) -> Arc<dyn Listenable> {
        self.scale.clone() as Arc<dyn Listenable>
    }
}

impl_animated_view!(ScaleTransition);
