//! [`PageTransition`] and [`PageTransitionsTheme`] — which transition a Material
//! app's routes run, per platform (ADR-0221 D3).
//!
//! `flui-widgets` owns the mechanism: a route given no `.transitions(…)` adopts
//! the nearest [`PageTransitions`](flui_sdk::widgets::PageTransitions) builder
//! of its kind when its navigator installs it. This module owns the policy:
//! [`MaterialApp`](crate::MaterialApp) installs the page builder
//! [`ThemeData::page_transitions_theme`](crate::ThemeData::page_transitions_theme)
//! names for [`ThemeData::platform`](crate::ThemeData::platform), and the
//! Material dialog fade as the popup builder, which is how
//! [`show_dialog`](crate::show_dialog) fades.
//!
//! | Transition | Duration | Motion |
//! |---|---|---|
//! | [`FadeForwards`](PageTransition::FadeForwards) | 450 ms | slides in a quarter width and fades, `EASE_IN_OUT_CUBIC_EMPHASIZED`; the covered page moves a quarter width the other way and fades out |
//! | [`Zoom`](PageTransition::Zoom) | 300 ms | scales 0.85 → 1 along `EASE_IN_OUT_CUBIC_EMPHASIZED` (two cubic phases), fading in over 12.5–25 %; the covered page scales 1 → 1.05 |
//! | [`Cupertino`](PageTransition::Cupertino) | 500 ms | `flui-widgets`' shared iOS parallax slide |
//! | [`SharedAxis`](PageTransition::SharedAxis) | 300 ms | fades in over 30–100 % while moving 30 px (or scaling 0.8 → 1) along `FAST_OUT_SLOW_IN`; the covered page fades out over 0–30 % while moving 30 px on (or scaling 1 → 1.1) |
//! | [`FadeThrough`](PageTransition::FadeThrough) | 300 ms | the covered page fades out over the first 6/20 along `Cubic(0.4, 0, 1, 1)`; the new page fades in and scales 0.92 → 1 over the remaining 14/20 along `Cubic(0, 0, 0.2, 1)` |
//! | [`None`](PageTransition::None) | 0 ms | an instant cut |
//! | dialog (popup slot) | 150 ms | fades in along `EASE_OUT` |
//!
//! Every builder's settled frame (primary at 1, secondary at 0) is the plain
//! page, so a route settled on its first reduced-motion frame shows no trace
//! of the transition.
//!
//! Not modelled: right-to-left mirroring of the horizontal shared axis and of
//! fade-forwards' slide direction (the slides move toward the leading edge
//! in left-to-right terms), and the predictive-back variants.

use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;
use std::time::Duration;

use flui_sdk::animation::{
    Animatable, Animation, Cubic, Curve, CurvedAnimation, Interval, Tween, curves,
};
use flui_sdk::pipeline::TranslationFraction;
use flui_sdk::platform::TargetPlatform;
use flui_sdk::view::prelude::BuildContext;
use flui_sdk::view::{BoxedView, ViewExt};
use flui_sdk::widgets::{
    FadeTransition, PageTransitionBuilder, ParallaxPageTransition, RouteAnimation, ScaleTransition,
    SlideTransition, Transform,
};

/// Fade-forwards' duration.
const FADE_FORWARDS_DURATION: Duration = Duration::from_millis(450);
/// Zoom's, shared axis' and fade-through's duration.
const MATERIAL_DURATION: Duration = Duration::from_millis(300);
/// The dialog fade's duration.
const DIALOG_DURATION: Duration = Duration::from_millis(150);
/// How far a shared-axis page travels, in logical pixels.
const SHARED_AXIS_TRAVEL: f64 = 30.0;
/// Where the incoming page starts fading in, for shared axis and fade through:
/// 6/20 of the run.
const FADE_IN_START: f64 = 0.3;
/// Fade-through's outgoing curve.
const FADE_THROUGH_OUT: Cubic = Cubic::new(0.4, 0.0, 1.0, 1.0);
/// Fade-through's incoming curve.
const FADE_THROUGH_IN: Cubic = Cubic::new(0.0, 0.0, 0.2, 1.0);

/// The axis a [`PageTransition::SharedAxis`] moves along.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum SharedAxis {
    /// Pages move along x: forward navigation enters from the right.
    #[default]
    Horizontal,
    /// Pages move along y: forward navigation enters from below.
    Vertical,
    /// Pages scale: the new page grows in, the covered page grows away.
    Scaled,
}

/// A Material page transition.
///
/// [`Custom`](Self::Custom) compares by identity (`Rc::ptr_eq`): two custom
/// transitions are equal only when they are the same builder.
#[derive(Clone)]
#[non_exhaustive]
pub enum PageTransition {
    /// Android's default: slide a quarter width and fade, 450 ms.
    FadeForwards,
    /// The desktop and web default: scale and fade, 300 ms.
    Zoom,
    /// The iOS parallax slide, 500 ms — Apple platforms' default.
    Cupertino,
    /// The shared-axis transition along an axis, 300 ms.
    SharedAxis(SharedAxis),
    /// The fade-through transition, 300 ms.
    FadeThrough,
    /// No transition: the route appears at once.
    None,
    /// A transition of the app's own.
    Custom(Rc<dyn PageTransitionBuilder>),
}

