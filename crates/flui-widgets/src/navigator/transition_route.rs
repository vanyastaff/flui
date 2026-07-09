//! [`TransitionRoute`] — a route with an entrance and exit animation.
//!
//! ADR-0020 U5.2. **Private.** No `ModalRoute`, no barrier, no `PageRoute`, no
//! public API. The first consumer of U5.1's `RouteBinding` seam.
//!
//! # Flutter parity
//!
//! `.flutter/packages/flutter/lib/src/widgets/routes.dart:111-639`
//! (`TransitionRoute`), master `3.33.0-0.0.pre-6280-g88e87cd963f`.
//!
//! The whole class turns on one observation: **the route drives its own lifecycle
//! from an animation status listener**, calling back into the navigator. It is not
//! the navigator that waits on the animation.
//!
//! ```dart
//! void _handleStatusChanged(AnimationStatus status) {              // :293-321
//!   switch (status) {
//!     case AnimationStatus.completed: …overlayEntries.first.opaque = opaque;
//!     case AnimationStatus.forward:
//!     case AnimationStatus.reverse:  …overlayEntries.first.opaque = false;
//!     case AnimationStatus.dismissed:
//!       if (!isActive) { navigator!.finalizeRoute(this); _popFinalized = true; }
//!   }
//! }
//! bool get finishedWhenPopped => _controller!.isDismissed && !_popFinalized;  // :177
//! ```
//!
//! Both callbacks reach the navigator through a [`RouteBinding`], which enqueues a
//! `RouteCommand` rather than re-entering the flush (U5.1, `binding.rs`
//! *Correction 1*). A zero-duration transition therefore completes *inside* the
//! flush that started it, and settles on that flush's second pass.
//!
//! # Deliberately not implemented here
//!
//! - **`opaque`.** `_handleStatusChanged` writes `overlayEntries.first.opaque`
//!   (`:297`, `:304`). FLUI's `Overlay` has no `opaque` (ADR-0019 U1 deferred it),
//!   so there is nothing to write to and **nothing is claimed**. U5.3 adds it.
//! - **`didReplace`'s controller-value inheritance** (`:363-374`). It needs the
//!   *replaced* route's controller, and FLUI's routes are named by `RouteId`; the
//!   `TransitionPeer` registry publishes the primary `Animation`, not the
//!   `AnimationController`. `pushReplacement` is also not exported (ADR-0019 U4),
//!   so this has no reachable caller. Recorded, not faked.
//! # `didPopNext` and `completed` — two claims this file first got wrong
//!
//! An early draft made `did_pop_next` a no-op and skipped the `completed` signal,
//! reasoning that the flush's `did_change_next(None)` would reset the proxy. Both
//! were wrong, and two tests caught it.
//!
//! `_RouteEntry.handleDidPopNext(poppedRoute)` hands `didPopNext` the **popped**
//! route, and `TransitionRoute.didPopNext` wires the secondary to *its* animation
//! (`routes.dart:393-402`). That is the point: the lower route animates back out
//! as the upper one reverses away. And `did_change_next(None)` never arrives —
//! `shouldAnnounceChangeToNext` suppresses it precisely because `didPopNext`
//! already spoke (`navigator.dart:3541-3546`).
//!
//! So the proxy must be released some other way, which is exactly what
//! `nextRoute.completed` does (`routes.dart:503-509`), guarded by
//! `if (_secondaryAnimation.parent == animation)` so a stale disposal cannot
//! clobber a newer parent. FLUI's [`CompletedSignal`] is that channel — private,
//! synchronous, and added only because the contract demanded it.
//! - **Predictive back / `_simulation` / `DartPerformanceMode`.** Platform work.

// `TransitionRoute` is reached only through `NavigatorHandle::push_bound`, whose
// production caller is `ModalRoute` (U5.3) / `PageRoute` (U5.4). Until then only
// the tests push one, so rustc sees the whole file as dead. Same posture, and the
// same promise, as `binding.rs`: this attribute goes when the consumer arrives.
#![allow(dead_code)]

use std::sync::Arc;
#[cfg(test)]
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use std::{fmt, marker::PhantomData};

use flui_animation::{
    ALWAYS_DISMISSED, Animation, AnimationController, AnimationStatus, AnimationSwitch,
    ConstantAnimation, ProxyAnimation, Scheduler, VsyncRegistration,
};
use parking_lot::Mutex;

