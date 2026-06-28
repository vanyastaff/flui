//! [`FadeTransition`] — animates its child's opacity from an [`Animation<f32>`].

use std::sync::Arc;

use flui_animation::Animation;
use flui_foundation::Listenable;
use flui_view::prelude::BuildContext;
use flui_view::{
    AnimatedView, BoxedView, IntoView, StatefulView, ViewExt, ViewState, impl_animated_view,
};

use crate::Opacity;

/// Fades its child in and out as an [`Animation<f32>`] (the opacity) changes.
///
/// Flutter parity: `widgets/transitions.dart` `FadeTransition` — an
/// `AnimatedWidget` wrapping `Opacity`. Each tick of `opacity` rebuilds the
/// transition (via the listenable subscription) and re-reads
/// [`Animation::value`] into an [`Opacity`]. `0.0` is fully transparent, `1.0`
/// fully opaque; the child is always laid out (only its painting fades).
///
/// ```rust,ignore
/// let controller = AnimationController::new(Duration::from_millis(300), scheduler);
/// let fade = FadeTransition::new(Arc::new(controller), Text::new("hi"));
/// controller.forward(); // each frame re-reads the opacity into the child
/// ```
#[derive(Clone)]
pub struct FadeTransition {
    opacity: Arc<dyn Animation<f32>>,
    child: BoxedView,
}

impl FadeTransition {
    /// A fade driven by `opacity`, fading `child`.
    pub fn new(opacity: Arc<dyn Animation<f32>>, child: impl IntoView) -> Self {
        Self {
            opacity,
            child: child.into_view().boxed(),
        }
    }
}

impl std::fmt::Debug for FadeTransition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FadeTransition")
            .field("opacity", &self.opacity.value())
            .finish_non_exhaustive()
    }
}

/// State for [`FadeTransition`]. Stateless beyond the listenable subscription
/// that [`AnimatedView`] manages — the opacity lives on the animation, not here.
#[derive(Debug)]
pub struct FadeTransitionState;

impl ViewState<FadeTransition> for FadeTransitionState {
    fn build(&self, view: &FadeTransition, _ctx: &dyn BuildContext) -> impl IntoView {
        Opacity::new(view.opacity.value()).child(view.child.clone())
    }
}

impl StatefulView for FadeTransition {
    type State = FadeTransitionState;

    fn create_state(&self) -> Self::State {
        FadeTransitionState
    }
}

impl AnimatedView for FadeTransition {
    fn listenable(&self) -> Arc<dyn Listenable> {
        // `Animation<f32>: Listenable`; upcast the trait object so the element
        // subscribes to the same notifier the animation ticks.
        self.opacity.clone() as Arc<dyn Listenable>
    }
}

impl_animated_view!(FadeTransition);
