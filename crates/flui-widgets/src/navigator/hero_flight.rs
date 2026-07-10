//! [`HeroFlight`] — the shuttle that actually flies between two routes.
//!
//! ADR-0021 U4. **Private.** Nothing here is exported.
//!
//! # Flutter parity
//!
//! `.flutter/packages/flutter/lib/src/widgets/heroes.dart`, master
//! `3.33.0-0.0.pre-6280-g88e87cd963f`: `_HeroFlight` (`:544-737`),
//! `_HeroFlight.start` (`:698-736`), `_buildOverlay` (`:571-598`),
//! `_performAnimationUpdate` (`:600-618`), `onTick` (`:666-696`),
//! `_defaultHeroFlightShuttleBuilder` (`:1076-1090`).
//!
//! # The shape
//!
//! `HeroController` measures a [`HeroFlightManifest`]; this takes one and turns it
//! into three things:
//!
//! 1. **Two placeholders.** `fromHero.startFlight(includeChild: push)` and
//!    `toHero.startFlight()` (`:730-734`) freeze both heroes at their committed sizes
//!    so the pages around them do not reflow while the shuttle is away. Nothing is
//!    reparented — ADR-0021 D1.
//! 2. **One overlay entry**, holding a `Positioned` shuttle inside an inner `Stack`,
//!    wrapped in an `IgnorePointer` (`:588-596`). The inner `Stack` is ADR-0021 S8,
//!    now verified: `RenderTheater` drops a bare `Positioned`'s parent data.
//! 3. **A driven `ProxyAnimation`**, whose parent is the destination route's animation
//!    for a push and its *reverse* for a pop (`:719-724`).
//!
//! Each tick re-measures the destination and re-aims the [`RectTween`]; when the
//! animation stops, the entry is removed and both heroes are released.
//!
//! # Deferred, and named
//!
//! * **No divert (U5).** Flutter redirects an in-flight hero when a second transition
//!   starts for the same tag (`_HeroFlight.divert`, `:738-813`). This slice does the
//!   conservative thing instead: [`FlightManager::start`] ends the existing flight
//!   before starting the new one, so a shuttle never stacks. Both heroes are released
//!   with `keep_placeholder: false`, which is what `_HeroFlight.abort` +
//!   `_performAnimationUpdate` would leave behind for a dismissed flight. The visible
//!   difference is a jump cut where Flutter would redirect.
//! * **No `createRectTween` / `flightShuttleBuilder` hooks**, no `placeholderBuilder`,
//!   no `Hero.curve` / `reverseCurve`. The tween is a linear `RectTween`, and the
//!   animation is used raw — Flutter wraps it in a `CurvedAnimation` (`:472-479`)
//!   whose default for a `Hero` is linear, so this is the same curve, not a missing
//!   one.
//! * **No `userGestureInProgress`.** `_handleAnimationUpdate`'s delay (`:620-648`)
//!   exists only for the iOS back-swipe. FLUI has none, so the status update is never
//!   deferred.
//! * **No `navigatorSize`.** Flutter converts the rect to a `RelativeRect` against it
//!   (`:591-592`) because its `Positioned` takes edge insets; FLUI's takes
//!   `left`/`top`/`width`/`height` directly, so the size is not needed.

// U4 is the flight. Nothing constructs a `HeroController` in production until the
// public `Hero` API lands (U6), so `dead_code` cascades from there into everything
// here. The tests are this module's only callers.
#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use flui_animation::{
    Animatable, Animation, AnimationStatus, Curve, Interval, ProxyAnimation, RectTween,
    ReverseAnimation,
};
use flui_foundation::{Listenable, ListenerId, RenderId};
use flui_geometry::Rect;
use flui_view::prelude::*;
use flui_view::{AnimatedView, BoxedView, ViewExt, impl_animated_view};
use parking_lot::Mutex;

