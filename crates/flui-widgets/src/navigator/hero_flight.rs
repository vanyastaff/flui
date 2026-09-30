//! [`HeroFlight`] — the shuttle that actually flies between two routes.
//!
//! **Private.** Nothing here is exported.
//!
//! # The shape
//!
//! `HeroController` measures a [`HeroFlightManifest`]; this takes one and turns it
//! into three things:
//!
//! 1. **Two placeholders.** `from_hero.start_flight(include_child: push)` and
//!    `to_hero.start_flight()` freeze both heroes at their committed sizes
//!    so the pages around them do not reflow while the shuttle is away. Nothing is
//!    reparented.
//! 2. **One overlay entry**, holding a `Positioned` shuttle inside an inner `Stack`,
//!    wrapped in an `IgnorePointer`. The inner `Stack` is required and
//!    verified: `RenderTheater` drops a bare `Positioned`'s parent data.
//! 3. **A driven `ProxyAnimation`**, whose parent is the destination route's animation
//!    for a push and its *reverse* for a pop.
//!
//! Each tick re-measures the destination and re-aims the [`RectTween`]; when the
//! animation stops, the entry is removed and both heroes are released.
//!
//! # Scope, and what is deliberately different
//!
//! * **Divert is private and implemented.** A second transition for the same tag
//!   redirects the existing [`HeroFlight`] in place: same flight object, same
//!   overlay entry, new manifest-derived state.
//! * **`create_rect_tween`, `flight_shuttle_builder`, and `Hero::curve` /
//!   `reverse_curve` are implemented**. There is no caller-supplied placeholder
//!   builder that receives the child; [`Hero`](super::hero::Hero) exposes a
//!   state-preserving `placeholder` hook instead. The animation handed to this
//!   flight is already the manifest's `CurvedAnimation`, built by the controller's
//!   `launch` — `Curves::FastOutSlowIn` by default.
//! * **User-gesture deferral is implemented.** A terminal status update is parked
//!   while the navigator's user gesture is in progress and replayed once the
//!   gesture ends, so dragging a pop back to zero mid-gesture does not tear the
//!   flight down with the finger still down. See `FlightInner::wake`'s doc for the
//!   Send+Sync boundary this crosses.
//! * **No navigator size.** `Positioned` takes `left`/`top`/`width`/`height`
//!   directly, so the rect needs no conversion against the navigator's size.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Weak;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use flui_animation::{
    Animatable, Animation, AnimationStatus, Curve, Interval, ProxyAnimation, RectTween,
    ReverseAnimation, Tween, animate,
};
use flui_foundation::geometry::Rect;
use flui_foundation::{ChangeNotifier, Listenable, ListenerId, RenderId};
use flui_scheduler::LocalPostFrameHandle;
use flui_view::prelude::*;
use flui_view::{AnimatedView, BoxedView, ViewExt, impl_animated_view};
use parking_lot::Mutex;

use super::hero::{HeroHandle, HeroTag, RectTweenFactory, ShuttleBuilder};
use super::hero_controller::{FlightDirection, HeroFlightManifest};
use super::navigator::UserGestureSignal;
use crate::{IgnorePointer, Opacity, Positioned, Stack, StackFit};
use crate::{InsertPosition, OverlayEntry, OverlayHandle};

/// The manifest-derived facts a divert can replace: which way the
/// flight runs, which two heroes it connects, and the coordinate space its
/// destination lives in.
///
/// Behind one `Mutex` so `on_tick`, `finish`, and `divert` all see a coherent set.
struct FlightState {
    direction: FlightDirection,
    from_hero: HeroHandle,
    to_hero: HeroHandle,
    /// The destination route's coordinate root, for the per-tick re-measure.
    to_route_subtree: RenderId,
    /// Whether the flight was started by a user gesture: set by
    /// [`FlightManager::start`] and rewritten by [`HeroFlight::divert`]. Read by
    /// `HeroController::did_stop_user_gesture`'s manual-dismiss sweep.
    is_user_gesture_transition: bool,
}

/// Everything one in-flight hero shares between its overlay entry, its animation
/// listeners, and the manager that owns it.
struct FlightInner {
    tag: HeroTag,

    /// The half a divert rewrites in place.
    state: Mutex<FlightState>,

    /// The animation the shuttle reads, already reversed for a pop. Its **parent** is repointed by a divert;
    /// the proxy object itself, and the listeners on it, never change.
    proxy: Arc<ProxyAnimation<f64>>,
    /// The shuttle's rect-tween endpoints. Re-aimed by
    /// [`FlightInner::on_tick`]; interpolated through [`rect_factory`](Self::rect_factory).
    rect: Mutex<RectTween>,
    /// The `create_rect_tween` factory this flight interpolates with, or `None` for the
    /// linear default. Behind a lock because a divert can swap the
    /// destination hero, and with it the factory.
    rect_factory: Mutex<Option<RectTweenFactory>>,
    /// The shuttle's opacity, evaluated eagerly. `1.0` until the destination is
    /// lost.
    opacity: Mutex<f64>,
    /// The animation value at which the destination was lost — the left edge of
    /// the fade-out interval.
    fade_from: Mutex<Option<f64>>,
    /// Whether the destination hero has been lost.
    aborted: AtomicBool,
    /// Guards a re-entrant animation-update teardown.
    ended: AtomicBool,

