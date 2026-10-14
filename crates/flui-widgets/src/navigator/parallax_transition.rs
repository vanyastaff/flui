//! [`ParallaxPageTransition`] — the iOS parallax slide, shared by every package
//! that ships it (ADR-0221 D4).
//!
//! The entering page slides in from the trailing edge while the page it covers
//! slides a third of its width toward the leading edge. `flui-cupertino`'s page
//! route and `flui-material`'s Apple-platform default both use this one copy.
//!
//! # Curves
//!
//! The **primary** position (this page sliding in or out) is curved with
//! `curves::FAST_EASE_IN_TO_SLOW_EASE_OUT`, reverse-curved with its
//! `.flipped()`. The **secondary** position (this page being covered by the
//! next) is curved with `curves::LINEAR_TO_EASE_OUT` forward and
//! `curves::EASE_IN_TO_LINEAR` in reverse. Both tween a [`TranslationFraction`]
//! through [`SlideTransition`].
//!
//! While a back gesture drives the route (`NavigatorHandle::user_gesture_in_progress`),
//! both curves are skipped and the tweens follow the raw animations, so the
//! page tracks the finger.
//!
//! # Not modelled
//!
//! The leading-edge shadow (no `DecoratedBoxTransition` yet) and the
//! fullscreen-dialog bottom-up variant (no `fullscreen_dialog` flag on
//! `PageRoute`).

use std::rc::Rc;
use std::time::Duration;

use flui_animation::{
    Animatable, Animation, ArcCurve, Curve, CurvedAnimation, MotionPolicy, Tween, curves,
};
use flui_objects::TranslationFraction;
use flui_view::{BoxedView, BuildContext, ViewExt};

use super::navigator::NavigatorHandle;
use super::overlay_route::RouteAnimation;
use super::page_transitions::PageTransitionBuilder;
use crate::{Directionality, MediaQuery, SlideTransition};

/// The slide's duration.
const DURATION: Duration = Duration::from_millis(500);

/// Off screen at the trailing edge → on screen.
fn trailing_to_middle() -> Tween<TranslationFraction> {
    Tween::new(
        TranslationFraction::new(1.0, 0.0),
        TranslationFraction::ZERO,
    )
}

/// On screen → a third off the leading edge: the parallax a covering page
/// applies to this one.
fn middle_to_leading() -> Tween<TranslationFraction> {
    Tween::new(
        TranslationFraction::ZERO,
        TranslationFraction::new(-1.0 / 3.0, 0.0),
    )
}

/// The iOS parallax page slide, 500 ms.
///
/// ```
/// use std::rc::Rc;
///
/// use flui_widgets::prelude::*;
/// use flui_widgets::{PageRoute, ParallaxPageTransition, Text};
///
/// let route = PageRoute::<()>::new(|_ctx, _primary, _secondary| {
///     Text::new("Details").into_view().boxed()
/// })
/// .transition_builder(Rc::new(ParallaxPageTransition::new()));
/// # let _ = route;
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct ParallaxPageTransition;

impl ParallaxPageTransition {
    /// The slide.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl PageTransitionBuilder for ParallaxPageTransition {
    fn duration(&self) -> Duration {
        DURATION
    }

    fn build(
        &self,
        ctx: &dyn BuildContext,
        primary: &RouteAnimation,
        secondary: &RouteAnimation,
        child: BoxedView,
    ) -> BoxedView {
        // The route settles on its first reduced-motion frame anyway; skipping
        // the slides keeps the settled page free of transform layers.
        if MediaQuery::motion_of(ctx) == Some(MotionPolicy::Reduce) {
            return child;
        }
        // Read fresh every build off the same ambient `NavigatorHandle` the
        // modal scope resolves its back-gesture detector through.
        let linear = NavigatorHandle::maybe_of(ctx)
            .is_some_and(|navigator| navigator.user_gesture_in_progress());

        let primary_position = if linear {
            trailing_to_middle().animate(Rc::clone(primary))
        } else {
            let curved = CurvedAnimation::new(
                Rc::clone(primary),
                ArcCurve::new(curves::FAST_EASE_IN_TO_SLOW_EASE_OUT),
            )
            .with_reverse_curve(ArcCurve::new(
                curves::FAST_EASE_IN_TO_SLOW_EASE_OUT.flipped(),
            ));
            trailing_to_middle().animate(Rc::new(curved) as Rc<dyn Animation<f64>>)
        };

        let secondary_position = if linear {
            middle_to_leading().animate(Rc::clone(secondary))
        } else {
            let curved = CurvedAnimation::new(Rc::clone(secondary), curves::LINEAR_TO_EASE_OUT)
                .with_reverse_curve(curves::EASE_IN_TO_LINEAR);
            middle_to_leading().animate(Rc::new(curved) as Rc<dyn Animation<f64>>)
        };

        let text_direction = Directionality::maybe_of(ctx);

        // The inner slide transforms hit tests (the default); only the outer,
        // secondary one turns that off.
        let mut primary_slide = SlideTransition::new(Rc::new(primary_position), child);
        if let Some(direction) = text_direction {
            primary_slide = primary_slide.text_direction(direction);
        }
        let mut secondary_slide = SlideTransition::new(Rc::new(secondary_position), primary_slide)
            .transform_hit_tests(false);
        if let Some(direction) = text_direction {
            secondary_slide = secondary_slide.text_direction(direction);
        }
        secondary_slide.boxed()
    }
}