use super::binding::{BoundRoute, CompletedSignal, RouteBinding, TransitionPeer};
use super::overlay_route::{NavigatorRoute, RouteContentBuilder};
use super::route::{PushCompletion, Route, RouteId, RouteSettings};

/// The always-dismissed animation a `secondary_animation` rests at.
///
/// Flutter's `kAlwaysDismissedAnimation` (`routes.dart:198`, `:491`).
fn always_dismissed() -> Arc<dyn Animation<f32>> {
    Arc::new(ConstantAnimation::dismissed(ALWAYS_DISMISSED.value()))
}

/// What the `secondary_animation` proxy currently points at.
enum SecondaryParent {
    /// `kAlwaysDismissedAnimation`: no route above, or it cannot be coordinated.
    Dismissed,
    /// Pointed straight at the next route's primary animation.
    Direct(RouteId),
    /// Mid-hop: an [`AnimationSwitch`] is proxying from the old train to `target`.
    Hopping {
        target: RouteId,
        switch: AnimationSwitch,
    },
}

impl SecondaryParent {
    /// The animation currently *driving* the proxy — Flutter's `currentTrain`
    /// (`routes.dart:434-436`), which unwraps a hopper.
    fn current_train(&self, proxy: &ProxyAnimation<f32>) -> Option<Arc<dyn Animation<f32>>> {
        match self {
            Self::Dismissed => None,
            Self::Direct(_) => Some(proxy.parent()),
            Self::Hopping { switch, .. } => Some(switch.current()),
        }
    }
}

/// State shared between the route and its animation status listener.
///
/// The listener is `Arc<dyn Fn(AnimationStatus)> + 'static` and cannot borrow the
/// route, so everything it touches lives here behind an `Arc`.
struct TransitionInner {
    controller: Mutex<Option<AnimationController>>,
    binding: Mutex<Option<RouteBinding>>,

    /// The proxy handed to the route *below* this one is **this** route's
    /// secondary; the primary is the controller, unproxied. Flutter is the same:
    /// "only `secondaryAnimation` is a `ProxyAnimation`" (`routes.dart:197-198`).
    secondary: Arc<ProxyAnimation<f32>>,
    secondary_parent: Mutex<SecondaryParent>,

    /// Flutter's `isActive` is `navigator.contains(this) && entry.isPresent`
    /// (`navigator.dart:584-643`). A popped route is not present, so this is the
    /// half that matters to `_handleStatusChanged`'s `dismissed` guard.
    popped: AtomicBool,
    /// Flutter's `_popFinalized` (`routes.dart:180`).
    pop_finalized: AtomicBool,

    vsync_registration: Mutex<Option<(flui_animation::Vsync, VsyncRegistration)>>,
    will_dispose_controller: bool,

    /// Fired in `dispose`. The route **below** listens on it to release its
    /// secondary proxy — Flutter's `nextRoute.completed` (`routes.dart:503-509`).
    completed: Arc<CompletedSignal>,

    /// How many times the status listener raised `finalize()`. Test-facing: the
    /// `_popFinalized` guard is what keeps this at one, and nothing else observes
    /// it — FLUI's `finalize` command is idempotent, where Flutter's
    /// `finalizeRoute` asserts.
    #[cfg(test)]
    finalize_calls: AtomicUsize,
}

impl TransitionInner {
    /// Flutter's `isActive` (`routes.dart:314` reads it).
    fn is_active(&self) -> bool {
        !self.popped.load(Ordering::Acquire)
    }