use super::hero::{HeroHandle, HeroTag};
use super::hero_controller::{FlightDirection, HeroFlightManifest};
use crate::overlay::{InsertPosition, OverlayEntry, OverlayHandle};
use crate::{IgnorePointer, Opacity, Positioned, Stack, StackFit};

/// Everything one in-flight hero shares between its overlay entry, its animation
/// listeners, and the manager that owns it.
struct FlightInner {
    tag: HeroTag,
    direction: FlightDirection,
    from_hero: HeroHandle,
    to_hero: HeroHandle,
    /// The destination route's coordinate root, for the per-tick re-measure.
    to_route_subtree: RenderId,
    overlay: OverlayHandle,

    /// `_HeroFlight._proxyAnimation` (`heroes.dart:557`): the animation the shuttle
    /// reads, already reversed for a pop.
    proxy: Arc<ProxyAnimation<f32>>,
    /// `_HeroFlight.heroRectTween` (`:553`). Re-aimed by [`FlightInner::on_tick`].
    rect: Mutex<RectTween>,
    /// `_HeroFlight._heroOpacity` (`:556`), evaluated eagerly. `1.0` until the
    /// destination is lost.
    opacity: Mutex<f32>,
    /// The animation value at which the destination was lost — the left edge of
    /// Flutter's `Interval(_proxyAnimation.value, 1.0)` (`:690`).
    fade_from: Mutex<Option<f32>>,
    /// `_HeroFlight._aborted` (`:566`).
    aborted: AtomicBool,
    /// Guards a re-entrant `_performAnimationUpdate`.
    ended: AtomicBool,

    entry: Mutex<Option<OverlayEntry>>,
    subscriptions: Mutex<Option<(ListenerId, ListenerId)>>,
    /// `_defaultHeroFlightShuttleBuilder`'s result (`:1089`), inflated once at start.
    shuttle: Mutex<Option<BoxedView>>,
}

impl FlightInner {
    /// `onTick` (`heroes.dart:666-696`).
    ///
    /// The destination hero may move between the frame that measured it and the frame
    /// that lands on it — a rebuild above it, a scroll, a relayout. Every tick asks
    /// where it is *now*, and re-aims the tween at it.
    ///
    /// **`begin` is preserved.** Flutter re-creates the tween as
    /// `createHeroRectTween(begin: heroRectTween.begin, end: heroRectEnd)` (`:685`):
    /// the shuttle keeps interpolating from where it started, not from where it
    /// currently is. Re-basing `begin` on the current rect would make the shuttle
    /// accelerate every time the destination twitched.
    fn on_tick(&self) {
        let destination = if self.aborted.load(Ordering::Relaxed) {
            None
        } else {
            self.to_hero.bounding_box_in(self.to_route_subtree)
        };
        let origin = destination
            .map(|rect| (rect.min_x(), rect.min_y()))
            .filter(|(x, y)| x.0.is_finite() && y.0.is_finite());

        if let Some((x, y)) = origin {
            let mut rect = self.rect.lock();
            if rect.end.min_x() != x || rect.end.min_y() != y {
                // `heroRectEnd = toHeroOrigin & heroRectTween.end!.size` (`:685`): the
                // *origin* is re-read, the size is the one that was measured.
                let size = rect.end.size();
                rect.end = Rect::from_ltwh(x, y, size.width, size.height);
            }
        } else {
            // "The toHero no longer exists or it's no longer the flight's destination.
            //  Continue flying while fading out." (`:687-692`)
            let mut fade_from = self.fade_from.lock();
            if fade_from.is_none() {
                *fade_from = Some(self.proxy.value());
            }
        }
        self.aborted.store(origin.is_none(), Ordering::Relaxed);

        // `_heroOpacity = _proxyAnimation.drive(_reverseTween.chain(CurveTween(
        //  Interval(_proxyAnimation.value, 1.0))))` (`:689-691`): `_reverseTween` is
        // `1 -> 0`, so the opacity is `1 - interval(t)`.
        let opacity = match *self.fade_from.lock() {
            Some(from) => 1.0 - Interval::linear(from, 1.0).transform(self.proxy.value()),
            None => 1.0,
        };
        *self.opacity.lock() = opacity;
    }