    entry: Mutex<Option<OverlayEntry>>,
    subscriptions: Mutex<Option<ListenerId>>,
    /// A Send+Sync-safe read of this flight's navigator's user-gesture state
    /// Fixed for the flight's whole life — every divert stays within the same
    /// controller, hence the same navigator.
    gesture_signal: UserGestureSignal,
    /// What [`Shuttle`] actually subscribes to (`AnimatedView::listenable`),
    /// in place of [`proxy`](Self::proxy) directly: a relay that forwards
    /// both `proxy`'s own ticks *and* [`gesture_signal`](Self::gesture_signal)'s
    /// notifier — the same "merge multiple `Listenable`s into one"
    /// `ChangeNotifier` idiom `ModalInner::relay` uses. `proxy` alone cannot
    /// tell the shuttle to rebuild when a gesture ends (nothing about the
    /// animation itself changed then); this is what gives a status parked
    /// mid-gesture a rebuild to drain, the moment the gesture ends.
    wake: Arc<ChangeNotifier>,
    /// The forwarding subscription feeding [`wake`](Self::wake) from
    /// [`proxy`](Self::proxy)'s own value changes.
    proxy_wake_subscription: Mutex<Option<ListenerId>>,
    /// The forwarding subscription feeding [`wake`](Self::wake) from
    /// [`gesture_signal`](Self::gesture_signal)'s notifier — the same
    /// listener that replays a terminal status parked mid-gesture. Registered
    /// once at [`FlightManager::start`] and removed in [`HeroFlight::finish`],
    /// or — if this flight is ever dropped without going through `finish` —
    /// in `FlightInner`'s own `Drop` impl below. Never re-registered, because one
    /// listener for the flight's whole life costs nothing and needs no extra
    /// "already scheduled" bookkeeping.
    ///
    /// **Why this one specifically needs the `Drop` backstop and the others
    /// on this struct do not:** it is registered on
    /// [`gesture_signal`](Self::gesture_signal)'s notifier, which is owned by
    /// the *navigator*, not by this flight — so it outlives any one flight by
    /// construction. Every other subscription here targets `self.proxy`
    /// (owned by this same struct), which simply drops the registry along
    /// with itself; only a registration on someone else's longer-lived
    /// notifier can leak a closure this way.
    gesture_wake_subscription: Mutex<Option<ListenerId>>,
    /// Terminal status reported by the data-plane animation listener.
    ///
    /// `0` means "none", `1` means dismissed, and `2` means completed. The
    /// listener that writes this field must stay `Send + Sync`, so it cannot
    /// capture [`FlightInner`] or [`FlightManager`]. The owner-local shuttle
    /// drains the flag from `build`. Written only while no user gesture is in
    /// progress on this flight's navigator — a terminal status arriving mid-
    /// gesture is parked instead (see
    /// [`gesture_wake_subscription`](Self::gesture_wake_subscription)).
    settled_status: Arc<AtomicU8>,
    /// The in-flight widget, inflated once at start and rebuilt on a divert. Either the resolved `flight_shuttle_builder`'s output or, when none is
    /// set, a fresh copy of the destination hero's child.
    shuttle: Mutex<Option<BoxedView>>,
    /// The resolved `flight_shuttle_builder`, retained so a divert can rebuild
    /// the shuttle from the new destination. Behind a lock because a divert can
    /// swap it for the new manifest's builder.
    shuttle_builder: Mutex<Option<ShuttleBuilder>>,
}

impl FlightInner {
    /// Per-tick re-aim of the tween at the destination.
    ///
    /// The destination hero may move between the frame that measured it and the frame
    /// that lands on it — a rebuild above it, a scroll, a relayout. Every tick asks
    /// where it is *now*, and re-aims the tween at it.
    ///
    /// **`begin` is preserved.** The tween is re-created with the same `begin` and
    /// the re-read `end`: the shuttle keeps interpolating from where it started, not from where it
    /// currently is. Re-basing `begin` on the current rect would make the shuttle
    /// accelerate every time the destination twitched.
    fn on_tick(&self) {
        let destination = if self.aborted.load(Ordering::Relaxed) {
            None
        } else {
            let state = self.state.lock();
            let (to_hero, subtree) = (state.to_hero.clone(), state.to_route_subtree);
            drop(state);
            to_hero.bounding_box_in(subtree)
        };
        let origin = destination
            .map(|rect| (rect.min_x(), rect.min_y()))
            .filter(|(x, y)| x.is_finite() && y.is_finite());

        if let Some((x, y)) = origin {
            let mut rect = self.rect.lock();
            if rect.end.min_x() != x || rect.end.min_y() != y {
                // The *origin* is re-read, the size is the one that was measured.
                let size = rect.end.size();
                rect.end = Rect::from_ltwh(x, y, size.width, size.height);
            }
        } else {
            // The destination hero no longer exists or is no longer the flight's
            // destination. Continue flying while fading out.
            let mut fade_from = self.fade_from.lock();
            if fade_from.is_none() {
                *fade_from = Some(self.proxy.value());
            }
        }
        self.aborted.store(origin.is_none(), Ordering::Relaxed);

        // Fades out over `Interval(fade_from, 1.0)`, so the opacity is
        // `1 - interval(t)`.
        let fade_from = *self.fade_from.lock();
        let opacity = match fade_from {
            Some(from) => 1.0 - Interval::linear(from, 1.0).transform(self.proxy.value()),
            None => 1.0,
        };
        *self.opacity.lock() = opacity;
    }

    /// The rect the shuttle occupies right now, in the theater's coordinate space.
    ///
    /// Interpolated through the `create_rect_tween` factory when one is set,
    /// re-created each read from the current endpoints. `None` is the linear
    /// default.
    fn current_rect(&self) -> Rect {
        let endpoints = *self.rect.lock();
        let t = self.proxy.value();
        match self.rect_factory.lock().as_ref() {
            Some(make) => make(endpoints.begin, endpoints.end).transform(t),
            None => endpoints.transform(t),
        }
    }

