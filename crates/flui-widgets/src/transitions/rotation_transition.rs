//! [`RotationTransition`] — animates its child's rotation from an
//! [`Animation<f64>`] of turns.

use std::rc::Rc;

use flui_animation::{Animation, ProxyAnimation};
use flui_objects::TransformMotion;
use flui_painting::typography::TextDirection;
use flui_view::prelude::{BuildContext, StatefulView};
use flui_view::{BoxedView, IntoView, ViewExt, ViewState};

use super::transform_view::AnimatedTransformView;

/// Rotates its child about its center as an [`Animation<f64>`] of *turns*
/// changes (`1.0` turn = a full 360° revolution).
///
/// Backed by `RenderAnimatedTransform`, which listens to `turns` itself: a tick
/// patches the node's transform layer without rebuilding the element tree.
/// Rotation is paint-only; the child is laid out as if unrotated.
#[derive(Clone, StatefulView)]
pub struct RotationTransition {
    turns: std::rc::Rc<dyn Animation<f64>>,
    child: BoxedView,
}

impl RotationTransition {
    /// A rotation driven by `turns` (1.0 = a full revolution), rotating `child`.
    pub fn new(turns: std::rc::Rc<dyn Animation<f64>>, child: impl IntoView) -> Self {
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

/// State for [`RotationTransition`]: the proxy the render object listens to,
/// kept across rebuilds so a new `turns` retargets it in place.
#[derive(Debug)]
pub struct RotationTransitionState {
    proxy: ProxyAnimation<f64>,
    turns: std::rc::Rc<dyn Animation<f64>>,
}

impl ViewState<RotationTransition> for RotationTransitionState {
    fn build(&self, view: &RotationTransition, _ctx: &dyn BuildContext) -> impl IntoView {
        AnimatedTransformView {
            motion: TransformMotion::Rotation {
                turns: self.proxy.clone(),
            },
            transform_hit_tests: true,
            text_direction: TextDirection::Ltr,
            child: view.child.clone(),
        }
    }

    fn did_update_view(&mut self, _old_view: &RotationTransition, new_view: &RotationTransition) {
        if !Rc::ptr_eq(&self.turns, &new_view.turns) {
            self.turns = Rc::clone(&new_view.turns);
            self.proxy.set_parent(Rc::clone(&new_view.turns));
        }
    }
}

impl StatefulView for RotationTransition {
    type State = RotationTransitionState;

    fn create_state(&self) -> Self::State {
        RotationTransitionState {
            proxy: ProxyAnimation::new(Rc::clone(&self.turns)),
            turns: Rc::clone(&self.turns),
        }
    }
}