    /// The rect the shuttle occupies right now, in the theater's coordinate space.
    fn current_rect(&self) -> Rect {
        self.rect.lock().transform(self.proxy.value())
    }
}

/// One hero in flight.
#[derive(Clone)]
pub(crate) struct HeroFlight {
    inner: Arc<FlightInner>,
}

impl HeroFlight {
    pub(crate) fn tag(&self) -> &HeroTag {
        &self.inner.tag
    }

    /// The overlay entry this flight presents its shuttle in, while it has one.
    #[cfg(test)]
    pub(crate) fn entry_id(&self) -> Option<crate::overlay::OverlayEntryId> {
        self.inner.entry.lock().as_ref().map(OverlayEntry::id)
    }

    /// The tween's current evaluation — where the shuttle is.
    #[cfg(test)]
    pub(crate) fn shuttle_rect(&self) -> Rect {
        self.inner.current_rect()
    }

    /// The tween's destination, re-aimed by every tick.
    #[cfg(test)]
    pub(crate) fn target_rect(&self) -> Rect {
        self.inner.rect.lock().end
    }

    /// The tween's origin. Re-aiming the destination must never move it
    /// (`heroes.dart:685` preserves `begin`).
    #[cfg(test)]
    pub(crate) fn begin_rect(&self) -> Rect {
        self.inner.rect.lock().begin
    }

    #[cfg(test)]
    pub(crate) fn opacity(&self) -> f32 {
        *self.inner.opacity.lock()
    }

    /// `_HeroFlight._performAnimationUpdate` (`heroes.dart:600-618`), minus the
    /// `onFlightEnded` callback — the manager does that half.
    ///
    /// Idempotent: detaching the proxy re-fires its status listener, and a diverted
    /// flight is ended by the manager before its own listener would.
    fn finish(&self, status: AnimationStatus) {
        if self.inner.ended.swap(true, Ordering::SeqCst) {
            return;
        }

        if let Some((value, status_id)) = self.inner.subscriptions.lock().take() {
            self.inner.proxy.remove_listener(value);
            self.inner.proxy.remove_status_listener(status_id);
        }

        if let Some(entry) = self.inner.entry.lock().take()
            && entry.is_attached()
        {
            entry.remove();
        }

        // "If [AnimationStatus.completed], toHero will be the one on top and we keep
        //  fromHero hidden. If [AnimationStatus.dismissed], the animation is triggered
        //  but canceled before it finishes. In this case, we keep toHero hidden
        //  instead." (`:608-614`)
        self.inner.from_hero.end_flight(status.is_completed());
        self.inner.to_hero.end_flight(status.is_dismissed());
    }
}

/// Everything `FlightManager::start` needs that the manifest does not carry.
///
/// A bundle rather than six parameters: the manifest is pure recorded data (U3.5), and
/// these are the live capabilities the flight will drive.
pub(crate) struct FlightPlan {
    pub(crate) direction: FlightDirection,
    pub(crate) from_hero: HeroHandle,
    pub(crate) to_hero: HeroHandle,
    /// The destination route's coordinate root, for the per-tick re-measure.
    pub(crate) to_route_subtree: RenderId,
    pub(crate) overlay: OverlayHandle,
    /// `manifest.animation` (`heroes.dart:466-480`): the destination route's primary
    /// animation for a push, the source route's for a pop.
    pub(crate) animation: Arc<dyn Animation<f32>>,
}

/// `HeroController._flights` (`heroes.dart:850`) plus the deferred-drop discipline
/// FLUI needs and Dart does not.
///
/// # Why flights are retired rather than dropped
///
/// A flight ends from inside its own `ProxyAnimation` status listener.
/// `ProxyAnimation::fan_out_status` snapshots the callbacks and then iterates them
/// while holding `&self` — so dropping the last `Arc<FlightInner>`, and with it the
/// proxy, *inside* that callback would free the animation the callback is running
/// under. Dart's GC makes this a non-question.
///
/// So `finish` never drops: it moves the flight into [`retired`](Self::retired), which
/// is drained the next time the controller runs a measurement pass — outside every
/// listener.
#[derive(Default)]
pub(crate) struct FlightManager {
    flights: Mutex<HashMap<HeroTag, HeroFlight>>,
    retired: Mutex<Vec<HeroFlight>>,
}

