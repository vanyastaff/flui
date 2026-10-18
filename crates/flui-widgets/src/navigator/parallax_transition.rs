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
//! # Edge shadow
//!
//! While the page moves, a [`DecoratedBoxTransition`] on the primary
//! position's progress casts a soft shadow off the page's leading edge (the
//! left one, or the right under a right-to-left [`Directionality`]), growing in
//! as the page slides over the one beneath. At rest on either end it paints
//! nothing: settled, the shadow would fall entirely off screen, and a blurred
//! shadow repainted with every settled page is a cost with nothing to show for
//! it.
//!
//! # Not modelled
//!
//! The fullscreen-dialog bottom-up variant (no `fullscreen_dialog` flag on
//! `PageRoute`).

use std::rc::Rc;
use std::time::Duration;

use flui_animation::{
    Animatable, Animation, ArcCurve, Curve, CurvedAnimation, MotionPolicy, Tween, curves,
};
use flui_foundation::geometry::Offset;
use flui_objects::TranslationFraction;
use flui_painting::styling::{BoxDecoration, BoxShadow, Color};
use flui_painting::typography::TextDirection;
use flui_view::{BoxedView, BuildContext, ViewExt};

use super::navigator::NavigatorHandle;
use super::overlay_route::RouteAnimation;
use super::page_transitions::PageTransitionBuilder;
use crate::{DecoratedBoxTransition, Directionality, MediaQuery, SlideTransition};

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

/// The shadow a moving page casts off its leading edge, read off the primary
/// position's progress: none at either end, where the page is at rest, and
/// otherwise grown in from nothing toward the full shadow.
#[derive(Debug, Clone)]
struct EdgeShadow {
    tween: Tween<BoxDecoration<f64>>,
}

impl EdgeShadow {
    /// The shadow's blur, in logical pixels.
    const BLUR: f64 = 12.0;

    /// A shadow shifted toward the edge `text_direction` leads with, so it
    /// falls on the page being covered rather than under this one.
    fn new(text_direction: TextDirection) -> Self {
        let lead = if text_direction == TextDirection::Rtl {
            1.0
        } else {
            -1.0
        };
        let full = BoxDecoration::new().set_box_shadow(Some(vec![BoxShadow::new(
            Color::from_argb(0x2000_0000),
            Offset::new(lead * Self::BLUR / 2.0, 0.0),
            Self::BLUR,
            0.0,
        )]));
        Self {
            tween: Tween::new(BoxDecoration::new(), full),
        }
    }
}

impl Animatable for EdgeShadow {
    type Value = BoxDecoration<f64>;

    fn transform(&self, t: f64) -> BoxDecoration<f64> {
        if t <= 0.0 || t >= 1.0 {
            return BoxDecoration::new();
        }
        self.tween.transform(t)
    }
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

        // The page's position and its edge shadow follow one progress.
        let primary_progress: Rc<dyn Animation<f64>> = if linear {
            Rc::clone(primary)
        } else {
            Rc::new(
                CurvedAnimation::new(
                    Rc::clone(primary),
                    ArcCurve::new(curves::FAST_EASE_IN_TO_SLOW_EASE_OUT),
                )
                .with_reverse_curve(ArcCurve::new(
                    curves::FAST_EASE_IN_TO_SLOW_EASE_OUT.flipped(),
                )),
            )
        };
        let primary_position = trailing_to_middle().animate(Rc::clone(&primary_progress));

        let secondary_position = if linear {
            middle_to_leading().animate(Rc::clone(secondary))
        } else {
            let curved = CurvedAnimation::new(Rc::clone(secondary), curves::LINEAR_TO_EASE_OUT)
                .with_reverse_curve(curves::EASE_IN_TO_LINEAR);
            middle_to_leading().animate(Rc::new(curved) as Rc<dyn Animation<f64>>)
        };

        let text_direction = Directionality::maybe_of(ctx);

        let edge_shadow =
            EdgeShadow::new(text_direction.unwrap_or(TextDirection::Ltr)).animate(primary_progress);
        let shadowed = DecoratedBoxTransition::new(Rc::new(edge_shadow), child);

        // The inner slide transforms hit tests (the default); only the outer,
        // secondary one turns that off.
        let mut primary_slide = SlideTransition::new(Rc::new(primary_position), shadowed);
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
