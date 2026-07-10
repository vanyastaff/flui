//! [`HeroController`] — the measurement half of Flutter's hero machinery.
//!
//! ADR-0021 U3. **Private.** Nothing here is exported, and there is no `Hero`
//! widget: this is the observer that decides *when* a flight would start and
//! *where* its destination will be. The flight itself — hero discovery, the
//! overlay entry, `RectTween`, `flightShuttleBuilder` — is U4.
//!
//! # What this is a port of
//!
//! `.flutter/packages/flutter/lib/src/widgets/heroes.dart`, master
//! `3.33.0-0.0.pre-6280-g88e87cd963f`:
//!
//! * `HeroController.didChangeTop` (`:853-869`) → [`HeroController::did_change_top`].
//!   Note it overrides **only** `didChangeTop` — never `didPush`/`didPop`, which a
//!   port is tempted to reach for and which fire for routes that never become the
//!   top one;
//! * `_maybeStartHeroTransition` (`:911-976`) → [`HeroController::maybe_start`];
//! * `_startHeroTransition`'s prologue (`:979-1012`) → [`HeroController::measure`].
//!
//! The whole design rests on one comment (`:964-966`):
//!
//! > *Putting a route offstage changes its animation value to 1.0. Once this frame
//! > completes, we'll know where the heroes in the `to` route are going to end up,
//! > and the `to` route will go back onstage.*
//!
//! So the sequence is: flip the destination offstage → let the frame build, lay out
//! and commit → measure from a post-frame callback → put it back onstage. Every
//! piece of that is a seam this ADR built, and this controller is the first thing
//! that composes them:
//!
//! | Step | Seam | Landed in |
//! |---|---|---|
//! | `toRoute.offstage = …` (`:967`) | [`ModalHandle::set_offstage`] via the navigator's modal registry | U3 (this pass) |
//! | `didChangeTop` (`navigator.dart:4590-4596`) | `Notification::TopChanged`, delivered outside the history lock | U2 + §7f |
//! | offstage ⇒ `animation.value == 1.0` (`routes.dart:1958`) | the `ModalRoute` animation proxies | U3 (this pass) |
//! | `addPostFrameCallback` (`:968`) | [`PostFrameHandle`] | U2 |
//! | the callback runs *after* layout commits | `Scheduler::drive_frame` | U1.5 |
//! | `to.subtreeContext` (`:1014`) | [`RouteSubtree`] | U2 |
//! | `subtreeContext.findRenderObject()!.size` (`:952`) | `PipelineOwner::box_size` | U1 |
//! | `getTransformTo(navigatorRenderObject)` (`:1029`) | `PipelineOwner::transform_to` | U1 |
//! | `navigator` on an observer (`navigator.dart:779`) | [`NavigatorObserver::did_attach`] | U2 |
//!
//! # What is deliberately absent
//!
//! No `Hero` widget, no `_allHeroesFor`, no `_HeroFlight`, no overlay entry, no
//! `RectTween`, no `flightShuttleBuilder`, no `HeroControllerScope` — and therefore
//! **no nested-navigator support**. A `HeroController` observes exactly the one
//! navigator that attached it, as Flutter's does (`navigator.dart:3995-4046`).
//!
//! No `userGestureInProgress` either: FLUI has no back-swipe, so
//! `isUserGestureTransition` is always `false`. That collapses `didStartUserGesture`
//! / `didStopUserGesture` (`heroes.dart:871-889`) and the `hasValidSize` fast path
//! (`:952-960`) — which only ever runs for a gesture-driven pop — out of this port.
//! Both are recorded as absent, not as done.
//!
//! [`ModalHandle::set_offstage`]: super::modal_route::ModalHandle::set_offstage
//! [`PostFrameHandle`]: flui_scheduler::PostFrameHandle
//! [`RouteSubtree`]: super::subtree::RouteSubtree

// U3 is the measurement skeleton: `HeroController` measures, and U4's `Hero` widget
// flies. Until that widget lands the tests are this module's only callers, and
// `dead_code` cascades from here into the `ModalHandle` / `RouteBinding` seams it is
// the sole production consumer of. Deleting it and re-deriving it in U4 is how a
// seam stops matching the ADR that specified it.
#![allow(dead_code)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_animation::AnimationStatus;
use flui_geometry::Matrix4;
use flui_types::Size;
use parking_lot::Mutex;

use super::binding::TransitionGroup;
use super::modal_route::ModalHandle;
use super::navigator::NavigatorHandle;
use super::observer::NavigatorObserver;
use super::route::RouteId;

/// Which way a flight would run. Flutter's `HeroFlightDirection` (`heroes.dart:57`).
///
/// Derived from the two routes' animation **statuses**, not from which navigator
/// call happened (`heroes.dart:924-932`) — a pop and a push both arrive here as a
/// change of top route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FlightDirection {
    /// `to` is arriving on top of `from`: the destination is running forward.
    Push,
    /// `from` is leaving, revealing `to` beneath it: the source is running backward.
    Pop,
}