impl FlightManager {
    /// Free everything a finished flight was holding. Called from the post-frame
    /// measurement pass, never from an animation listener.
    pub(crate) fn drain_retired(&self) {
        let retired = std::mem::take(&mut *self.retired.lock());
        drop(retired);
    }

    /// How many flights are in the air.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.flights.lock().len()
    }

    /// The flight for `tag`, if any.
    #[cfg(test)]
    pub(crate) fn get(&self, tag: &HeroTag) -> Option<HeroFlight> {
        self.flights.lock().get(tag).cloned()
    }

    /// `_HeroFlight.start` (`heroes.dart:698-736`), plus the no-divert decision.
    ///
    /// `animation` is `manifest.animation` (`:466-480`): the **destination** route's
    /// primary animation for a push, the **source** route's for a pop.
    pub(crate) fn start(self: &Arc<Self>, manifest: &HeroFlightManifest, plan: FlightPlan) {
        let FlightPlan {
            direction,
            from_hero,
            to_hero,
            to_route_subtree,
            overlay,
            animation,
        } = plan;

        // No divert in U4: end the flight already in the air for this tag before
        // starting another, so shuttles never stack. Flutter would `divert` it.
        if let Some(existing) = self.flights.lock().remove(&manifest.tag) {
            existing.finish(AnimationStatus::Dismissed);
            self.retired.lock().push(existing);
        }

        // `_proxyAnimation.parent = ReverseAnimation(manifest.animation)` for a pop,
        // `manifest.animation` for a push (`:719-724`).
        let parent: Arc<dyn Animation<f32>> = match direction {
            FlightDirection::Push => animation,
            FlightDirection::Pop => Arc::new(ReverseAnimation::new(animation)),
        };

        let inner = Arc::new(FlightInner {
            tag: manifest.tag.clone(),
            direction,
            from_hero: from_hero.clone(),
            to_hero: to_hero.clone(),
            to_route_subtree,
            overlay: overlay.clone(),
            proxy: Arc::new(ProxyAnimation::new(parent)),
            rect: Mutex::new(RectTween {
                begin: manifest.from_rect,
                end: manifest.to_rect,
            }),
            opacity: Mutex::new(1.0),
            fade_from: Mutex::new(None),
            aborted: AtomicBool::new(false),
            ended: AtomicBool::new(false),
            entry: Mutex::new(None),
            subscriptions: Mutex::new(None),
            shuttle: Mutex::new(None),
        });

        // `shouldIncludeChildInPlaceholder` is `true` only for the *from* hero of a
        // push (`:716-724`): its subtree is preserved offstage so its state survives.
        from_hero.start_flight(direction == FlightDirection::Push);
        to_hero.start_flight(false);

        // `_defaultHeroFlightShuttleBuilder` returns the **destination** hero's child,
        // inflated afresh (`:1083`, `:1089`). Nothing is reparented (D1).
        *inner.shuttle.lock() = Some(to_hero.shuttle_child());

        let entry = {
            let inner = Arc::clone(&inner);
            OverlayEntry::new(move |_ctx| {
                Shuttle {
                    flight: Arc::clone(&inner),
                }
                .boxed()
            })
        };
        overlay.insert(&entry, &InsertPosition::Top);
        *inner.entry.lock() = Some(entry);

        let flight = HeroFlight {
            inner: Arc::clone(&inner),
        };

        // `_proxyAnimation.addListener(onTick)` (`:735`) and the status listener
        // installed in the constructor (`:547`). Both hold `Weak`s: a flight that has
        // been retired must not be resurrected by its own animation.
        let tick_target = Arc::downgrade(&inner);
        let value_id = inner.proxy.add_listener(Arc::new(move || {
            if let Some(inner) = tick_target.upgrade() {
                inner.on_tick();
            }
        }));

        let manager = Arc::downgrade(self);
        let status_target = Arc::downgrade(&inner);
        let status_id = inner.proxy.add_status_listener(Arc::new(move |status| {
            // `if (!status.isAnimating)` (`heroes.dart:601`) — `AnimationStatus` here
            // carries no `is_animating`, and forward/reverse is exactly the complement
            // of dismissed/completed.
            if !matches!(
                status,
                AnimationStatus::Dismissed | AnimationStatus::Completed
            ) {
                return;
            }
            let (Some(manager), Some(inner)) = (manager.upgrade(), status_target.upgrade()) else {
                return;
            };
            manager.finish(&HeroFlight { inner }, status);
        }));
        *inner.subscriptions.lock() = Some((value_id, status_id));

        self.flights.lock().insert(manifest.tag.clone(), flight);
    }

    /// `HeroController._handleFlightEnded` (`heroes.dart:1069-1071`): drop the flight
    /// from the registry. Called from the flight's own status listener, so the flight
    /// is *retired*, not dropped — see the type docs.
    fn finish(&self, flight: &HeroFlight, status: AnimationStatus) {
        flight.finish(status);
        let removed = self.flights.lock().remove(flight.tag());
        if let Some(removed) = removed {
            self.retired.lock().push(removed);
        }
    }
}