impl PageTransition {
    /// The builder that runs this transition.
    #[must_use]
    pub fn builder(&self) -> Rc<dyn PageTransitionBuilder> {
        match self {
            Self::FadeForwards => Rc::new(FadeForwards),
            Self::Zoom => Rc::new(Zoom),
            Self::Cupertino => Rc::new(ParallaxPageTransition::new()),
            Self::SharedAxis(axis) => Rc::new(SharedAxisTransition(*axis)),
            Self::FadeThrough => Rc::new(FadeThrough),
            Self::None => Rc::new(Cut),
            Self::Custom(builder) => Rc::clone(builder),
        }
    }
}

impl PartialEq for PageTransition {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Custom(a), Self::Custom(b)) => Rc::ptr_eq(a, b),
            (Self::SharedAxis(a), Self::SharedAxis(b)) => a == b,
            (Self::FadeForwards, Self::FadeForwards)
            | (Self::Zoom, Self::Zoom)
            | (Self::Cupertino, Self::Cupertino)
            | (Self::FadeThrough, Self::FadeThrough)
            | (Self::None, Self::None) => true,
            _ => false,
        }
    }
}

impl fmt::Debug for PageTransition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FadeForwards => f.write_str("FadeForwards"),
            Self::Zoom => f.write_str("Zoom"),
            Self::Cupertino => f.write_str("Cupertino"),
            Self::SharedAxis(axis) => f.debug_tuple("SharedAxis").field(axis).finish(),
            Self::FadeThrough => f.write_str("FadeThrough"),
            Self::None => f.write_str("None"),
            Self::Custom(builder) => f
                .debug_tuple("Custom")
                .field(&Rc::as_ptr(builder).cast::<()>())
                .finish(),
        }
    }
}

/// Which [`PageTransition`] each platform's page routes run.
///
/// A platform with no entry of its own gets [`default_for`](Self::default_for):
/// fade-forwards on Android, the Cupertino slide on iOS and macOS, zoom
/// everywhere else.
///
/// ```
/// use flui_material::{PageTransition, PageTransitionsTheme, SharedAxis};
/// use flui_sdk::platform::TargetPlatform;
///
/// let theme = PageTransitionsTheme::default()
///     .with(TargetPlatform::Android, PageTransition::SharedAxis(SharedAxis::Horizontal));
/// assert_eq!(
///     theme.transition_for(TargetPlatform::Android),
///     PageTransition::SharedAxis(SharedAxis::Horizontal)
/// );
/// assert_eq!(theme.transition_for(TargetPlatform::iOS), PageTransition::Cupertino);
/// ```
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PageTransitionsTheme {
    overrides: HashMap<TargetPlatform, PageTransition>,
}

impl PageTransitionsTheme {
    /// Run `transition` on `platform`.
    #[must_use]
    pub fn with(mut self, platform: TargetPlatform, transition: PageTransition) -> Self {
        let _previous = self.overrides.insert(platform, transition);
        self
    }

    /// The transition `platform`'s page routes run under this theme.
    #[must_use]
    pub fn transition_for(&self, platform: TargetPlatform) -> PageTransition {
        self.overrides
            .get(&platform)
            .cloned()
            .unwrap_or_else(|| Self::default_for(platform))
    }

    /// The Material default for `platform`.
    #[must_use]
    pub fn default_for(platform: TargetPlatform) -> PageTransition {
        match platform {
            TargetPlatform::Android => PageTransition::FadeForwards,
            TargetPlatform::iOS | TargetPlatform::MacOS => PageTransition::Cupertino,
            _ => PageTransition::Zoom,
        }
    }
}

/// The popup builder [`MaterialApp`](crate::MaterialApp) installs: the
/// dialog's 150 ms fade.
pub(crate) fn dialog_transition() -> Rc<dyn PageTransitionBuilder> {
    Rc::new(DialogFade)
}

/// `parent` along `curve`.
fn curved<C: Curve + 'static>(parent: &RouteAnimation, curve: C) -> RouteAnimation {
    Rc::new(CurvedAnimation::new(Rc::clone(parent), curve))
}

/// `parent` mapped linearly onto `begin..end`.
fn tweened(parent: RouteAnimation, begin: f64, end: f64) -> RouteAnimation {
    Rc::new(Tween::new(begin, end).animate(parent))
}

/// `parent` mapped linearly onto a horizontal fractional slide.
fn slid(parent: RouteAnimation, begin: f64, end: f64) -> Rc<dyn Animation<TranslationFraction>> {
    Rc::new(
        Tween::new(
            TranslationFraction::new(begin, 0.0),
            TranslationFraction::new(end, 0.0),
        )
        .animate(parent),
    )
}

/// Android's fade-forwards.
struct FadeForwards;

impl PageTransitionBuilder for FadeForwards {
    fn duration(&self) -> Duration {
        FADE_FORWARDS_DURATION
    }