impl FlightDirection {
    /// `switch ((isUserGestureTransition, oldRouteAnimation.status,
    /// newRouteAnimation.status))` (`heroes.dart:924-932`), minus the gesture arm.
    ///
    /// `None` means "neither route is transitioning" — Flutter's `default: flightType
    /// = null`, which does **not** abort: the measurement still runs (`:934-976`).
    fn classify(from_status: AnimationStatus, to_status: AnimationStatus) -> Option<Self> {
        match (from_status, to_status) {
            (AnimationStatus::Reverse, _) => Some(Self::Pop),
            (_, AnimationStatus::Forward) => Some(Self::Push),
            _ => None,
        }
    }
}

/// What one post-frame measurement resolved.
///
/// This is what U4's `_HeroFlightManifest` will be built from. Today it is only
/// *recorded*, which is the point: it proves the U1/U1.5/U2/U3 seams compose into a
/// destination rect, without yet flying anything into it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Measurement {
    /// `None` when neither route was animating; see [`FlightDirection::classify`].
    pub(crate) direction: Option<FlightDirection>,
    pub(crate) from: RouteId,
    pub(crate) to: RouteId,
    /// `to.subtreeContext.findRenderObject()!.size` (`heroes.dart:952`). `None` when
    /// the destination has not laid out — which, after a frame, would be a bug.
    pub(crate) to_size: Option<Size>,
    /// `to.subtreeContext.findRenderObject()!.getTransformTo(navigatorRenderObject)`
    /// (`heroes.dart:1029`), taken against the render root rather than the
    /// navigator's own render object — FLUI's `Navigator` is not a render object.
    pub(crate) to_transform: Option<Matrix4>,
    /// What the destination's primary animation read *while it was offstage*. The
    /// whole mechanism is a lie unless this is `1.0` (`routes.dart:1958`).
    pub(crate) to_animation_while_offstage: f32,
}

/// Watches a navigator and measures where a hero flight *would* land.
///
/// Install with [`NavigatorHandle::add_observer`]. Holds no `GlobalKey`, reads no
/// element tree, and never touches the render tree from an observer callback — the
/// measurement happens in a post-frame callback, which is the only moment a route's
/// geometry is both committed and offstage.
#[derive(Default)]
pub(crate) struct HeroController {
    /// Flutter's `NavigatorObserver.navigator` (`navigator.dart:779`). `None` before
    /// attach and after detach, which is what makes a stale controller inert.
    navigator: Mutex<Option<NavigatorHandle>>,
    /// How many post-frame measurements have been *scheduled*. One per eligible
    /// push/pop, never one per observer callback.
    scheduled: Arc<AtomicUsize>,
    /// What those callbacks resolved, in order.
    measurements: Arc<Mutex<Vec<Measurement>>>,
}

impl HeroController {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// The navigator this controller observes, or `None` when detached.
    pub(crate) fn navigator(&self) -> Option<NavigatorHandle> {
        self.navigator.lock().clone()
    }

    /// How many post-frame measurements have been scheduled.
    pub(crate) fn scheduled_count(&self) -> usize {
        self.scheduled.load(Ordering::SeqCst)
    }

    /// Everything the post-frame callbacks resolved, in order.
    pub(crate) fn measurements(&self) -> Vec<Measurement> {
        self.measurements.lock().clone()
    }

    /// Flutter's `_maybeStartHeroTransition` (`heroes.dart:892-976`), reduced to the
    /// eligibility test and the offstage-then-schedule move.
    ///
    /// Runs **inside an observer callback**, so it does exactly two kinds of work:
    /// registry lookups behind their own mutexes, and scheduling. No element-tree
    /// read, no render-tree read, no `history` mutation. (Those would be legal since
    /// ADR-0021 §7f moved notification out from under the lock — but they would be
    /// wrong: nothing has laid out yet.)
    fn maybe_start(&self, from: Option<RouteId>, to: Option<RouteId>) {
        let Some(navigator) = self.navigator() else {
            return; // Detached. Flutter: `if (navigator == null) return;` (`:970`).
        };
        // `if (previousTopRoute == null) return;` (`:857-859`)
        let (Some(from), Some(to)) = (from, to) else {
            return;
        };
        // `if (toRoute == fromRoute || toRoute is! PageRoute || fromRoute is! PageRoute)`
        // (`:916-920`). ADR-0020 §7e already encoded `is PageRoute` as
        // `TransitionGroup::Page`, because FLUI's routes name each other by id.
        if from == to
            || !Self::is_page_route(&navigator, from)
            || !Self::is_page_route(&navigator, to)
        {
            return;
        }

        // The two routes' `offstage` controls, which also carry the animation proxies
        // Flutter reads as `route.animation` (`routes.dart:1969`). A route that is not
        // a `ModalRoute` published none, and a disposed one withdrew it.
        let (Some(source), Some(destination)) =
            (navigator.route_modal(from), navigator.route_modal(to))
        else {
            return;
        };

        let from_animation = source.primary_animation();
        let to_animation = destination.primary_animation();
        let direction = FlightDirection::classify(from_animation.status(), to_animation.status());

        // `:934-946` — a flight that has already arrived is not a flight. Note the
        // `null` arm falls through: no direction still measures.
        match direction {
            Some(FlightDirection::Pop) if from_animation.value() == 0.0 => return,
            Some(FlightDirection::Push) if to_animation.value() == 1.0 => return,
            _ => {}
        }

        // `toRoute.offstage = toRoute.animation!.value == 0.0;` (`:967`)
        //
        // Only a destination that has not begun entering is worth hiding: one already
        // part-way through its transition is on screen, and hiding it would flicker.
        destination.set_offstage(to_animation.value() == 0.0);

        // `WidgetsBinding.instance.addPostFrameCallback(…)` (`:968`). Acquired from a
        // handle the navigator captured in `init_state`, never from a frame phase.
        let Some(post_frame) = navigator.post_frame_handle() else {
            return; // Unmounted, or a binding that installs no post-frame handle.
        };

        self.scheduled.fetch_add(1, Ordering::SeqCst);
        let measurements = Arc::clone(&self.measurements);
        post_frame.schedule(move |_timing| {
            Self::measure(&navigator, &destination, from, to, direction, &measurements);
        });
    }

