//! [`ScaleTransition`] — animates its child's scale from an [`Animation<f64>`].

use flui_view::LifecycleContext;
use std::rc::Rc;

use flui_animation::{Animation, ProxyAnimation};
use flui_objects::TransformMotion;
use flui_painting::typography::TextDirection;
use flui_view::prelude::{BuildContext, StatefulView};
use flui_view::{Child, IntoView, ViewState};

use super::transform_view::AnimatedTransformView;

/// Scales its child about its center as an [`Animation<f64>`] (the scale factor)
/// changes.
///
/// Backed by `RenderAnimatedTransform`, which listens to `scale` itself: a tick
/// patches the node's transform layer without rebuilding the element tree.
/// `1.0` is the child's natural size; `0.0` collapses it — nothing is painted
/// and nothing is hit. Scaling is paint-only; the child is laid out as if
/// untransformed.
#[derive(Clone, StatefulView)]
pub struct ScaleTransition {
    scale: std::rc::Rc<dyn Animation<f64>>,
    child: Child,
}

impl ScaleTransition {
    /// A scale driven by `scale`, with no child yet.
    pub fn new(scale: std::rc::Rc<dyn Animation<f64>>) -> Self {
        Self {
            scale,
            child: Child::empty(),
        }
    }

    /// Sets the child the transition animates.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }

    /// Hands over an optional child as it is, for a widget that wraps this
    /// transition and keeps its own child slot.
    pub(crate) fn child_slot(mut self, child: Child) -> Self {
        self.child = child;
        self
    }
}

impl std::fmt::Debug for ScaleTransition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScaleTransition")
            .field("scale", &self.scale.value())
            .finish_non_exhaustive()
    }
}

/// State for [`ScaleTransition`]: the proxy the render object listens to,
/// kept across rebuilds so a new `scale` retargets it in place.
#[derive(Debug)]
pub struct ScaleTransitionState {
    proxy: ProxyAnimation<f64>,
    scale: std::rc::Rc<dyn Animation<f64>>,
}

impl ViewState<ScaleTransition> for ScaleTransitionState {
    fn build(&self, view: &ScaleTransition, _ctx: &dyn BuildContext) -> impl IntoView {
        AnimatedTransformView {
            motion: TransformMotion::Scale {
                scale: self.proxy.clone(),
            },
            transform_hit_tests: true,
            text_direction: TextDirection::Ltr,
            child: view.child.clone(),
        }
    }

    fn did_update_view(&mut self, _old_view: &ScaleTransition, new_view: &ScaleTransition) {
        if !Rc::ptr_eq(&self.scale, &new_view.scale) {
            self.scale = Rc::clone(&new_view.scale);
            self.proxy.set_parent(Rc::clone(&new_view.scale));
        }
    }
}

impl StatefulView for ScaleTransition {
    type State = ScaleTransitionState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        ScaleTransitionState {
            proxy: ProxyAnimation::new(Rc::clone(&self.scale)),
            scale: Rc::clone(&self.scale),
        }
    }
}