    /// `_handleStatusChanged` (`routes.dart:293-321`).
    ///
    /// `completed` and `dismissed` are the two arms with behavior here. The
    /// `forward`/`reverse` arm only writes `overlayEntries.first.opaque = false`,
    /// which FLUI has nowhere to write (see the module docs), so it is a no-op —
    /// and *not* claimed.
    fn handle_status_changed(&self, status: AnimationStatus) {
        let binding = self.binding.lock().clone();
        let Some(binding) = binding else { return };

        match status {
            AnimationStatus::Completed => {
                // The entrance transition finished: `pushing` → `idle`.
                // Flutter gets this from `didPush`'s `TickerFuture`; FLUI's
                // controller returns no future, so the status listener is the
                // seam (`PushCompletion::Animating` + the command queue).
                binding.notify_push_completed();
            }
            // "We might still be an active route if a subclass is controlling the
            // transition and hits the dismissed status." (`routes.dart:310-313`)
            AnimationStatus::Dismissed
                if !self.is_active() && !self.pop_finalized.swap(true, Ordering::AcqRel) =>
            {
                #[cfg(test)]
                self.finalize_calls.fetch_add(1, Ordering::Relaxed);
                binding.finalize();
            }
            // `forward`/`reverse` only write `overlayEntries.first.opaque = false`
            // (`routes.dart:303-305`), which FLUI has nowhere to write. Not claimed.
            // A `dismissed` that fails the guard above lands here too.
            _ => {}
        }
    }
}

/// A route whose entrance and exit are animated.
///
/// Private: `TransitionRoute` is not exported, and `transition_route_is_not_exported`
/// keeps it that way until U5.4's parity + sign-off gate.
pub(crate) struct TransitionRoute<T> {
    settings: RouteSettings,
    builder: RouteContentBuilder,
    duration: Duration,
    reverse_duration: Option<Duration>,
    current_result: Option<T>,

    /// `canTransitionTo(nextRoute)` (`routes.dart:536`), default `true`.
    can_transition_to: bool,
    /// `canTransitionFrom(previousRoute)` (`:561`), default `true`. Published to
    /// the registry so the route *below* can ask it.
    can_transition_from: bool,

    inner: Arc<TransitionInner>,
    _output: PhantomData<fn() -> T>,
}

impl<T> TransitionRoute<T> {
    /// A route showing `builder`, entering and leaving over `duration`.
    pub(crate) fn new(
        duration: Duration,
        builder: impl Fn(&dyn flui_view::BuildContext) -> flui_view::BoxedView + Send + Sync + 'static,
    ) -> Self {
        Self {
            settings: RouteSettings::default(),
            builder: Arc::new(builder),
            duration,
            reverse_duration: None,
            current_result: None,
            can_transition_to: true,
            can_transition_from: true,
            inner: Arc::new(TransitionInner {
                controller: Mutex::new(None),
                binding: Mutex::new(None),
                secondary: Arc::new(ProxyAnimation::new(always_dismissed())),
                secondary_parent: Mutex::new(SecondaryParent::Dismissed),
                popped: AtomicBool::new(false),
                pop_finalized: AtomicBool::new(false),
                vsync_registration: Mutex::new(None),
                will_dispose_controller: true,
                completed: Arc::new(CompletedSignal::default()),
                #[cfg(test)]
                finalize_calls: AtomicUsize::new(0),
            }),
            _output: PhantomData,
        }
    }

    pub(crate) fn named(mut self, name: impl Into<String>) -> Self {
        self.settings = RouteSettings::named(name);
        self
    }

    /// Flutter's `reverseTransitionDuration`, which defaults to
    /// `transitionDuration` (`routes.dart:148`).
    pub(crate) fn reverse_duration(mut self, duration: Duration) -> Self {
        self.reverse_duration = Some(duration);
        self
    }

    pub(crate) fn with_current_result(mut self, result: T) -> Self {
        self.current_result = Some(result);
        self
    }

    pub(crate) fn can_transition_to(mut self, allow: bool) -> Self {
        self.can_transition_to = allow;
        self
    }

    pub(crate) fn can_transition_from(mut self, allow: bool) -> Self {
        self.can_transition_from = allow;
        self
    }

    /// A cloneable view of the route's animation state, obtainable **before** the
    /// route is moved into `push_bound`.
    ///
    /// The controller is created in `install()`, so a caller cannot hold it up
    /// front; the handle resolves it lazily. Test-facing: FLUI's controller
    /// returns no `TickerFuture`, so a test drives the transition by hand.
    pub(crate) fn handle(&self) -> TransitionHandle {
        TransitionHandle {
            inner: Arc::clone(&self.inner),
        }
    }

