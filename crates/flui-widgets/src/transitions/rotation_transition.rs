//! [`RotationTransition`] — animates its child's rotation from an
//! [`Animation<f32>`] of turns.

use std::f32::consts::TAU;
use std::sync::Arc;

use flui_animation::Animation;
use flui_foundation::Listenable;
use flui_view::prelude::BuildContext;
use flui_view::{
    AnimatedView, BoxedView, IntoView, StatefulView, ViewExt, ViewState, impl_animated_view,
};

use crate::Transform;

/// Rotates its child about its center as an [`Animation<f32>`] of *turns*
/// changes (`1.0` turn = a full 360° revolution).
///
/// Flutter parity: `widgets/transitions.dart` `RotationTransition` — an
/// `AnimatedWidget` wrapping a center-aligned `Transform.rotate`, where the
/// animation value is measured in turns. Rotation is paint-only; the child is
/// laid out as if unrotated.
#[derive(Clone)]
pub struct RotationTransition {
    turns: Arc<dyn Animation<f32>>,
    child: BoxedView,
}

impl RotationTransition {
    /// A rotation driven by `turns` (1.0 = a full revolution), rotating `child`.
    pub fn new(turns: Arc<dyn Animation<f32>>, child: impl IntoView) -> Self {
        Self {
            turns,
            child: child.into_view().boxed(),
        }
    }
}

impl std::fmt::Debug for RotationTransition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RotationTransition")
            .field("turns", &self.turns.value())
            .finish_non_exhaustive()
    }
}

/// State for [`RotationTransition`] — the angle lives on the animation.
#[derive(Debug)]
pub struct RotationTransitionState;

impl ViewState<RotationTransition> for RotationTransitionState {
    fn build(&self, view: &RotationTransition, _ctx: &dyn BuildContext) -> impl IntoView {
        // Turns → radians; `Transform` rotates about its center by default.
        Transform::rotation(view.turns.value() * TAU).child(view.child.clone())
    }
}

impl StatefulView for RotationTransition {
    type State = RotationTransitionState;

    fn create_state(&self) -> Self::State {
        RotationTransitionState
    }
}

impl AnimatedView for RotationTransition {
    fn listenable(&self) -> Arc<dyn Listenable> {
        self.turns.clone() as Arc<dyn Listenable>
    }
}

impl_animated_view!(RotationTransition);
