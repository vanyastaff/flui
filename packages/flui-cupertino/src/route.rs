//! [`cupertino_page_route`] — the iOS-style slide-in page transition, over
//! `flui-widgets`' existing [`PageRoute`] machinery.
//!
//! `PageRoute` documents itself as deliberately not extensible (no subclassing
//! in Rust, and this crate declines to expose `ModalRoute`/`TransitionRoute` as
//! bases) — so this is exactly what its own doc anticipates: a **thin
//! constructor** that configures the existing builder, not a new route type.
//!
//! ## What it configures
//!
//! - A 500 ms transition duration.
//! - A `0x18000000` barrier color, via [`PageRoute::barrier_color`] (mirroring
//!   `PopupRoute`'s existing builder method).
//! - `back_gesture(true)` by default — the iOS route wires the edge-swipe-back
//!   detector unconditionally; FLUI has no platform-theme selection, so every
//!   `cupertino_page_route` opts in (matching `flui-widgets`' own
//!   `PageRoute::back_gesture` doc, which names this exact route as the
//!   Cupertino opt-in's first real caller).
//! - The page transition's curve wiring: the **primary** position (this page
//!   sliding in/out) is curved with `curves::FAST_EASE_IN_TO_SLOW_EASE_OUT`,
//!   reverse-curved with its `.flipped()`. The **secondary** position (this
//!   page being covered by the next) is curved with `curves::LINEAR_TO_EASE_OUT`
//!   forward, `curves::EASE_IN_TO_LINEAR` reverse. Both tween a
//!   [`TranslationFraction`] through [`SlideTransition`].
//! - A linear transition: mid-back-gesture, both curves are skipped and the
//!   tweens are driven directly off the raw animations, to precisely track
//!   finger motion. This reads [`NavigatorHandle::user_gesture_in_progress`]
//!   off the ambient `NavigatorHandle` the `transitions` closure's own
//!   `BuildContext` already publishes (the same lookup `modal_route.rs` uses to
//!   wire the back-gesture detector itself).
//! - An edge shadow: while the page moves, a [`DecoratedBoxTransition`] on the
//!   primary position's animation casts a soft shadow off the page's leading
//!   edge (the left one, or the right under a right-to-left
//!   [`Directionality`]), growing in as the page slides over the one beneath.
//!   At rest on either end it paints nothing: settled, the shadow would fall
//!   entirely off screen, and a blurred shadow repainted with every settled
//!   page is a cost with nothing to show for it.
//!
//! ## Deferred, named
//!
//! - **`title`/`previous_title`.** These exist to auto-populate
//!   `CupertinoNavigationBar`'s `middle`/large title when the app author does
//!   not supply one — a consumer this crate does not ship yet. Deferred with
//!   that consumer.
//! - **`fullscreen_dialog`** (the bottom-up sheet transition, and the
//!   barrier/back-gesture/edge-shadow suppression that comes with it). Not
//!   modeled — `flui-widgets`' `PageRoute` has no `fullscreen_dialog` flag yet
//!   (`back_gesture.rs`'s module doc already records this same gap for the
//!   detector).
//! - **A delegated transition** (letting a covered route borrow the
//!   *covering* route's own transition instead of running its own secondary
//!   animation). FLUI has no declarative-Navigator API to delegate through.
//! - **`can_transition_to`/`can_transition_from`.** Secondary-animation
//!   coordination could be narrowed to routes that are *specifically*
//!   Cupertino. `flui-widgets`' existing (private) `TransitionGroup::Page`
//!   coordinates every `PageRoute` together, Cupertino-styled or not — broader
//!   than that. FLUI has no other `PageRoute` "style" (no Material page
//!   route) to conflict with yet, so the two groupings coincide in practice
//!   today.
//! - A modal popup route, a dialog route and a page-transitions builder —
//!   separate route/theme types, out of this component's scope.

use std::time::Duration;

use flui_sdk::animation::{Animatable, Animation, ArcCurve, Curve, CurvedAnimation, Tween, curves};
use flui_sdk::geometry::Offset;
use flui_sdk::painting::{BoxDecoration, BoxShadow, Color, TextDirection};
use flui_sdk::pipeline::TranslationFraction;
use flui_sdk::view::prelude::BuildContext;
use flui_sdk::view::{BoxedView, ViewExt};
use flui_sdk::widgets::{
    DecoratedBoxTransition, Directionality, NavigatorHandle, PageRoute, RouteAnimation,
    SlideTransition,
};

/// The page transition's duration.
const TRANSITION_DURATION: Duration = Duration::from_millis(500);

/// The barrier dim painted during the transition, `0x18000000` — an eyeballed
/// estimate of the iOS dim.
fn barrier_color() -> Color {
    Color::from_argb(0x1800_0000)
}

/// Offscreen right → on screen.
fn right_middle_tween() -> Tween<TranslationFraction> {
    Tween::new(
        TranslationFraction::new(1.0, 0.0),
        TranslationFraction::ZERO,
    )
}