    /// Flutter's `_updateSecondaryAnimation(nextRoute)` (`routes.dart:422-496`).
    ///
    /// Reads the next route's `TransitionPeer` (its primary animation and its
    /// `canTransitionFrom`), gates on both predicates, and either points the proxy
    /// straight at it or — when the two animations are at different values and the
    /// target is moving — installs an [`AnimationSwitch`] that hops when they
    /// cross.
    fn update_secondary_animation(&self, next: Option<RouteId>) {
        let binding = self.inner.binding.lock().clone();
        let Some(binding) = binding else { return };

        // `nextRoute is TransitionRoute && canTransitionTo(next) && next.canTransitionFrom(this)`
        // (`routes.dart:429-431`). A non-transition route has no peer.
        let target = next.and_then(|id| binding.peer(id).map(|peer| (id, peer)));
        let Some((next_id, peer)) =
            target.filter(|(_, peer)| self.can_transition_to && peer.can_transition_from)
        else {
            self.set_secondary(SecondaryParent::Dismissed, always_dismissed());
            return;
        };

        let mut parent = self.inner.secondary_parent.lock();

        // Already pointed at this route (directly, or as a hop target): nothing to do.
        match &*parent {
            SecondaryParent::Direct(id) | SecondaryParent::Hopping { target: id, .. }
                if *id == next_id =>
            {
                return;
            }
            _ => {}
        }

        let current_train = parent.current_train(&self.inner.secondary);
        let next_animation = Arc::clone(&peer.animation);

        // `currentTrain.value == nextTrain.value || !nextTrain.isAnimating`
        // (`routes.dart:438-439`).
        //
        // **Not** `Animation::is_animating`. Flutter's `isAnimating` is
        // status-based (`forward || reverse`), but FLUI's `AnimationController`
        // *overrides* it to mean "the ticker is running", which stays true after a
        // controller has settled at `Completed`. Using the override here makes a
        // settled route look like a moving train and forces a spurious hop —
        // caught by `a_stale_train_does_not_clobber_a_newer_parent`. The
        // controller's override is a separate divergence, recorded in ADR-0020 §7c.
        let is_moving = matches!(
            next_animation.status(),
            AnimationStatus::Forward | AnimationStatus::Reverse
        );
        let jump = match &current_train {
            None => true,
            Some(train) => {
                (train.value() - next_animation.value()).abs() < f32::EPSILON || !is_moving
            }
        };

        let previous = std::mem::replace(&mut *parent, SecondaryParent::Dismissed);

        if jump {
            self.inner.secondary.set_parent(Arc::clone(&next_animation));
            *parent = SecondaryParent::Direct(next_id);
        } else {
            let train = current_train.expect("jump == false implies a current train");
            let proxy = Arc::clone(&self.inner.secondary);
            let target_for_hop = Arc::clone(&next_animation);
            let switch = AnimationSwitch::new(train, Some(Arc::clone(&next_animation)))
                // `onSwitchedTrain`: point the proxy **directly** at the target and
                // drop the hopper (`routes.dart:473-483`).
                .on_switched(move || proxy.set_parent(Arc::clone(&target_for_hop)));
            self.inner
                .secondary
                .set_parent(Arc::new(switch.clone()) as Arc<dyn Animation<f32>>);
            *parent = SecondaryParent::Hopping {
                target: next_id,
                switch,
            };
        }

        // "You cannot dispose the old hopper until its replacement exists."
        // (`routes.dart:495` — the previous remover runs last.)
        //
        // No test reaches this ordering: `ProxyAnimation::set_parent` re-subscribes
        // eagerly, so once the new parent is installed the proxy holds no reference
        // to the old hopper and disposing it early is invisible. Kept because it is
        // faithful and free; stated rather than claimed.
        drop(parent);
        if let SecondaryParent::Hopping { switch, .. } = previous {
            switch.dispose();
        }

        // `_setSecondaryAnimation(animation, nextRoute.completed)` (`routes.dart:498-509`):
        // release the reference when the route above is disposed, but only if we
        // are still pointing at it — a stale disposal must not clobber a newer parent.
        let inner = Arc::downgrade(&self.inner);
        peer.completed.on_completed(Arc::new(move || {
            let Some(inner) = inner.upgrade() else { return };
            let mut parent = inner.secondary_parent.lock();
            let still_ours = matches!(
                &*parent,
                SecondaryParent::Direct(id) | SecondaryParent::Hopping { target: id, .. }
                    if *id == next_id
            );
            if !still_ours {
                return;
            }
            let previous = std::mem::replace(&mut *parent, SecondaryParent::Dismissed);
            inner.secondary.set_parent(always_dismissed());
            drop(parent);
            if let SecondaryParent::Hopping { switch, .. } = previous {
                switch.dispose();
            }
        }));
    }