    /// Flutter tests `nextRoute is PageRoute` on the Dart type; FLUI's routes name
    /// each other by id, so the family travels with the published `TransitionPeer`
    /// (`binding.rs`, `TransitionGroup`). A route that is not a `TransitionRoute` at
    /// all publishes no peer, and is not a `PageRoute` either.
    fn is_page_route(navigator: &NavigatorHandle, route: RouteId) -> bool {
        navigator
            .route_peer(route)
            .is_some_and(|peer| peer.group == TransitionGroup::Page)
    }

    /// The prologue of `_startHeroTransition` (`heroes.dart:978-1030`): put the
    /// destination back onstage, then read the geometry the offstage frame committed.
    ///
    /// This runs in the **post-frame** phase of the frame the offstage flip dirtied,
    /// so `box_size` and `transform_to` answer against committed layout (ADR-0021
    /// §7c). Reading them from `did_push` instead would answer `None`, or worse,
    /// answer with the *previous* frame's geometry.
    fn measure(
        navigator: &NavigatorHandle,
        destination: &ModalHandle,
        from: RouteId,
        to: RouteId,
        direction: Option<FlightDirection>,
        measurements: &Mutex<Vec<Measurement>>,
    ) {
        // `if (fromRoute.navigator == null || toRoute.navigator == null) return;`
        // (`:969-972`) — the navigator may have left the tree while we waited.
        if !navigator.is_mounted() {
            return;
        }

        // Read before restoring: this is the value the frame under measurement
        // actually laid out with.
        let to_animation_while_offstage = destination.primary_animation().value();

        // `to.offstage = false;` (`:987`). Geometry stays committed until the next
        // layout, so measuring after this is safe — and it is what Flutter does.
        destination.set_offstage(false);

        let subtree = navigator.route_subtree(to);
        let owner = navigator.render_tree();

        let (to_size, to_transform) = match (subtree, owner) {
            (Some(subtree), Some(owner)) => {
                let owner = owner.read();
                let transform = owner
                    .root_id()
                    .and_then(|root| owner.transform_to(subtree.render_id, root));
                (owner.box_size(subtree.render_id), transform)
            }
            // An unmounted destination (`maintain_state == false` and covered) or an
            // unmounted navigator: nothing to measure, and nothing to fake.
            _ => (None, None),
        };

        measurements.lock().push(Measurement {
            direction,
            from,
            to,
            to_size,
            to_transform,
            to_animation_while_offstage,
        });
    }
}

impl NavigatorObserver for HeroController {
    /// `NavigatorObserver._navigators[this] = navigator` (`navigator.dart:3836`).
    fn did_attach(&self, navigator: NavigatorHandle) {
        *self.navigator.lock() = Some(navigator);
    }

    /// `… = null` (`:4108`). A controller that keeps observing a detached navigator
    /// would schedule against a dead binding.
    fn did_detach(&self) {
        *self.navigator.lock() = None;
    }

    /// `HeroController.didChangeTop` (`heroes.dart:853-869`) — the **only** route
    /// callback it overrides.
    ///
    /// `didPush` / `didPop` are the wrong hook: they fire for routes that never
    /// become the top one (a `pushAndRemoveUntil` beneath the current top), and they
    /// do not fire when a route becomes top by having its cover popped. Flutter's
    /// `assert(topRoute.isCurrent)` says as much.
    fn did_change_top(&self, top: RouteId, previous_top: Option<RouteId>) {
        debug_assert!(
            self.navigator()
                .is_none_or(|navigator| navigator.is_current(top)),
            "BUG: did_change_top named a route that is not the current one — \
             `assert(topRoute.isCurrent)` (heroes.dart:855)"
        );
        self.maybe_start(previous_top, Some(top));
    }
}