// ============================================================================
// The shuttle
// ============================================================================

/// `_HeroFlight._buildOverlay` (`heroes.dart:571-598`).
///
/// An [`AnimatedView`] over the flight's `ProxyAnimation`, so every tick rebuilds it —
/// Flutter's `AnimatedBuilder(animation: _proxyAnimation, …)` (`:583`).
///
/// The inner `Stack` is **load-bearing**: `RenderTheater` runs no positioned split, so
/// a `Positioned` handed straight to an overlay entry has its parent data dropped and
/// lands at the origin. ADR-0021 S8, verified by
/// `overlay::tests::positioned_inside_an_overlay_entry_is_laid_out_by_an_inner_stack`.
#[derive(Clone)]
struct Shuttle {
    flight: Arc<FlightInner>,
}

impl std::fmt::Debug for Shuttle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shuttle")
            .field("tag", &self.flight.tag)
            .finish_non_exhaustive()
    }
}

impl_animated_view!(Shuttle);

impl AnimatedView for Shuttle {
    fn listenable(&self) -> Arc<dyn Listenable> {
        Arc::clone(&self.flight.proxy) as Arc<dyn Listenable>
    }
}

impl StatefulView for Shuttle {
    type State = ShuttleState;

    fn create_state(&self) -> Self::State {
        ShuttleState
    }
}

pub(crate) struct ShuttleState;

impl ViewState<Shuttle> for ShuttleState {
    fn build(&self, view: &Shuttle, _ctx: &dyn BuildContext) -> impl IntoView {
        let rect = view.flight.current_rect();
        let opacity = *view.flight.opacity.lock();
        let child = view
            .flight
            .shuttle
            .lock()
            .clone()
            .unwrap_or_else(|| crate::SizedBox::shrink().boxed());

        // `Positioned(… child: IgnorePointer(child: FadeTransition(…)))` (`:588-596`).
        // `Opacity`, not `FadeTransition`: the opacity is evaluated eagerly in
        // `on_tick`, so there is no second animation to subscribe to.
        Stack::new(vec![
            Positioned::new(
                IgnorePointer::new()
                    .ignoring(true)
                    .child(Opacity::new(opacity).child(child)),
            )
            .left(rect.min_x().0)
            .top(rect.min_y().0)
            .width(rect.width().0)
            .height(rect.height().0)
            .into_view()
            .boxed(),
        ])
        .fit(StackFit::Expand)
    }
}