    fn take_settled_status(&self) -> Option<AnimationStatus> {
        match self.settled_status.swap(0, Ordering::AcqRel) {
            1 => Some(AnimationStatus::Dismissed),
            2 => Some(AnimationStatus::Completed),
            _ => None,
        }
    }
}

impl Drop for FlightInner {
    /// A flight normally tears down through [`HeroFlight::finish`], which
    /// removes every subscription this struct opened. But nothing guarantees
    /// `finish` ever runs before the last `Arc<FlightInner>` drops — a
    /// `FlightManager` torn down with a flight still airborne, or every
    /// `HeroController`/observer holding one detached mid-flight, both drop
    /// this struct directly. Without this, [`gesture_wake_subscription`]'s
    /// closure (and everything it captured — a clone of this very `Arc`'s
    /// dependencies) would stay registered in the *navigator's* longer-lived
    /// notifier forever: a leak, not merely a stale listener, because nothing
    /// else will ever remove it.
    ///
    /// Idempotent with `finish`, in either order: each subscription is an
    /// `Option` a prior teardown already took, so a second attempt here (or
    /// there) finds `None` and does nothing.
    ///
    /// [`gesture_wake_subscription`]: FlightInner::gesture_wake_subscription
    fn drop(&mut self) {
        let status_id = self.subscriptions.lock().take();
        if let Some(status_id) = status_id {
            self.proxy.remove_status_listener(status_id);
        }
        let id = self.proxy_wake_subscription.lock().take();
        if let Some(id) = id {
            self.proxy.remove_listener(id);
        }
        let id = self.gesture_wake_subscription.lock().take();
        if let Some(id) = id {
            self.gesture_signal.notifier().remove_listener(id);
        }
    }
}

/// The in-flight widget: the resolved `flight_shuttle_builder`'s output, or — when none
/// is set — a fresh copy of the destination hero's child.
///
/// The builder receives the two hero child views (not foreign `BuildContext`s);
/// `animation` is the manifest's curved route animation, not the (possibly
/// reversed) proxy.
fn inflate_shuttle(
    builder: Option<&ShuttleBuilder>,
    animation: &Arc<dyn Animation<f64>>,
    direction: FlightDirection,
    from_hero: &HeroHandle,
    to_hero: &HeroHandle,
) -> BoxedView {
    match builder {
        Some(build) => build(
            animation,
            direction,
            &from_hero.shuttle_child(),
            &to_hero.shuttle_child(),
        ),
        None => to_hero.shuttle_child(),
    }
}

/// One hero in flight.
///
/// `pub` only so `crate::__test_access` can re-export it (ADR-0083 §4); the
/// module is private, so nothing else names it.
#[derive(Clone)]
pub struct HeroFlight {
    inner: Arc<FlightInner>,
}

impl std::fmt::Debug for HeroFlight {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeroFlight")
            .field("tag", &self.inner.tag)
            .finish_non_exhaustive()
    }
}

impl HeroFlight {
    pub(crate) fn tag(&self) -> &HeroTag {
        &self.inner.tag
    }

    /// The overlay entry this flight presents its shuttle in, while it has one.
    #[must_use]
    pub fn entry_id(&self) -> Option<crate::OverlayEntryId> {
        self.inner.entry.lock().as_ref().map(OverlayEntry::id)
    }

    /// The tween's current evaluation — where the shuttle is.
    #[must_use]
    pub fn shuttle_rect(&self) -> Rect {
        self.inner.current_rect()
    }

    /// The tween's destination, re-aimed by every tick.
    #[must_use]
    pub fn target_rect(&self) -> Rect {
        self.inner.rect.lock().end
    }

    /// The tween's origin. Re-aiming the destination must never move it.
    #[must_use]
    pub fn begin_rect(&self) -> Rect {
        self.inner.rect.lock().begin
    }

    /// The shuttle's current opacity.
    #[must_use]
    pub fn opacity(&self) -> f64 {
        *self.inner.opacity.lock()
    }

    /// Which way the flight currently runs — a divert can flip it.
    #[must_use]
    pub fn direction(&self) -> FlightDirection {
        self.inner.state.lock().direction
    }

    /// Whether this is a gesture-driven pop whose proxy never left
    /// `Dismissed`, as `HeroController::did_stop_user_gesture` checks: the drag
    /// never moved, so no status transition ever fired to report it, and
    /// nothing else will end this flight on its own.
    fn is_stalled_gesture_pop(&self) -> bool {
        let state = self.inner.state.lock();
        state.is_user_gesture_transition
            && state.direction == FlightDirection::Pop
            && self.inner.proxy.is_dismissed()
    }

    /// Tear this flight down and hand the two heroes back for the caller to
    /// decide each placeholder's fate — the terminal animation status picks that,
    /// and [`finish`](Self::finish) and [`abort`](Self::abort) disagree.
    ///
    /// Idempotent: detaching the proxy re-fires its status listener, and a
    /// diverted flight is ended by the manager before its own listener would.
    fn teardown(&self) -> Option<(HeroHandle, HeroHandle)> {
        if self.inner.ended.swap(true, Ordering::SeqCst) {
            return None;
        }

        let status_id = self.inner.subscriptions.lock().take();
        if let Some(status_id) = status_id {
            self.inner.proxy.remove_status_listener(status_id);
        }
        let id = self.inner.proxy_wake_subscription.lock().take();
        if let Some(id) = id {
            self.inner.proxy.remove_listener(id);
        }
        let id = self.inner.gesture_wake_subscription.lock().take();
        if let Some(id) = id {
            self.inner.gesture_signal.notifier().remove_listener(id);
        }

        if let Some(entry) = self.inner.entry.lock().take()
            && entry.is_attached()
        {
            entry.remove();
        }

        let state = self.inner.state.lock();
        Some((state.from_hero.clone(), state.to_hero.clone()))
    }

