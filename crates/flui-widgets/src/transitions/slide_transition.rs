//! [`SlideTransition`] — animates its child's position as a fraction of its
//! own size from an [`Animation<TranslationFraction>`].

use std::sync::Arc;

use flui_animation::{Animation, ProxyAnimation};
use flui_objects::{TransformMotion, TranslationFraction};
use flui_painting::typography::TextDirection;
use flui_view::prelude::{BuildContext, StatefulView};
use flui_view::{BoxedView, IntoView, ViewExt, ViewState};

use super::transform_view::AnimatedTransformView;

/// Animates its child's position by a fraction of the child's own size, as a
/// [`TranslationFraction`] read off an [`Animation`].
///
/// Backed by `RenderAnimatedTransform`, which listens to `position` itself: a
/// tick patches the node's transform layer without rebuilding the element
/// tree or repainting the child.
///
/// The animated value is a dedicated [`TranslationFraction`], not an `Offset`:
/// an `Offset` is in pixels, so carrying a size-relative fraction in one would
/// be a unit mismatch (see `flui_objects::TranslationFraction`'s module doc).
///
/// `position.value() == TranslationFraction { dx: 0.0, dy: 0.0 }` paints the
/// child at its normal location; `{ dx: 1.0, dy: 0.0 }` shifts it fully off
/// to the right, one child-width away.
///
/// # `text_direction`
///
/// `SlideTransition` does **not** read the ambient `Directionality`;
/// `text_direction` is a plain, caller-supplied setting. Unset, `dx` is in
/// canvas coordinates (positive moves the child right), and
/// `TextDirection::Rtl` flips `dx`'s sign so positive values move the child
/// toward the reading-direction start instead.
///
/// ```rust,ignore
/// let controller = AnimationController::without_ticker(Duration::from_millis(300));
/// let tween = Tween::new(TranslationFraction::new(-1.0, 0.0), TranslationFraction::ZERO);
/// let position = Arc::new(tween.animate(Arc::new(controller.clone()) as Arc<dyn Animation<f64>>));
/// let slide = SlideTransition::new(position, Text::new("hi"));
/// controller.forward(); // each frame moves the child's transform layer
/// ```
#[derive(Clone, StatefulView)]
pub struct SlideTransition {
    position: Arc<dyn Animation<TranslationFraction>>,
    transform_hit_tests: bool,
    text_direction: Option<TextDirection>,
    child: BoxedView,
}

impl SlideTransition {
    /// A slide driven by `position`, translating `child`.
    pub fn new(position: Arc<dyn Animation<TranslationFraction>>, child: impl IntoView) -> Self {
        Self {
            position,
            transform_hit_tests: true,
            text_direction: None,
            child: child.into_view().boxed(),
        }
    }

    /// Sets whether hit-testing follows the painted translation (default
    /// `true`).
    #[must_use]
    pub fn transform_hit_tests(mut self, transform_hit_tests: bool) -> Self {
        self.transform_hit_tests = transform_hit_tests;
        self
    }

    /// Sets the reading direction `dx` is interpreted against — see the type
    /// doc's `text_direction` section.
    #[must_use]
    pub fn text_direction(mut self, text_direction: TextDirection) -> Self {
        self.text_direction = Some(text_direction);
        self
    }

    fn resolved_direction(&self) -> TextDirection {
        self.text_direction.unwrap_or(TextDirection::Ltr)
    }
}

impl std::fmt::Debug for SlideTransition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SlideTransition")
            .field("position", &self.position.value())
            .field("transform_hit_tests", &self.transform_hit_tests)
            .field("text_direction", &self.text_direction)
            .finish_non_exhaustive()
    }
}

/// State for [`SlideTransition`]: the proxy the render object listens to,
/// kept across rebuilds so a new `position` retargets it in place.
#[derive(Debug)]
pub struct SlideTransitionState {
    proxy: ProxyAnimation<TranslationFraction>,
    position: Arc<dyn Animation<TranslationFraction>>,
}

impl ViewState<SlideTransition> for SlideTransitionState {
    fn build(&self, view: &SlideTransition, _ctx: &dyn BuildContext) -> impl IntoView {
        AnimatedTransformView {
            motion: TransformMotion::Slide {
                offset: self.proxy.clone(),
                text_direction: view.resolved_direction(),
            },
            transform_hit_tests: view.transform_hit_tests,
            text_direction: view.resolved_direction(),
            child: view.child.clone(),
        }
    }

    fn did_update_view(&mut self, _old_view: &SlideTransition, new_view: &SlideTransition) {
        if !Arc::ptr_eq(&self.position, &new_view.position) {
            self.position = Arc::clone(&new_view.position);
            self.proxy.set_parent(Arc::clone(&new_view.position));
        }
    }
}

impl StatefulView for SlideTransition {
    type State = SlideTransitionState;

    fn create_state(&self) -> Self::State {
        SlideTransitionState {
            proxy: ProxyAnimation::new(Arc::clone(&self.position)),
            position: Arc::clone(&self.position),
        }
    }
}