    fn build(
        &self,
        _ctx: &dyn BuildContext,
        primary: &RouteAnimation,
        secondary: &RouteAnimation,
        child: BoxedView,
    ) -> BoxedView {
        let entering = curved(primary, curves::EASE_IN_OUT_CUBIC_EMPHASIZED);
        let covered = curved(secondary, curves::EASE_IN_OUT_CUBIC_EMPHASIZED);
        let page = SlideTransition::new(
            slid(Rc::clone(&entering), 0.25, 0.0),
            FadeTransition::new(entering, child),
        );
        SlideTransition::new(
            slid(Rc::clone(&covered), 0.0, -0.25),
            FadeTransition::new(tweened(covered, 1.0, 0.0), page),
        )
        .transform_hit_tests(false)
        .boxed()
    }
}

/// The desktop zoom.
struct Zoom;

impl PageTransitionBuilder for Zoom {
    fn duration(&self) -> Duration {
        MATERIAL_DURATION
    }

    fn build(
        &self,
        _ctx: &dyn BuildContext,
        primary: &RouteAnimation,
        secondary: &RouteAnimation,
        child: BoxedView,
    ) -> BoxedView {
        let scale_in = tweened(
            curved(primary, curves::EASE_IN_OUT_CUBIC_EMPHASIZED),
            0.85,
            1.0,
        );
        let fade_in = curved(primary, Interval::linear(0.125, 0.25));
        let scale_away = tweened(
            curved(secondary, curves::EASE_IN_OUT_CUBIC_EMPHASIZED),
            1.0,
            1.05,
        );
        ScaleTransition::new(
            scale_away,
            FadeTransition::new(fade_in, ScaleTransition::new(scale_in, child)),
        )
        .boxed()
    }
}

/// The shared-axis transition.
struct SharedAxisTransition(SharedAxis);

impl PageTransitionBuilder for SharedAxisTransition {
    fn duration(&self) -> Duration {
        MATERIAL_DURATION
    }

    fn build(
        &self,
        _ctx: &dyn BuildContext,
        primary: &RouteAnimation,
        secondary: &RouteAnimation,
        child: BoxedView,
    ) -> BoxedView {
        let fade_in = curved(primary, Interval::linear(FADE_IN_START, 1.0));
        let fade_out = tweened(
            curved(secondary, Interval::linear(0.0, FADE_IN_START)),
            1.0,
            0.0,
        );
        let entering = curves::FAST_OUT_SLOW_IN.transform(primary.value());
        let leaving = curves::FAST_OUT_SLOW_IN.transform(secondary.value());
        let page = FadeTransition::new(fade_in, child);
        let moved = match self.0 {
            SharedAxis::Scaled => {
                let scale_in = 0.8 + 0.2 * entering;
                let scale_away = 1.0 + 0.1 * leaving;
                Transform::scale(scale_away, scale_away)
                    .child(Transform::scale(scale_in, scale_in).child(page))
                    .boxed()
            }
            axis => {
                // Enters from 30 px on, leaves 30 px back.
                let offset = SHARED_AXIS_TRAVEL * (1.0 - entering) - SHARED_AXIS_TRAVEL * leaving;
                let (dx, dy) = if axis == SharedAxis::Vertical {
                    (0.0, offset)
                } else {
                    (offset, 0.0)
                };
                Transform::translate(dx, dy).child(page).boxed()
            }
        };
        FadeTransition::new(fade_out, moved).boxed()
    }
}

/// The fade-through transition.
struct FadeThrough;

impl PageTransitionBuilder for FadeThrough {
    fn duration(&self) -> Duration {
        MATERIAL_DURATION
    }

    fn build(
        &self,
        _ctx: &dyn BuildContext,
        primary: &RouteAnimation,
        secondary: &RouteAnimation,
        child: BoxedView,
    ) -> BoxedView {
        let entering = curved(primary, Interval::new(FADE_IN_START, 1.0, FADE_THROUGH_IN));
        let fade_out = tweened(
            curved(
                secondary,
                Interval::new(0.0, FADE_IN_START, FADE_THROUGH_OUT),
            ),
            1.0,
            0.0,
        );
        let page = FadeTransition::new(
            Rc::clone(&entering),
            ScaleTransition::new(tweened(entering, 0.92, 1.0), child),
        );
        FadeTransition::new(fade_out, page).boxed()
    }
}

/// An instant cut.
struct Cut;

impl PageTransitionBuilder for Cut {
    fn duration(&self) -> Duration {
        Duration::ZERO
    }

    fn build(
        &self,
        _ctx: &dyn BuildContext,
        _primary: &RouteAnimation,
        _secondary: &RouteAnimation,
        child: BoxedView,
    ) -> BoxedView {
        child
    }
}

/// The Material dialog's fade.
struct DialogFade;

impl PageTransitionBuilder for DialogFade {
    fn duration(&self) -> Duration {
        DIALOG_DURATION
    }

    fn build(
        &self,
        _ctx: &dyn BuildContext,
        primary: &RouteAnimation,
        _secondary: &RouteAnimation,
        child: BoxedView,
    ) -> BoxedView {
        FadeTransition::new(curved(primary, curves::EASE_OUT), child).boxed()
    }
}