    /// End the flight on a terminal animation status, minus the ended callback —
    /// the manager does that half.
    fn finish(&self, status: AnimationStatus) {
        let Some((from_hero, to_hero)) = self.teardown() else {
            return;
        };

        // If completed, the destination hero is the one on top and the source hero
        // stays hidden. If dismissed, the animation was triggered but canceled
        // before it finished; the destination hero stays hidden instead.
        from_hero.end_flight(status.is_completed());
        to_hero.end_flight(status.is_dismissed());
    }

    /// Tear this flight down with no terminal animation status to decide the
    /// heroes' fate — used when its controller is detached (replaced or removed
    /// by `NavigatorHandle::add_observer` / `remove_observer`) while the
    /// navigator, and therefore both heroes, stay alive.
    ///
    /// Unlike [`finish`](Self::finish), **both** heroes restore their children:
    /// the flight is abandoned rather than undone in a particular direction, so
    /// neither page keeps a blank placeholder where its hero was.
    ///
    /// This exists because a controller can be replaced while the navigator lives;
    /// were the controller owned by the navigator for its whole life, leaving the
    /// placeholders frozen would be harmless, since the tree would be torn down
    /// anyway. Recorded in `ARCHITECTURE.md`.
    fn abort(&self) {
        let Some((from_hero, to_hero)) = self.teardown() else {
            return;
        };
        from_hero.end_flight(false);
        to_hero.end_flight(false);
    }

    /// A second transition for this tag started while the flight was airborne. Redirect the **same** flight — same
    /// object, same overlay entry — rather than end it and start a fresh one.
    ///
    /// Called from `FlightManager::start`, i.e. from the measurement pass, never from a
    /// status listener. It still must not hold a flight lock across
    /// [`ProxyAnimation::set_parent`], which fires `on_tick` synchronously; so every
    /// branch computes first, mutates the guarded fields, and repoints the proxy
    /// **last** with no lock held.
    fn divert(&self, new: &HeroFlightManifest, plan: FlightPlan) {
        let FlightPlan {
            direction: new_dir,
            from_hero: new_from,
            to_hero: new_to,
            to_route_subtree: new_subtree,
            overlay: _,
            animation: new_anim,
            rect_factory: new_rect_factory,
            shuttle_builder: new_shuttle_builder,
            is_user_gesture_transition: new_is_user_gesture_transition,
            // Fixed for the flight's whole life (see `FlightInner::gesture_signal`'s
            // doc) — every divert stays within the same controller/navigator, so
            // there is nothing to repoint here.
            gesture_signal: _,
        } = plan;

        let (old_dir, old_from, old_to) = {
            let state = self.inner.state.lock();
            (
                state.direction,
                state.from_hero.clone(),
                state.to_hero.clone(),
            )
        };

        // The new parent for the proxy animation, the new rect endpoints, and whether the
        // shuttle is rebuilt — decided per branch, applied afterwards.
        let new_parent: Arc<dyn Animation<f64>>;
        let (new_begin, new_end): (Rect, Rect);
        let mut new_shuttle: Option<BoxedView> = None;

        match (old_dir, new_dir) {
            // A push flight was interrupted by a pop.
            (FlightDirection::Push, FlightDirection::Pop) => {
                debug_assert!(
                    old_from.is_same(&new_to) && old_to.is_same(&new_from),
                    "BUG: a push→pop divert must reverse the same two heroes \
                     (heroes.dart:744-745)"
                );
                // The proxy's parent becomes the reverse of the new animation.
                new_parent = Arc::new(ReverseAnimation::new(new_anim));
                // The tween is reversed. FLUI has only a **linear** `RectTween`, for
                // which reversing the tween and swapping begin/end are identical
                // (`lerp(a,b,1-t) == lerp(b,a,t)`). A non-linear path (an arc tween)
                // would need a real reversed tween to stay symmetric; when an arc
                // tween lands, this must become one.
                let rect = self.inner.rect.lock();
                new_begin = rect.end;
                new_end = rect.begin;
                // Same heroes keep flying: no placeholder changes.
            }

            // A pop flight was interrupted by a push.
            (FlightDirection::Pop, FlightDirection::Push) => {
                debug_assert!(
                    old_to.is_same(&new_from),
                    "BUG: a pop→push divert keeps the old destination as the new source \
                     (heroes.dart:766)"
                );
                // The proxy's parent becomes the new animation driven from the old
                // animation's value to `1.0`. The begin is the **old manifest animation's** value. A pop flight's
                // proxy is a `ReverseAnimation` over it (every branch that sets
                // `direction = Pop` does so), so that value is `1 − proxy` — using the
                // proxy's own value here reads mirrored progress and teleports the
                // shuttle unless the divert happens at exactly the halfway point.
                let begin = 1.0 - self.inner.proxy.value();
                new_parent = Arc::new(animate(Tween { begin, end: 1.0 }, new_anim));

                if old_from.is_same(&new_to) {
                    // Same hero: begin from the old end, end at the old
                    // begin — the reverse of the reverse, without a new destination.
                    let rect = self.inner.rect.lock();
                    new_begin = rect.end;
                    new_end = rect.begin;
                } else {
                    // Different hero: hand the old source its placeholder back and freeze the
                    // new destination, then aim from the old end at the new location.
                    old_from.end_flight(true);
                    new_to.start_flight(false);
                    new_begin = self.inner.rect.lock().end;
                    new_end = new.to_rect;
                }
            }

            // A push or a pop flight is heading to a new route: push→push or
            // pop→pop, all four heroes distinct.
            (_, _) => {
                debug_assert!(
                    !old_from.is_same(&new_from) && !old_to.is_same(&new_to),
                    "BUG: a same-direction divert connects four distinct heroes \
                     (heroes.dart:786-787)"
                );
                // Begin from where the shuttle is right now; end at the new
                // destination's location.
                new_begin = self.inner.current_rect();
                new_end = new.to_rect;

                // The shuttle builder wants the raw route animation, so clone before the
                // proxy parent takes ownership below.
                let shuttle_animation = Arc::clone(&new_anim);
                new_parent = match new_dir {
                    FlightDirection::Pop => Arc::new(ReverseAnimation::new(new_anim)),
                    FlightDirection::Push => new_anim,
                };

                // End the old heroes' flights keeping their placeholders, then start
                // the new heroes' flights.
                old_from.end_flight(true);
                old_to.end_flight(true);
                new_from.start_flight(new_dir == FlightDirection::Push);
                new_to.start_flight(false);

                // Rebuild the shuttle from the new destination, through the new
                // manifest's shuttle builder if it set one, else the default fresh
                // child.
                new_shuttle = Some(inflate_shuttle(
                    new_shuttle_builder.as_ref(),
                    &shuttle_animation,
                    new_dir,
                    &new_from,
                    &new_to,
                ));
            }
        }

        // Apply the guarded fields — locks released before the proxy repoint below.
        {
            let mut rect = self.inner.rect.lock();
            rect.begin = new_begin;
            rect.end = new_end;
        }
        *self.inner.fade_from.lock() = None;
        self.inner.aborted.store(false, Ordering::Relaxed);
        // Re-read the new manifest's hooks: a divert can swap the destination hero
        // and, with it, its `create_rect_tween` / `flight_shuttle_builder`. The
        // same-direction branch above already rebuilt the shuttle with the new
        // builder; the other branches keep the existing shuttle, so the stored
        // builder only matters for a later same-tag divert.
        let _prev = std::mem::replace(&mut *self.inner.rect_factory.lock(), new_rect_factory);
        let _prev = std::mem::replace(&mut *self.inner.shuttle_builder.lock(), new_shuttle_builder);
        if let Some(shuttle) = new_shuttle.take() {
            let _prev = self.inner.shuttle.lock().replace(shuttle);
        }
        {
            let mut state = self.inner.state.lock();
            state.direction = new_dir;
            state.from_hero = new_from;
            state.to_hero = new_to;
            state.to_route_subtree = new_subtree;
            state.is_user_gesture_transition = new_is_user_gesture_transition;
        }

        // `manifest = newManifest` is the last line of `divert`; the proxy repoint is
        // the visible effect. No flight lock is held here, so the `on_tick` it fires
        // reads the state just written.
        self.inner.proxy.set_parent(new_parent);

        // Rebuild the overlay entry for the replaced shuttle. Harmless for the
        // other branches, but only the same-direction branch changed it.
        if let Some(entry) = self.inner.entry.lock().as_ref() {
            entry.mark_needs_build();
        }
    }
}

