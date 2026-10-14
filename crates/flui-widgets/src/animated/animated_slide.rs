//! [`AnimatedSlide`] — translates its child by a fraction of its own size,
//! animating to each new offset while keeping the velocity it had.

use std::rc::Rc;

use flui_animation::{MotionSpec, ProxyAnimation};
use flui_view::prelude::{BuildContext, LifecycleContext, StatefulView};
use flui_view::{BoxedView, IntoView, ViewExt, ViewState};

use flui_objects::TranslationFraction;

use crate::SlideTransition;
use crate::animated::implicitly_animated::{ImplicitMotion, MotionBuilder};

/// Translates its child by `offset`, a fraction of the child's own size,
/// animating to each new offset it is given.
///
/// The first build shows the child at `offset` with no motion; a later build
/// with a different offset moves from the offset currently shown. The slide
/// is paint-only and hit-testing follows it. A non-finite first offset is
/// shown as zero; a later non-finite offset is refused and the current
/// motion continues.
///
/// ```
/// use flui_widgets::{AnimatedSlide, MotionBuilder, SizedBox};
/// use flui_objects::TranslationFraction;
/// use std::time::Duration;
///
/// let hidden = AnimatedSlide::new(TranslationFraction::new(0.0, 1.0), SizedBox::new(40.0, 40.0))
///     .duration(Duration::from_millis(250));
/// # let _ = hidden;
/// ```
#[derive(Clone, StatefulView)]
pub struct AnimatedSlide {
    offset: TranslationFraction,
    motion: MotionSpec,
    child: BoxedView,
}

impl AnimatedSlide {
    /// Translate `child` by `offset` with the default motion.
    pub fn new(offset: TranslationFraction, child: impl IntoView) -> Self {
        Self {
            offset,
            motion: MotionSpec::default(),
            child: child.into_view().boxed(),
        }
    }
}

impl MotionBuilder for AnimatedSlide {
    fn motion_mut(&mut self) -> &mut MotionSpec {
        &mut self.motion
    }
}

impl std::fmt::Debug for AnimatedSlide {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimatedSlide")
            .field("offset", &self.offset)
            .field("motion", &self.motion)
            .finish_non_exhaustive()
    }
}

/// Owns the slide motion and the stable stream the transition observes.
#[derive(Debug)]
pub struct AnimatedSlideState {
    animation: ImplicitMotion<TranslationFraction>,
    proxy: ProxyAnimation<TranslationFraction>,
    child: BoxedView,
}

impl StatefulView for AnimatedSlide {
    type State = AnimatedSlideState;

    fn create_state(&self) -> Self::State {
        let animation = ImplicitMotion::new(
            "AnimatedSlide",
            self.offset,
            TranslationFraction::ZERO,
            &self.motion,
        );
        let proxy = ProxyAnimation::new(Rc::new(animation.stream()));
        AnimatedSlideState {
            animation,
            proxy,
            child: self.child.clone(),
        }
    }
}

impl ViewState<AnimatedSlide> for AnimatedSlideState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.animation.bind(ctx);
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.animation.bind(ctx);
    }

    fn build(&self, _view: &AnimatedSlide, _ctx: &dyn BuildContext) -> impl IntoView {
        SlideTransition::new(Rc::new(self.proxy.clone()), self.child.clone())
    }

    fn did_update_view(&mut self, _old_view: &AnimatedSlide, new_view: &AnimatedSlide) {
        self.child = new_view.child.clone();
        self.animation.retarget(new_view.offset, &new_view.motion);
    }

    fn dispose(&mut self) {
        self.animation.dispose();
    }
}
