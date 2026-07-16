//! [`cupertino_page_route`] — the iOS-style slide-in page transition, over
//! `flui-widgets`' existing [`PageRoute`] machinery.
//!
//! Flutter parity: `cupertino/route.dart`'s `CupertinoRouteTransitionMixin` +
//! `CupertinoPageRoute` (oracle tag `3.44.0`). `PageRoute` documents itself as
//! deliberately not extensible (no subclassing in Rust, and this crate declines
//! to expose `ModalRoute`/`TransitionRoute` as bases) — so this is exactly what
//! its own doc anticipates: a **thin constructor** that configures the existing
//! builder, not a new route type.
//!
//! ## What this ports
//!
//! - `kTransitionDuration` (500 ms, `route.dart`'s
//!   `CupertinoRouteTransitionMixin.kTransitionDuration`).
//! - `_kCupertinoPageTransitionBarrierColor` (`0x18000000`), via the
//!   [`PageRoute::barrier_color`] this change adds (mirroring `PopupRoute`'s
//!   existing builder method — `PageRoute` had no barrier at all before).
//! - `back_gesture(true)` by default — `CupertinoRouteTransitionMixin` wires
//!   the edge-swipe-back detector unconditionally under `TargetPlatform.iOS`;
//!   FLUI has no platform-theme selection, so every `cupertino_page_route` opts
//!   in (matching `flui-widgets`' own `PageRoute::back_gesture` doc, which
//!   names this exact route as the Cupertino opt-in's first real caller).
//! - `CupertinoPageTransition`'s curve wiring, verified against the oracle
//!   (not guessed): the **primary** position (this page sliding in/out) is
//!   curved with `Curves.fastEaseInToSlowEaseOut`, reverse-curved with its
//!   `.flipped()`. The **secondary** position (this page being covered by the
//!   next) is curved with `Curves.linearToEaseOut` forward, `Curves.easeInToLinear`
//!   reverse. Both tween `Offset` (here, [`TranslationFraction`]) through
//!   [`SlideTransition`] exactly as the oracle's `SlideTransition` does.
//! - `linearTransition`: mid-back-gesture, the oracle skips both curves and
//!   drives the tweens directly off the raw animations, "to precisely track
//!   finger motions" (`buildPageTransitions`'s `route.popGestureInProgress`).
//!   This reads [`NavigatorHandle::user_gesture_in_progress`] off the ambient
//!   `NavigatorHandle` the `transitions` closure's own `BuildContext` already
//!   publishes (the same lookup `modal_route.rs` uses to wire the back-gesture
//!   detector itself) — not a guess or a dropped feature.
//!
//! ## Deferred, named
//!
//! - **Edge shadow** (`_CupertinoEdgeShadowDecoration`/`_CupertinoEdgeShadowPainter`,
//!   wrapped via `DecoratedBoxTransition`). `flui-widgets` has no
//!   `DecoratedBoxTransition` (an `Animation<Decoration>`-driven proxy) and
//!   `flui-painting`'s `BoxDecoration` has no `Decoration::lerp`-equivalent
//!   trait for tweening an arbitrary decoration; adding either is out of this
//!   change's scope. The primary page's leading edge paints with no shadow
//!   gradient during the transition.
//! - **`title`/`previousTitle`** (`CupertinoRouteTransitionMixin.title`,
//!   `previousTitle` `ValueListenable`). These exist to auto-populate
//!   `CupertinoNavigationBar`'s `middle`/`largeTitle` when the app author does
//!   not supply one — a consumer this crate does not ship yet (no
//!   `CupertinoNavigationBar` in this pass). Deferred with that consumer.
//! - **`fullscreenDialog`** (`CupertinoFullscreenDialogTransition`, the
//!   bottom-up sheet transition, and the barrier/back-gesture/edge-shadow
//!   suppression that comes with it). Not modeled — `flui-widgets`' `PageRoute`
//!   has no `fullscreenDialog` flag yet (`back_gesture.rs`'s module doc already
//!   records this same gap for the detector).
//! - **`delegatedTransition`** (`CupertinoPageTransition.delegatedTransition`,
//!   used by the `Page`/`Navigator 2.0` API to let a covered route borrow the
//!   *covering* route's own transition instead of running its own secondary
//!   animation). FLUI has no `Page`/declarative-Navigator API to delegate
//!   through; `canTransitionTo`/`canTransitionFrom`'s `nextRouteHasDelegatedTransition`
//!   branch has nothing to call.
//! - **`canTransitionTo`/`canTransitionFrom`** narrow the oracle's own
//!   secondary-animation coordination to routes that are *specifically*
//!   `CupertinoRouteTransitionMixin` (or carry a `delegatedTransition`).
//!   `flui-widgets`' existing (private) `TransitionGroup::Page` coordinates
//!   every `PageRoute` together, Cupertino-styled or not — broader than the
//!   oracle.
//!   **Named divergence**, not a silent gap: FLUI has no other `PageRoute`
//!   "style" (no `MaterialPageRoute`) to conflict with yet, so the two
//!   groupings coincide in practice today.
//! - `CupertinoModalPopupRoute`, `CupertinoDialogRoute`,
//!   `CupertinoPageTransitionsBuilder` — separate route/theme types, out of
//!   this component's scope.