/// Everything `FlightManager::start` needs that the manifest does not carry.
///
/// A bundle rather than six parameters: the manifest is pure recorded data, and
/// these are the live capabilities the flight will drive.
pub(crate) struct FlightPlan {
    pub(crate) direction: FlightDirection,
    pub(crate) from_hero: HeroHandle,
    pub(crate) to_hero: HeroHandle,
    /// The destination route's coordinate root, for the per-tick re-measure.
    pub(crate) to_route_subtree: RenderId,
    pub(crate) overlay: OverlayHandle,
    /// The destination route's primary animation for a push, the source route's
    /// for a pop, already wrapped in the manifest's `CurvedAnimation` on the
    /// driving hero's `curve`/`reverse_curve`.
    pub(crate) animation: Arc<dyn Animation<f64>>,
    /// The resolved `create_rect_tween` factory: the destination hero's, else the
    /// controller's default, else `None` (linear).
    pub(crate) rect_factory: Option<RectTweenFactory>,
    /// The resolved `flight_shuttle_builder`: the destination hero's, else the
    /// source hero's, else `None` (default shuttle).
    pub(crate) shuttle_builder: Option<ShuttleBuilder>,
    /// Whether this transition was started by `did_start_user_gesture` rather
    /// than a programmatic push/pop.
    pub(crate) is_user_gesture_transition: bool,
    /// A Send+Sync-safe read of the navigator's user-gesture state, for the
    /// terminal-status deferral.
    pub(crate) gesture_signal: UserGestureSignal,
}