/// On screen → 1/3 offscreen left, the
/// parallax a covering page applies to this one.
fn middle_left_tween() -> Tween<TranslationFraction> {
    Tween::new(
        TranslationFraction::ZERO,
        TranslationFraction::new(-1.0 / 3.0, 0.0),
    )
}

/// The shadow a moving page casts off its leading edge, read off the primary
/// position's progress: none at either end, where the page is at rest, and
/// otherwise faded in from nothing toward [`EdgeShadow::full`].
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

/// A `cupertino_page_route`-configured [`PageRoute`], showing `builder` with
/// the iOS slide transition, a transition-only barrier dim, and an
/// edge-swipe-back gesture — see the module docs for exactly what is and is
/// not supported.
///
/// Returns a plain [`PageRoute<T>`], not a distinct wrapper type: every other
/// `PageRoute` builder method (`.named(...)`, `.maintain_state(...)`, …) stays
/// available to chain afterward, exactly as if this were `PageRoute::new`
/// itself with Cupertino's defaults pre-applied.
///
/// ```
/// use flui_cupertino::cupertino_page_route;
/// use flui_sdk::widgets::Text;
/// use flui_sdk::view::prelude::*;
///
/// let route = cupertino_page_route::<(), _>(|_ctx, _primary, _secondary| {
///     Text::new("Details").into_view().boxed()
/// })
/// .named("/details");
/// # let _ = route;
/// ```
#[must_use]
pub fn cupertino_page_route<T, F>(builder: F) -> PageRoute<T>
where
    T: Send + Clone + 'static,
    F: Fn(&dyn BuildContext, &RouteAnimation, &RouteAnimation) -> BoxedView + 'static,
{
    PageRoute::new(builder)
        .transition_duration(TRANSITION_DURATION)
        .barrier_color(barrier_color())
        .back_gesture(true)
        .transitions(cupertino_page_transitions)
}

/// The iOS slide transition for a non-fullscreen-dialog page.
fn cupertino_page_transitions(
    ctx: &dyn BuildContext,
    primary: &RouteAnimation,
    secondary: &RouteAnimation,
    child: BoxedView,
) -> BoxedView {
    if flui_sdk::widgets::MediaQuery::motion_of(ctx)
        == Some(flui_sdk::animation::MotionPolicy::Reduce)
    {
        return child;
    }
    // Whether a pop gesture is in progress: read fresh every build off the same
    // ambient `NavigatorHandle` `modal_route.rs` itself resolves the back
    // gesture detector through.
    let linear_transition = NavigatorHandle::maybe_of(ctx)
        .is_some_and(|navigator| navigator.user_gesture_in_progress());

    // The page's position and its edge shadow follow one progress.
    let primary_progress: std::rc::Rc<dyn Animation<f64>> = if linear_transition {
        std::rc::Rc::clone(primary)
    } else {
        std::rc::Rc::new(
            CurvedAnimation::new(
                std::rc::Rc::clone(primary),
                ArcCurve::new(curves::FAST_EASE_IN_TO_SLOW_EASE_OUT),
            )
            .with_reverse_curve(ArcCurve::new(
                curves::FAST_EASE_IN_TO_SLOW_EASE_OUT.flipped(),
            )),
        )
    };
    let primary_position = right_middle_tween().animate(std::rc::Rc::clone(&primary_progress));

    let secondary_position = if linear_transition {
        middle_left_tween().animate(std::rc::Rc::clone(secondary))
    } else {
        let curved =
            CurvedAnimation::new(std::rc::Rc::clone(secondary), curves::LINEAR_TO_EASE_OUT)
                .with_reverse_curve(curves::EASE_IN_TO_LINEAR);
        let curved: std::rc::Rc<dyn Animation<f64>> = std::rc::Rc::new(curved);
        middle_left_tween().animate(curved)
    };

    let text_direction = Directionality::maybe_of(ctx);

    let edge_shadow =
        EdgeShadow::new(text_direction.unwrap_or(TextDirection::Ltr)).animate(primary_progress);
    let shadowed = DecoratedBoxTransition::new(std::rc::Rc::new(edge_shadow), child);

    // The inner slide keeps `transform_hit_tests`'s default of `true` (only the
    // outer/secondary one turns it off).
    let mut primary_slide = SlideTransition::new(std::rc::Rc::new(primary_position), shadowed);
    if let Some(direction) = text_direction {
        primary_slide = primary_slide.text_direction(direction);
    }

    // The secondary slide wraps the primary one and does not transform hit tests.
    let mut secondary_slide =
        SlideTransition::new(std::rc::Rc::new(secondary_position), primary_slide)
            .transform_hit_tests(false);
    if let Some(direction) = text_direction {
        secondary_slide = secondary_slide.text_direction(direction);
    }
    secondary_slide.boxed()
}
