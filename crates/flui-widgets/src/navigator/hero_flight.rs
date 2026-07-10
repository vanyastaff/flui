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
//! * **Divert is private and implemented (U5.1).** A second transition for the same tag
//!   redirects the existing [`HeroFlight`] in place (`_HeroFlight.divert`, `:738-816`):
//!   same flight object, same overlay entry, new manifest-derived state.
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

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use flui_animation::{
    Animatable, Animation, AnimationStatus, Curve, Interval, ProxyAnimation, RectTween,
    ReverseAnimation, Tween, animate,
};
use flui_foundation::{Listenable, ListenerId, RenderId};
use flui_geometry::Rect;
use flui_scheduler::PostFrameHandle;
use flui_view::prelude::*;
use flui_view::{AnimatedView, BoxedView, ViewExt, impl_animated_view};
use parking_lot::Mutex;

use super::hero::{HeroHandle, HeroTag};
use super::hero_controller::{FlightDirection, HeroFlightManifest};
use crate::overlay::{InsertPosition, OverlayEntry, OverlayHandle};
use crate::{IgnorePointer, Opacity, Positioned, Stack, StackFit};

/// The `_HeroFlightManifest`-derived facts a divert can replace: which way the
/// flight runs, which two heroes it connects, and the coordinate space its
/// destination lives in (`heroes.dart:815` — `manifest = newManifest`).
///
/// Behind one `Mutex` so `on_tick`, `finish`, and `divert` all see a coherent set.
struct FlightState {
    direction: FlightDirection,
    from_hero: HeroHandle,
    to_hero: HeroHandle,
    /// The destination route's coordinate root, for the per-tick re-measure.
    to_route_subtree: RenderId,
}

/// Everything one in-flight hero shares between its overlay entry, its animation
/// listeners, and the manager that owns it.
struct FlightInner {
    tag: HeroTag,

    /// The half a divert rewrites in place — Flutter's `manifest = newManifest`.
    state: Mutex<FlightState>,

    /// `_HeroFlight._proxyAnimation` (`heroes.dart:557`): the animation the shuttle
    /// reads, already reversed for a pop. Its **parent** is repointed by a divert;
    /// the proxy object itself, and the listeners on it, never change.
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
            let state = self.state.lock();
            let (to_hero, subtree) = (state.to_hero.clone(), state.to_route_subtree);
            drop(state);
            to_hero.bounding_box_in(subtree)
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

    /// Which way the flight currently runs — a divert can flip it.
    #[cfg(test)]
    pub(crate) fn direction(&self) -> FlightDirection {
        self.inner.state.lock().direction
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
        let (from_hero, to_hero) = {
            let state = self.inner.state.lock();
            (state.from_hero.clone(), state.to_hero.clone())
        };
        from_hero.end_flight(status.is_completed());
        to_hero.end_flight(status.is_dismissed());
    }