/// The flights in the air, one per tag, plus the deferred-drop discipline a
/// garbage-collected runtime would not need.
///
/// # Why flights are retired rather than dropped
///
/// A flight ends from inside its own `ProxyAnimation` status listener.
/// `ProxyAnimation::fan_out_status` snapshots the callbacks and then iterates them
/// while holding `&self` — so dropping the last `Arc<FlightInner>`, and with it the
/// proxy, *inside* that callback would free the animation the callback is running
/// under.
///
/// So `finish` never drops: it moves the flight into `retired` and
/// schedules a drain through the binding's [`LocalPostFrameHandle`]. That runs at
/// **end-of-frame** — after the status listener has returned and `fan_out_status` has
/// unwound, but within the same turn — so a single transition cleans up after itself
/// without waiting for an unrelated hero measurement. The drain is coalesced: many
/// flights landing in one frame schedule exactly one.
///
/// `drain_retired` is still called at the head of every
/// measurement pass, as a backstop for the case where no post-frame capability was
/// captured (an unmounted navigator, which is being torn down anyway).
///
/// `pub` only so `crate::__test_access` can re-export it (ADR-0083 §4).
#[derive(Default)]
pub struct FlightManager {
    flights: Mutex<HashMap<HeroTag, HeroFlight>>,
    retired: Mutex<Vec<HeroFlight>>,
    /// The binding's post-frame capability, captured from the controller. A finished
    /// flight schedules its own end-of-frame drain through this, so cleanup does not
    /// wait for the next transition. `None` before the first launch or on an unmounted
    /// navigator — then the measurement-head backstop is the only path.
    post_frame: Mutex<Option<LocalPostFrameHandle>>,
    /// One drain per frame: set when a drain is scheduled, cleared when it runs.
    drain_scheduled: AtomicBool,
    /// How many drains this manager has actually scheduled — for the coalescing
    /// test. Compiled into every build so the manager has one layout whether or
    /// not the integration tests link it (ADR-0083 §4).
    drains_scheduled: std::sync::atomic::AtomicUsize,
}

impl std::fmt::Debug for FlightManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FlightManager")
            .field("in_flight", &self.flights.lock().len())
            .field("retired", &self.retired.lock().len())
            .finish_non_exhaustive()
    }
}

impl FlightManager {
    /// Free everything the retired flights were holding. Runs from the end-of-frame
    /// drain a landing flight scheduled, and as a backstop from the measurement pass —
    /// never from an animation listener.
    pub(crate) fn drain_retired(&self) {
        let retired = std::mem::take(&mut *self.retired.lock());
        drop(retired);
    }

    /// Capture the binding's post-frame capability, so a finished flight can schedule
    /// its own drain. Set from the controller's measurement pass, where the navigator
    /// still resolves it.
    pub(crate) fn set_post_frame(&self, handle: Option<LocalPostFrameHandle>) {
        *self.post_frame.lock() = handle;
    }

    /// How many flights are parked awaiting a safe drop.
    #[must_use]
    pub fn retired_count(&self) -> usize {
        self.retired.lock().len()
    }

    /// How many end-of-frame drains have been scheduled — coalescing must keep this at
    /// one per frame no matter how many flights land.
    #[must_use]
    pub fn drains_scheduled(&self) -> usize {
        self.drains_scheduled.load(Ordering::SeqCst)
    }

    /// Queue a single end-of-frame drain of [`retired`](Self::retired).
    ///
    /// **Not re-entrant.** `LocalPostFrameHandle::schedule_local` only pushes onto the lane's
    /// post-frame queue; the closure runs at `end_frame`, long after `fan_out_status`
    /// has returned, so nothing here drops a flight while its listener is still on the
    /// stack. The closure holds a `Weak`: a manager dropped before the frame ends
    /// simply takes its retired flights with it.
    fn schedule_drain(self: &Arc<Self>) {
        let Some(post_frame) = self.post_frame.lock().clone() else {
            return; // No binding capability; the measurement-head drain is the backstop.
        };
        if self.drain_scheduled.swap(true, Ordering::SeqCst) {
            return; // Already scheduled this frame — coalesce.
        }
        let weak = Arc::downgrade(self);
        let schedule_result = post_frame.schedule_local(move |_timing| {
            if let Some(this) = weak.upgrade() {
                this.drain_scheduled.store(false, Ordering::SeqCst);
                this.drain_retired();
            }
        });
        if let Err(error) = schedule_result {
            self.drain_scheduled.store(false, Ordering::SeqCst);
            tracing::warn!(
                ?error,
                "retired hero flights remain queued because the owner-local post-frame lane is inactive"
            );
        } else {
            self.drains_scheduled.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// How many flights are in the air.
    #[must_use]
    #[expect(
        clippy::len_without_is_empty,
        reason = "a test-facing count; nothing asks whether the manager is empty"
    )]
    pub fn len(&self) -> usize {
        self.flights.lock().len()
    }

    /// The flight for `tag`, if any.
    #[must_use]
    pub fn get(&self, tag: &HeroTag) -> Option<HeroFlight> {
        self.flights.lock().get(tag).cloned()
    }

    /// Whether a flight for `tag` is already in the air, i.e. the next manifest
    /// for it is a divert. A diverted manifest's animation carries no reverse
    /// curve.
    pub(crate) fn is_airborne(&self, tag: &HeroTag) -> bool {
        self.flights.lock().contains_key(tag)
    }