    fn set_secondary(&self, kind: SecondaryParent, animation: Arc<dyn Animation<f32>>) {
        let previous = std::mem::replace(&mut *self.inner.secondary_parent.lock(), kind);
        self.inner.secondary.set_parent(animation);
        if let SecondaryParent::Hopping { switch, .. } = previous {
            switch.dispose();
        }
    }
}

/// A cloneable view of a [`TransitionRoute`]'s animation state.
#[derive(Clone)]
pub(crate) struct TransitionHandle {
    inner: Arc<TransitionInner>,
}

impl TransitionHandle {
    /// The controller driving the route's **primary** animation, once `install`
    /// has created it.
    pub(crate) fn controller(&self) -> Option<AnimationController> {
        self.inner.controller.lock().clone()
    }

    /// Flutter's `secondaryAnimation` (`routes.dart:197`). A `ProxyAnimation`
    /// resting at `kAlwaysDismissedAnimation`.
    pub(crate) fn secondary_animation(&self) -> Arc<ProxyAnimation<f32>> {
        Arc::clone(&self.inner.secondary)
    }

    /// Flutter's `_popFinalized` (`routes.dart:180`).
    pub(crate) fn is_pop_finalized(&self) -> bool {
        self.inner.pop_finalized.load(Ordering::Acquire)
    }

    /// How many times the status listener raised `finalize()`.
    #[cfg(test)]
    pub(crate) fn finalize_calls(&self) -> usize {
        self.inner.finalize_calls.load(Ordering::Relaxed)
    }

    /// Whether the secondary proxy currently rests at always-dismissed.
    pub(crate) fn secondary_is_dismissed(&self) -> bool {
        matches!(
            &*self.inner.secondary_parent.lock(),
            SecondaryParent::Dismissed
        )
    }

    /// Whether the secondary proxy is mid-hop (an `AnimationSwitch` is installed).
    pub(crate) fn secondary_is_hopping(&self) -> bool {
        matches!(
            &*self.inner.secondary_parent.lock(),
            SecondaryParent::Hopping { .. }
        )
    }
}

impl<T> fmt::Debug for TransitionRoute<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TransitionRoute")
            .field("name", &self.settings.name())
            .field("duration", &self.duration)
            .finish_non_exhaustive()
    }
}