    /// `_HeroFlight.divert` (`heroes.dart:740-816`): a second transition for this tag
    /// started while the flight was airborne. Redirect the **same** flight — same
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
        } = plan;

        let (old_dir, old_from, old_to) = {
            let state = self.inner.state.lock();
            (
                state.direction,
                state.from_hero.clone(),
                state.to_hero.clone(),
            )
        };

        // The new parent for `_proxyAnimation`, the new rect endpoints, and whether the
        // shuttle is rebuilt — decided per branch, applied afterwards.
        let new_parent: Arc<dyn Animation<f32>>;
        let (new_begin, new_end): (Rect, Rect);
        let mut new_shuttle: Option<BoxedView> = None;

        match (old_dir, new_dir) {
            // "A push flight was interrupted by a pop." (`heroes.dart:742-757`)
            (FlightDirection::Push, FlightDirection::Pop) => {
                debug_assert!(
                    old_from.is_same(&new_to) && old_to.is_same(&new_from),
                    "BUG: a push→pop divert must reverse the same two heroes \
                     (heroes.dart:744-745)"
                );
                // `_proxyAnimation.parent = ReverseAnimation(newManifest.animation)`.
                new_parent = Arc::new(ReverseAnimation::new(new_anim));
                // `heroRectTween = ReverseTween<Rect?>(heroRectTween)`. FLUI has only a
                // **linear** `RectTween`, for which reversing the tween and swapping
                // begin/end are identical (`lerp(a,b,1-t) == lerp(b,a,t)`). Flutter uses
                // `ReverseTween` only to keep a non-linear path (`MaterialRectArcTween`)
                // symmetric; when an arc tween lands, this must become a real
                // `ReverseTween`. Divergence recorded in ADR-0021 §7j.
                let rect = self.inner.rect.lock();
                new_begin = rect.end;
                new_end = rect.begin;
                // Same heroes keep flying: no placeholder changes.
            }

            // "A pop flight was interrupted by a push." (`heroes.dart:758-780`)
            (FlightDirection::Pop, FlightDirection::Push) => {
                debug_assert!(
                    old_to.is_same(&new_from),
                    "BUG: a pop→push divert keeps the old destination as the new source \
                     (heroes.dart:766)"
                );
                // `_proxyAnimation.parent = newManifest.animation.drive(
                //      Tween(begin: manifest.animation.value, end: 1.0))`.
                let begin = self.inner.proxy.value();
                new_parent = Arc::new(animate(Tween { begin, end: 1.0 }, new_anim));

                if old_from.is_same(&new_to) {
                    // "same hero" (`:772-777`): begin from the old end, end at the old
                    // begin — the reverse of the reverse, without a new destination.
                    let rect = self.inner.rect.lock();
                    new_begin = rect.end;
                    new_end = rect.begin;
                } else {
                    // `:767-771`: hand the old source its placeholder back and freeze the
                    // new destination, then aim from the old end at the new location.
                    old_from.end_flight(true);
                    new_to.start_flight(false);
                    new_begin = self.inner.rect.lock().end;
                    new_end = new.to_rect;
                }
            }

            // "A push or a pop flight is heading to a new route." (`heroes.dart:781-815`)
            // push→push or pop→pop, all four heroes distinct.
            (_, _) => {
                debug_assert!(
                    !old_from.is_same(&new_from) && !old_to.is_same(&new_to),
                    "BUG: a same-direction divert connects four distinct heroes \
                     (heroes.dart:786-787)"
                );
                // `begin: heroRectTween.evaluate(_proxyAnimation)` — from where the
                // shuttle is right now — `end: newManifest.toHeroLocation`.
                new_begin = self.inner.current_rect();
                new_end = new.to_rect;

                new_parent = match new_dir {
                    FlightDirection::Pop => Arc::new(ReverseAnimation::new(new_anim)),
                    FlightDirection::Push => new_anim,
                };

                // `manifest.fromHero.endFlight(keepPlaceholder: true)` + `toHero`, then
                // `newManifest.fromHero.startFlight(push?)` + `toHero.startFlight()`.
                old_from.end_flight(true);
                old_to.end_flight(true);
                new_from.start_flight(new_dir == FlightDirection::Push);
                new_to.start_flight(false);

                // `shuttle = null; overlayEntry!.markNeedsBuild();` — rebuild the shuttle
                // from the new destination's child.
                new_shuttle = Some(new_to.shuttle_child());
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
        if let Some(shuttle) = new_shuttle.take() {
            *self.inner.shuttle.lock() = Some(shuttle);
        }
        {
            let mut state = self.inner.state.lock();
            state.direction = new_dir;
            state.from_hero = new_from;
            state.to_hero = new_to;
            state.to_route_subtree = new_subtree;
        }

        // `manifest = newManifest` is the last line of `divert`; the proxy repoint is
        // the visible effect. No flight lock is held here, so the `on_tick` it fires
        // reads the state just written.
        self.inner.proxy.set_parent(new_parent);

        // `overlayEntry!.markNeedsBuild()` for the cleared shuttle (`:813`). Harmless
        // for the other branches, but only the same-direction branch changed it.
        if let Some(entry) = self.inner.entry.lock().as_ref() {
            entry.mark_needs_build();
        }
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
/// So `finish` never drops: it moves the flight into [`retired`](Self::retired) and
/// schedules a drain through the binding's [`PostFrameHandle`]. That runs at
/// **end-of-frame** — after the status listener has returned and `fan_out_status` has
/// unwound, but within the same turn — so a single transition cleans up after itself
/// without waiting for an unrelated hero measurement. The drain is coalesced: many
/// flights landing in one frame schedule exactly one.
///
/// [`drain_retired`](Self::drain_retired) is still called at the head of every
/// measurement pass, as a backstop for the case where no post-frame capability was
/// captured (an unmounted navigator, which is being torn down anyway).
#[derive(Default)]
pub(crate) struct FlightManager {
    flights: Mutex<HashMap<HeroTag, HeroFlight>>,
    retired: Mutex<Vec<HeroFlight>>,
    /// The binding's post-frame capability, captured from the controller. A finished
    /// flight schedules its own end-of-frame drain through this, so cleanup does not
    /// wait for the next transition. `None` before the first launch or on an unmounted
    /// navigator — then the measurement-head backstop is the only path.
    post_frame: Mutex<Option<PostFrameHandle>>,
    /// One drain per frame: set when a drain is scheduled, cleared when it runs.
    drain_scheduled: AtomicBool,
    /// How many drains this manager has actually scheduled — for the coalescing test.
    #[cfg(test)]
    drains_scheduled: std::sync::atomic::AtomicUsize,
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
    pub(crate) fn set_post_frame(&self, handle: Option<PostFrameHandle>) {
        *self.post_frame.lock() = handle;
    }

    /// How many flights are parked awaiting a safe drop.
    #[cfg(test)]
    pub(crate) fn retired_count(&self) -> usize {
        self.retired.lock().len()
    }

    /// How many end-of-frame drains have been scheduled — coalescing must keep this at
    /// one per frame no matter how many flights land.
    #[cfg(test)]
    pub(crate) fn drains_scheduled(&self) -> usize {
        self.drains_scheduled.load(Ordering::SeqCst)
    }

    /// Queue a single end-of-frame drain of [`retired`](Self::retired).
    ///
    /// **Not re-entrant.** `PostFrameHandle::schedule` only pushes onto the scheduler's
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
        #[cfg(test)]
        self.drains_scheduled.fetch_add(1, Ordering::SeqCst);

        let weak = Arc::downgrade(self);
        post_frame.schedule(move |_timing| {
            if let Some(this) = weak.upgrade() {
                this.drain_scheduled.store(false, Ordering::SeqCst);
                this.drain_retired();
            }
        });
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

    /// `_HeroFlight.start` (`heroes.dart:698-736`), or — when a flight for this tag is
    /// already airborne — `_HeroFlight.divert` (`:1051-1052`).
    ///
    /// `plan.animation` is `manifest.animation` (`:466-480`): the **destination**
    /// route's primary animation for a push, the **source** route's for a pop.
    pub(crate) fn start(self: &Arc<Self>, manifest: &HeroFlightManifest, plan: FlightPlan) {
        // `if (existingFlight != null) existingFlight.divert(manifest)` (`:1051-1052`):
        // U5.1 redirects the airborne flight in place, keeping its one overlay entry,
        // rather than the U4 end-and-restart. The flight stays in the map under its tag.
        if let Some(existing) = self.flights.lock().get(&manifest.tag).cloned() {
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
        } = plan;

        // `_proxyAnimation.parent = ReverseAnimation(manifest.animation)` for a pop,
        // `manifest.animation` for a push (`:719-724`).
        let parent: Arc<dyn Animation<f32>> = match direction {
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
            }),
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
    fn finish(self: &Arc<Self>, flight: &HeroFlight, status: AnimationStatus) {
        flight.finish(status);
        let removed = self.flights.lock().remove(flight.tag());
        if let Some(removed) = removed {
            // Park it — we are inside its status listener — and schedule the drop for
            // the end of this frame.
            self.retired.lock().push(removed);
            self.schedule_drain();
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