    /// Start a flight, or — when a flight for this tag is already airborne —
    /// divert it.
    ///
    /// `plan.animation` is the **destination** route's primary animation for a
    /// push, the **source** route's for a pop.
    pub(crate) fn start(self: &Arc<Self>, manifest: &HeroFlightManifest, plan: FlightPlan) {
        // Divert redirects the airborne flight in place, keeping its one overlay
        // entry, rather than an end-and-restart. The flight stays in the map under
        // its tag.
        let existing = self.flights.lock().get(&manifest.tag).cloned();
        if let Some(existing) = existing {
            existing.divert(manifest, plan);
            return;
        }

        let FlightPlan {
            direction,
            from_hero,
            to_hero,
            to_route_subtree,
            overlay,
            animation,
            rect_factory,
            shuttle_builder,
            is_user_gesture_transition,
            gesture_signal,
        } = plan;

        // The shuttle builder gets `manifest.animation` — the curved route animation, not
        // the (possibly reversed) proxy — so keep a clone before the proxy takes ownership.
        let shuttle_animation = Arc::clone(&animation);

        // The proxy's parent is the reverse of the animation for a pop, the
        // animation itself for a push.
        let parent: Arc<dyn Animation<f64>> = match direction {
            FlightDirection::Push => animation,
            FlightDirection::Pop => Arc::new(ReverseAnimation::new(animation)),
        };

        let inner = Arc::new(FlightInner {
            tag: manifest.tag.clone(),
            state: Mutex::new(FlightState {
                direction,
                from_hero: from_hero.clone(),
                to_hero: to_hero.clone(),
                to_route_subtree,
                is_user_gesture_transition,
            }),
            proxy: Arc::new(ProxyAnimation::new(parent)),
            rect: Mutex::new(RectTween {
                begin: manifest.from_rect,
                end: manifest.to_rect,
            }),
            rect_factory: Mutex::new(rect_factory),
            opacity: Mutex::new(1.0),
            fade_from: Mutex::new(None),
            aborted: AtomicBool::new(false),
            ended: AtomicBool::new(false),
            entry: Mutex::new(None),
            subscriptions: Mutex::new(None),
            gesture_signal: gesture_signal.clone(),
            wake: Arc::new(ChangeNotifier::new()),
            proxy_wake_subscription: Mutex::new(None),
            gesture_wake_subscription: Mutex::new(None),
            settled_status: Arc::new(AtomicU8::new(0)),
            shuttle: Mutex::new(None),
            shuttle_builder: Mutex::new(shuttle_builder),
        });

        // The child stays in the placeholder only for the *from* hero of a push:
        // its subtree is preserved offstage so its state survives.
        from_hero.start_flight(direction == FlightDirection::Push);
        to_hero.start_flight(false);

        // The resolved `flight_shuttle_builder`'s output, or — the default — a fresh copy
        // of the **destination** hero's child. Nothing is reparented.
        *inner.shuttle.lock() = Some(inflate_shuttle(
            inner.shuttle_builder.lock().as_ref(),
            &shuttle_animation,
            direction,
            &from_hero,
            &to_hero,
        ));

        let entry = {
            let inner = Arc::clone(&inner);
            let manager = Arc::downgrade(self);
            OverlayEntry::new(move |_ctx| {
                Shuttle {
                    flight: Arc::clone(&inner),
                    manager: manager.clone(),
                }
                .boxed()
            })
        };
        overlay.insert(&entry, &InsertPosition::Top);
        let _prev = inner.entry.lock().replace(entry);

        let flight = HeroFlight {
            inner: Arc::clone(&inner),
        };

        // The per-tick `on_tick` is served by the shuttle's `AnimatedView`
        // rebuild: every value tick marks the owner-local subtree
        // dirty, and `ShuttleState::build` runs `on_tick` before reading the rect.
        // The shuttle listens to `wake`, not `proxy` directly — forward every proxy
        // tick into it so that stays true.
        let proxy_to_wake = Arc::clone(&inner.wake);
        let proxy_wake_id = inner
            .proxy
            .add_listener(Arc::new(move || proxy_to_wake.notify_listeners()));
        *inner.proxy_wake_subscription.lock() = Some(proxy_wake_id);

        // The status listener must remain a data-plane callback. It records only a
        // tiny terminal-status flag; the owner-local shuttle drains the flag and
        // calls back into the manager.
        //
        // While a user gesture is in progress on this flight's navigator, a terminal status is *not*
        // recorded here — the gesture-notifier listener registered below (armed
        // once, for the flight's whole life) picks it up when the gesture ends,
        // reading the proxy's status fresh at that time rather than trusting
        // whatever it was at the moment it was skipped.
        let settled_status = Arc::clone(&inner.settled_status);
        let listener_gesture_signal = gesture_signal.clone();
        let status_id = inner.proxy.add_status_listener(Arc::new(move |status| {
            // Only terminal statuses matter: forward/reverse is exactly the
            // complement of dismissed/completed.
            if listener_gesture_signal.in_progress() {
                return;
            }
            match status {
                AnimationStatus::Dismissed => settled_status.store(1, Ordering::Release),
                AnimationStatus::Completed => settled_status.store(2, Ordering::Release),
                _ => {}
            }
        }));
        *inner.subscriptions.lock() = Some(status_id);

        // The deferred-replay half of the gesture deferral: fires on every 0→1/1→0 transition of the navigator's gesture state, for the
        // flight's whole life. Must stay `Send + Sync` exactly like the status
        // listener above — it can only touch the same data-plane primitives
        // (`proxy`, `settled_status`, `wake`, all `Send + Sync`), never
        // `Arc<FlightInner>`/`Arc<FlightManager>` as a whole (both hold owner-local,
        // `Rc`-based view state). Writing `settled_status` alone would not be seen:
        // nothing about `proxy` itself changed on a gesture-end, so the owner-local
        // shuttle needs telling to rebuild and drain it — hence also waking.
        let settled_status = Arc::clone(&inner.settled_status);
        let proxy_for_replay = Arc::clone(&inner.proxy);
        let gesture_to_wake = Arc::clone(&inner.wake);
        let replay_gesture_signal = gesture_signal.clone();
        let gesture_wake_id = gesture_signal.notifier().add_listener(Arc::new(move || {
            if !replay_gesture_signal.in_progress() {
                // Read the status fresh, not whatever status was skipped when it
                // was parked.
                match proxy_for_replay.status() {
                    AnimationStatus::Dismissed => settled_status.store(1, Ordering::Release),
                    AnimationStatus::Completed => settled_status.store(2, Ordering::Release),
                    _ => {} // Still animating; nothing to replay.
                }
            }
            gesture_to_wake.notify_listeners();
        }));
        *inner.gesture_wake_subscription.lock() = Some(gesture_wake_id);

        let _prev = self.flights.lock().insert(manifest.tag.clone(), flight);
    }