impl<T: Send + Sync + Clone + 'static> Route for TransitionRoute<T> {
    type Output = T;

    fn settings(&self) -> &RouteSettings {
        &self.settings
    }

    fn current_result(&mut self) -> Option<T> {
        self.current_result.clone()
    }

    /// `finishedWhenPopped => _controller!.isDismissed && !_popFinalized`
    /// (`routes.dart:177-178`).
    ///
    /// False while the exit transition runs, so `handle_pop` leaves the entry in
    /// `Popping` and the overlay entry alive. True when the controller was
    /// **already** dismissed at pop time (the Cupertino dismiss gesture,
    /// `routes.dart:173-176`), which finalizes synchronously; `_popFinalized` then
    /// stops the status listener finalizing a second time.
    fn finished_when_popped(&self) -> bool {
        let dismissed = self
            .inner
            .controller
            .lock()
            .as_ref()
            .is_some_and(AnimationController::is_dismissed);
        dismissed && !self.inner.pop_finalized.load(Ordering::Acquire)
    }

    /// `install()` (`routes.dart:323-334`): create the controller, attach the
    /// status listener, then let the overlay entry be created.
    ///
    /// The controller is created **here**, not in the constructor — Flutter is the
    /// same, and it is what lets a route be constructed before it has a navigator.
    fn install(&mut self) {
        debug_assert!(
            self.inner.binding.lock().is_some(),
            "BUG: a TransitionRoute must be pushed with `push_bound` so it is bound before install"
        );

        let controller = AnimationController::new(self.duration, Arc::new(Scheduler::new()));
        if let Some(reverse) = self.reverse_duration {
            controller.set_reverse_duration(reverse);
        }

        let weak = Arc::downgrade(&self.inner);
        controller.add_status_listener(Arc::new(move |status| {
            if let Some(inner) = weak.upgrade() {
                inner.handle_status_changed(status);
            }
        }));

        // The navigator's clock — the FLUI shape of `vsync: navigator!`. Absent a
        // `VsyncScope`, the controller keeps its own wall-clock ticker.
        if let Some(binding) = self.inner.binding.lock().as_ref()
            && let Some(vsync) = binding.vsync()
        {
            let registration = vsync.register(controller.clone());
            *self.inner.vsync_registration.lock() = Some((vsync, registration));
        }

        // Publish the primary animation so the route below can coordinate.
        if let Some(binding) = self.inner.binding.lock().as_ref() {
            binding.publish_peer(TransitionPeer {
                animation: Arc::new(controller.clone()) as Arc<dyn Animation<f32>>,
                can_transition_from: self.can_transition_from,
                completed: Arc::clone(&self.inner.completed),
            });
        }

        *self.inner.controller.lock() = Some(controller);
    }

    /// `didPush()` (`routes.dart:336-350`): drive the controller forward.
    ///
    /// Flutter returns the `TickerFuture` and `handlePush` awaits it. FLUI's
    /// controller returns no future, so the entry parks in `Pushing` and the
    /// status listener raises `notify_push_completed` when the controller reaches
    /// `Completed` — through the U5.1 command queue, never a direct call.
    fn did_push(&mut self) -> PushCompletion {
        if let Some(controller) = self.inner.controller.lock().as_ref() {
            let _ = controller.forward();
        }
        PushCompletion::Animating
    }

    /// `didAdd()` (`routes.dart:352-361`): jump to the end, no animation.
    fn did_add(&mut self) {
        if let Some(controller) = self.inner.controller.lock().as_ref() {
            controller.set_value(1.0);
        }
    }

    /// `didPop(result)` (`routes.dart:376-391`): drive the controller in reverse
    /// and consent. The route's `RouteResult` completes **now**, via
    /// `RouteRecord::did_pop`; only its disposal waits for `dismissed`.
    fn did_pop(&mut self) -> bool {
        self.inner.popped.store(true, Ordering::Release);
        if let Some(controller) = self.inner.controller.lock().as_ref() {
            let _ = controller.reverse();
        }
        true
    }

    /// `didChangeNext(nextRoute)` (`routes.dart:404-413`).
    fn did_change_next(&mut self, next: Option<RouteId>) {
        self.update_secondary_animation(next);
    }

    /// `didPopNext(nextRoute)` (`routes.dart:393-402`).
    ///
    /// The argument is the **popped** route (`navigator.dart:3312`), and the
    /// secondary is wired to *its* animation on purpose: this route animates back
    /// out as the one above reverses away. It is released when that route
    /// completes — see the module docs.
    fn did_pop_next(&mut self, popped: RouteId) {
        self.update_secondary_animation(Some(popped));
    }

    /// `dispose()` (`routes.dart:627-638`): detach the listener, unregister the
    /// clock, drop the peer, and dispose the controller **only if we own it**.
    fn dispose(&mut self) {
        if let Some(binding) = self.inner.binding.lock().as_ref() {
            binding.withdraw_peer();
        }
        // Release every route below that is still proxying our animation, before
        // the controller is disposed under them.
        self.inner.completed.complete();
        self.set_secondary(SecondaryParent::Dismissed, always_dismissed());

        if let Some((vsync, registration)) = self.inner.vsync_registration.lock().take() {
            // `VsyncRegistration` has no `Drop`; a missed unregister keeps a
            // disposed route's controller ticking forever.
            vsync.unregister(registration);
        }

        if let Some(controller) = self.inner.controller.lock().take()
            && self.inner.will_dispose_controller
        {
            controller.dispose();
        }
    }
}

impl<T: Send + Sync + Clone + 'static> NavigatorRoute for TransitionRoute<T> {
    fn content_builder(&self) -> RouteContentBuilder {
        Arc::clone(&self.builder)
    }
}

impl<T: Send + Sync + Clone + 'static> BoundRoute for TransitionRoute<T> {
    fn bind(&mut self, binding: RouteBinding) {
        *self.inner.binding.lock() = Some(binding);
    }
}