use std::sync::Arc;
use std::time::Duration;

use flui_animation::{Animation, ArcCurve, Curve, CurvedAnimation, Curves, Tween, animate};
use flui_objects::TranslationFraction;
use flui_types::Color;
use flui_view::prelude::BuildContext;
use flui_view::{BoxedView, ViewExt};
use flui_widgets::{Directionality, NavigatorHandle, PageRoute, RouteAnimation, SlideTransition};

/// `CupertinoRouteTransitionMixin.kTransitionDuration` (`route.dart`, oracle
/// tag `3.44.0`).
const TRANSITION_DURATION: Duration = Duration::from_millis(500);

/// `_kCupertinoPageTransitionBarrierColor` (`route.dart`, oracle tag
/// `3.44.0`): "a relatively rigorous eyeball estimation", `0x18000000`.
fn barrier_color() -> Color {
    Color::from_argb(0x1800_0000)
}

/// `_kRightMiddleTween` (`route.dart`): offscreen right → on screen.
fn right_middle_tween() -> Tween<TranslationFraction> {
    Tween::new(
        TranslationFraction::new(1.0, 0.0),
        TranslationFraction::ZERO,
    )
}

/// `_kMiddleLeftTween` (`route.dart`): on screen → 1/3 offscreen left, the
/// parallax a covering page applies to this one.
fn middle_left_tween() -> Tween<TranslationFraction> {
    Tween::new(
        TranslationFraction::ZERO,
        TranslationFraction::new(-1.0 / 3.0, 0.0),
    )
}

/// A `cupertino_page_route`-configured [`PageRoute`], showing `builder` with
/// the iOS slide transition, a transition-only barrier dim, and an
/// edge-swipe-back gesture. Flutter parity: `CupertinoPageRoute` (`route.dart`,
/// oracle tag `3.44.0`) — see the module docs for exactly what is and is not
/// ported.
///
/// Returns a plain [`PageRoute<T>`], not a distinct wrapper type: every other
/// `PageRoute` builder method (`.named(...)`, `.maintain_state(...)`, …) stays
/// available to chain afterward, exactly as if this were `PageRoute::new`
/// itself with Cupertino's defaults pre-applied.
///
/// ```
/// use flui_cupertino::cupertino_page_route;
/// use flui_widgets::Text;
/// use flui_view::prelude::*;
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

/// `CupertinoRouteTransitionMixin.buildPageTransitions`'s non-fullscreen-dialog
/// branch → `CupertinoPageTransition.build` (`route.dart`, oracle tag
/// `3.44.0`).
fn cupertino_page_transitions(
    ctx: &dyn BuildContext,
    primary: &RouteAnimation,
    secondary: &RouteAnimation,
    child: BoxedView,
) -> BoxedView {
    // `route.popGestureInProgress` (`ModalRoute.popGestureInProgress =>
    // navigator!.userGestureInProgress`): read fresh every build off the same
    // ambient `NavigatorHandle` `modal_route.rs` itself resolves the back
    // gesture detector through.
    let linear_transition = NavigatorHandle::maybe_of(ctx)
        .is_some_and(|navigator| navigator.user_gesture_in_progress());

    let primary_position = if linear_transition {
        animate(right_middle_tween(), Arc::clone(primary))
    } else {
        let curved = CurvedAnimation::new(
            Arc::clone(primary),
            ArcCurve::new(Curves::FastEaseInToSlowEaseOut),
        )
        .with_reverse_curve(ArcCurve::new(Curves::FastEaseInToSlowEaseOut.flipped()));
        let curved: Arc<dyn Animation<f32>> = Arc::new(curved);
        animate(right_middle_tween(), curved)
    };

    let secondary_position = if linear_transition {
        animate(middle_left_tween(), Arc::clone(secondary))
    } else {
        let curved = CurvedAnimation::new(Arc::clone(secondary), Curves::LinearToEaseOut)
            .with_reverse_curve(Curves::EaseInToLinear);
        let curved: Arc<dyn Animation<f32>> = Arc::new(curved);
        animate(middle_left_tween(), curved)
    };

    let text_direction = Directionality::maybe_of(ctx);

    // `SlideTransition(position: _primaryPositionAnimation, textDirection:
    // textDirection, child: …)` — the inner slide keeps `transformHitTests`'s
    // default of `true` (only the outer/secondary one turns it off).
    let mut primary_slide = SlideTransition::new(Arc::new(primary_position), child);
    if let Some(direction) = text_direction {
        primary_slide = primary_slide.text_direction(direction);
    }

    // `SlideTransition(position: _secondaryPositionAnimation, textDirection:
    // textDirection, transformHitTests: false, child: …)`.
    let mut secondary_slide = SlideTransition::new(Arc::new(secondary_position), primary_slide)
        .transform_hit_tests(false);
    if let Some(direction) = text_direction {
        secondary_slide = secondary_slide.text_direction(direction);
    }
    secondary_slide.boxed()
}