    /// Drop the flight from the registry. Called from the flight's own status listener, so the flight
    /// is *retired*, not dropped — see the type docs.
    fn finish(self: &Arc<Self>, flight: &HeroFlight, status: AnimationStatus) {
        flight.finish(status);
        self.retire(flight);
    }

    /// [`HeroFlight::abort`], then the same retire-and-drain `finish` uses.
    fn abort(self: &Arc<Self>, flight: &HeroFlight) {
        flight.abort();
        self.retire(flight);
    }

    /// Drop the flight from the registry and park it for a safe end-of-frame
    /// drop. Called both from the flight's own status listener (`finish`) and
    /// from a detached controller's sweep (`abort`) — both park rather than
    /// drop, so the flight is freed outside any animation listener (see the
    /// type docs).
    fn retire(self: &Arc<Self>, flight: &HeroFlight) {
        let removed = self.flights.lock().remove(flight.tag());
        if let Some(removed) = removed {
            // Park it — we may be inside its status listener — and schedule the
            // drop for the end of this frame.
            self.retired.lock().push(removed);
            self.schedule_drain();
        }
    }

    /// Cancel every flight still in the air, restoring both heroes and removing
    /// every overlay entry.
    ///
    /// Called from `HeroController::did_detach` — when the controller is
    /// replaced by `NavigatorHandle::add_observer`, removed by
    /// `remove_observer`, or its navigator unmounts. A detached controller can
    /// no longer service a flight's end-of-flight drain (the shuttle retires a
    /// flight only through a live `FlightManager`), so leaving flights airborne
    /// would strand their overlay entries and their shuttle's painting forever.
    /// Owner-local, never from inside an animation listener, so aborting (which
    /// parks a flight) is safe here, unlike the data-plane status listeners in
    /// [`start`](Self::start).
    pub(crate) fn finish_all(self: &Arc<Self>) {
        let all: Vec<HeroFlight> = self.flights.lock().values().cloned().collect();
        for flight in all {
            self.abort(&flight);
        }
    }

    /// The manual sweep for `HeroController::did_stop_user_gesture`:
    /// every still-airborne, gesture-driven pop flight whose proxy never left
    /// `Dismissed` (the drag never moved) is fed a synthetic `Dismissed` update
    /// through the same [`finish`](Self::finish) path a real terminal status
    /// would use.
    ///
    /// Called directly from `HeroController::did_stop_user_gesture` — owner-local,
    /// never from inside an animation listener — so calling `finish` here (which
    /// may drop the flight) is unconditionally safe, unlike the data-plane status
    /// listeners in [`start`](Self::start).
    pub(crate) fn finish_stalled_gesture_pops(self: &Arc<Self>) {
        let stalled: Vec<HeroFlight> = self
            .flights
            .lock()
            .values()
            .filter(|flight| flight.is_stalled_gesture_pop())
            .cloned()
            .collect();
        for flight in stalled {
            self.finish(&flight, AnimationStatus::Dismissed);
        }
    }
}

// ============================================================================
// The shuttle
// ============================================================================

/// The overlay content of one flight.
///
/// An [`AnimatedView`] over the flight's `ProxyAnimation`, so every tick rebuilds it.
///
/// The inner `Stack` is **load-bearing**: `RenderTheater` runs no positioned split, so
/// a `Positioned` handed straight to an overlay entry has its parent data dropped and
/// lands at the origin.
#[derive(Clone)]
struct Shuttle {
    flight: Arc<FlightInner>,
    manager: Weak<FlightManager>,
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
    /// `self.flight.wake`, not `proxy` directly: the relay that also
    /// forwards the navigator's gesture-notifier, so a flight parked
    /// mid-gesture (`FlightInner::gesture_wake_subscription`) gets a rebuild
    /// the moment the gesture ends, to drain the status that was replayed.
    fn listenable(&self) -> Arc<dyn Listenable> {
        Arc::clone(&self.flight.wake) as Arc<dyn Listenable>
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
        view.flight.on_tick();
        if let Some(status) = view.flight.take_settled_status()
            && let Some(manager) = view.manager.upgrade()
        {
            manager.finish(
                &HeroFlight {
                    inner: Arc::clone(&view.flight),
                },
                status,
            );
        }

        let rect = view.flight.current_rect();
        let opacity = *view.flight.opacity.lock();
        let child = view
            .flight
            .shuttle
            .lock()
            .clone()
            .unwrap_or_else(|| crate::SizedBox::shrink().boxed());

        // `Positioned(… child: IgnorePointer(child: Opacity(…)))`. `Opacity`, not a
        // fade transition: the opacity is evaluated eagerly in
        // `on_tick`, so there is no second animation to subscribe to.
        Stack::new(vec![
            Positioned::new(
                IgnorePointer::new()
                    .ignoring(true)
                    .child(Opacity::new(opacity).child(child)),
            )
            .left(rect.min_x())
            .top(rect.min_y())
            .width(rect.width())
            .height(rect.height())
            .into_view()
            .boxed(),
        ])
        .fit(StackFit::Expand)
    }
}
