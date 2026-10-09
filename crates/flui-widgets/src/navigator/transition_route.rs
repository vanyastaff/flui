//! [`TransitionRoute`] — a route with an entrance and exit animation.
//!
//! **Private.** No `ModalRoute`, no barrier, no `PageRoute`, no
//! public API. The first consumer of the `RouteBinding` seam.
//!
//! # The route drives its own lifecycle
//!
//! The whole type turns on one observation: **the route drives its own lifecycle
//! from an animation status listener**, calling back into the navigator. It is not
//! the navigator that waits on the animation. On `Completed` the route writes its
//! `opaque` flag to the overlay entry; on `Forward`/`Reverse` it clears it; on
//! `Dismissed`, if the route is no longer active, it finalizes itself once.
//!
//! Both callbacks reach the navigator through a `RouteBinding`, which enqueues a
//! `RouteCommand` rather than re-entering the flush (see `binding.rs`
//! *Correction 1*). A zero-duration transition therefore completes *inside* the
//! flush that started it, and settles on that flush's second pass.
//!
//! # Deliberately not implemented here
//!
//! - **`did_replace`'s controller-value inheritance.** It needs the
//!   *replaced* route's controller, and routes are named by `RouteId`; the
//!   `TransitionPeer` registry publishes the primary `Animation`, not the
//!   `AnimationController`. Its only producer would be the `Replace` lifecycle
//!   (a route replacing the one below it), which is not exported.
//!   (`push_replacement` **is** exported, but its `PushReplace` lifecycle runs
//!   `did_push`, a full entrance animation, and never reaches `did_replace`.)
//!   Recorded, not faked.
//! - **Predictive back and platform performance modes.** Platform work.
//!
//! # `did_pop_next` and `completed`
//!
//! An early draft made `did_pop_next` a no-op and skipped the `completed` signal,
//! reasoning that the flush's `did_change_next(None)` would reset the proxy. Both
//! were wrong, and two tests caught it.
//!
//! `did_pop_next` receives the **popped** route, and the secondary is wired to
//! *its* animation. That is the point: the lower route animates back out
//! as the upper one reverses away. And `did_change_next(None)` never arrives —
//! the flush suppresses it precisely because `did_pop_next` already spoke.
//!
//! So the proxy must be released some other way: when the route above completes,
//! guarded by a check that the proxy still points at it so a stale disposal cannot
//! clobber a newer parent. [`CompletedSignal`] is that channel — private,
//! synchronous, and added only because the contract demanded it.

// `TransitionRoute` is private and reached only through `ModalRoute` and,
// above it, the public `PageRoute` / `PopupRoute`. Exporting those removed
// this file's `#![allow(dead_code)]`: everything left is either reachable from a
// public route or re-exported to the integration tests by `crate::__test_access`
// (ADR-0083 §4).

use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;
use std::{fmt, marker::PhantomData};

use flui_animation::{
    ALWAYS_COMPLETE, ALWAYS_DISMISSED, Animation, AnimationController, AnimationStatus,
    AnimationSwitch, ConstantAnimation, DrivenController, ProxyAnimation,
};
use flui_foundation::ChangeNotifier;
use std::cell::{Cell, RefCell};

use super::binding::{CompletedSignal, RouteBindingSlot, TransitionGroup, TransitionPeer};
use super::overlay_route::{NavigatorRoute, RouteContentBuilder};
use super::route::{PushCompletion, Route, RouteId, RouteSettings};

/// The always-dismissed animation a `secondary_animation` rests at
/// (`value == 0.0`, `status == dismissed`).
pub(crate) fn always_dismissed() -> std::rc::Rc<dyn Animation<f64>> {
    Rc::new(ConstantAnimation::dismissed(ALWAYS_DISMISSED.value()))
}

/// The always-complete animation (`value == 1.0`, `status == completed`). What an
/// **offstage** `ModalRoute`'s primary animation points at.
pub(crate) fn always_complete() -> std::rc::Rc<dyn Animation<f64>> {
    Rc::new(ConstantAnimation::completed(ALWAYS_COMPLETE.value()))
}

/// What the `secondary_animation` proxy currently points at.
enum SecondaryParent {
    /// The always-dismissed animation: no route above, or it cannot be coordinated.
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
    /// The animation currently *driving* the proxy, unwrapping a hopper.
    fn current_train(
        &self,
        proxy: &ProxyAnimation<f64>,
    ) -> Option<std::rc::Rc<dyn Animation<f64>>> {
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
    controller: RefCell<Option<DrivenController>>,
    disposed: Cell<bool>,
    binding: super::lifecycle::Terminal<RouteBindingSlot>,
    /// Statuses reported by the `Send + Sync` animation listener, awaiting
    /// owner-local application.
    pending_statuses: Rc<RefCell<Vec<AnimationStatus>>>,
    /// Data-only owner wake for status changes that do not produce a value tick,
    /// e.g. `reverse()` from 1.0. `ModalRoute` points this at the same relay that
    /// its `ModalScope` listens to.
    status_wake: RefCell<Option<Rc<ChangeNotifier>>>,

    /// The proxy handed to the route *below* this one is **this** route's
    /// secondary; the primary is the controller, unproxied. Only the secondary
    /// animation is a `ProxyAnimation`.
    secondary: super::lifecycle::Terminal<Rc<ProxyAnimation<f64>>>,
    secondary_parent: RefCell<SecondaryParent>,

    /// Set once the route is popped: a popped route is no longer active, which is
    /// the half that matters to the status handler's `Dismissed` guard.
    popped: AtomicBool,
    /// Whether the pop has already been finalized.
    pop_finalized: AtomicBool,

    /// Whether the route obscures the ones below **once its entrance transition
    /// completes**. `PageRoute` sets `true`, `PopupRoute` `false`. Defaults to
    /// `false`, the conservative value: nothing is skipped unless a route asks.
    opaque: AtomicBool,

    /// Fired in `dispose`. The route **below** listens on it to release its
    /// secondary proxy.
    completed: super::lifecycle::Terminal<Rc<CompletedSignal>>,

    /// How many times the status listener raised `finalize()`. Test-facing: the
    /// `pop_finalized` guard is what keeps this at one, and nothing else observes
    /// it, since the `finalize` command is idempotent. Compiled into every build so the route has one
    /// layout whether or not the integration tests link it (ADR-0083 §4).
    finalize_calls: AtomicUsize,
}

impl Drop for TransitionInner {
    fn drop(&mut self) {
        let controller = self.controller.get_mut().take();
        let binding = self.binding.withdraw();
        let wake = super::lifecycle::Terminal::new(self.status_wake.get_mut().take());
        let secondary = self.secondary.withdraw();
        let parent = super::lifecycle::Terminal::new(std::mem::replace(
            self.secondary_parent.get_mut(),
            SecondaryParent::Dismissed,
        ));
        let completed = self.completed.withdraw();
        let mut recovery = flui_foundation::panic::PanicRecovery::new();
        recovery.run(|| drop(controller));
        recovery.retire((binding, wake, secondary, parent, completed));
        recovery.finish();
    }
}

impl TransitionInner {
    fn controller(&self) -> Option<AnimationController> {
        self.controller
            .borrow()
            .as_ref()
            .map(|owner| owner.controller().clone())
    }

    fn rebind_clock(&self, vsync: Option<&flui_animation::Vsync>) {
        if self.disposed.get() {
            return;
        }
        let outgoing = self.controller.borrow_mut().take();
        let Some(mut owner) = outgoing else {
            return;
        };
        let mut recovery = flui_foundation::panic::PanicRecovery::new();
        recovery.run(|| {
            if let Err(error) = owner.rebind(vsync) {
                tracing::error!(%error, "route animation has no frame clock");
            }
        });
        if !self.disposed.get() && self.controller.borrow().is_none() {
            *self.controller.borrow_mut() = Some(owner);
        } else {
            recovery.run(|| drop(owner));
        }
        recovery.finish();
    }

    /// Whether the route is still active (not yet popped).
    fn is_active(&self) -> bool {
        !self.popped.load(Ordering::Acquire)
    }

    /// React to an animation status change.
    fn handle_status_changed(&self, status: AnimationStatus) {
        let Some(binding) = self.binding.get() else {
            return;
        };

        match status {
            AnimationStatus::Completed => {
                // Publish `opaque` to the route's overlay entry.
                // The entrance settling `pushing` → `idle` is driven by
                // `NavigatorShared::apply` awaiting the `AnimationRunFuture`
                // `did_push` already handed the navigator (ADR-0064), not by
                // this listener — which owns only the opaque flag.
                binding.set_entry_opaque(self.opaque.load(Ordering::Relaxed));
            }
            // A route in motion never occludes, because the routes beneath it show
            // through the transition.
            AnimationStatus::Forward | AnimationStatus::Reverse => {
                binding.set_entry_opaque(false);
            }
            // A route may still be active if something else is controlling the
            // transition and hits the dismissed status.
            AnimationStatus::Dismissed
                if !self.is_active() && !self.pop_finalized.swap(true, Ordering::AcqRel) =>
            {
                self.finalize_calls.fetch_add(1, Ordering::Relaxed);
                binding.finalize();
            }
            // A `dismissed` that fails the guard above: still an active route.
            // `AnimationStatus` is `#[non_exhaustive]`.
            _ => {}
        }
    }

    fn drain_pending_statuses(&self) {
        let statuses = std::mem::take(&mut *self.pending_statuses.borrow_mut());
        for status in statuses {
            self.handle_status_changed(status);
        }
    }
}

/// A route whose entrance and exit are animated.
///
/// Private: `TransitionRoute` is not exported until its sign-off gate.
pub struct TransitionRoute<T> {
    settings: RouteSettings,
    builder: super::lifecycle::Terminal<RouteContentBuilder>,
    duration: Duration,
    reverse_duration: Option<Duration>,
    current_result: Option<T>,

    /// Whether this route may coordinate with the route above it; default `true`.
    can_transition_to: bool,
    /// Whether the route below may coordinate with this one; default `true`. Published to
    /// the registry so the route *below* can ask it.
    can_transition_from: bool,
    /// The family this route coordinates transitions with. `PageRoute` sets
    /// [`TransitionGroup::Page`]; everything else stays at the default.
    group: TransitionGroup,

    inner: super::lifecycle::Terminal<Rc<TransitionInner>>,
    _output: PhantomData<fn() -> T>,
}

impl<T> Drop for TransitionRoute<T> {
    fn drop(&mut self) {
        let settings = super::lifecycle::Terminal::new(std::mem::take(&mut self.settings));
        let builder = self.builder.withdraw();
        let result = super::lifecycle::Terminal::new(self.current_result.take());
        let inner = self.inner.withdraw();
        drop((settings, builder, result, inner));
    }
}

impl<T> TransitionRoute<T> {
    /// A route showing `builder`, entering and leaving over `duration`.
    pub fn new(
        duration: Duration,
        builder: impl Fn(&dyn flui_view::BuildContext) -> flui_view::BoxedView + 'static,
    ) -> Self {
        let binding = RouteBindingSlot::new();
        binding.set_group(TransitionGroup::Default);
        Self {
            settings: RouteSettings::default(),
            builder: super::lifecycle::Terminal::new(Rc::new(builder)),
            duration,
            reverse_duration: None,
            current_result: None,
            can_transition_to: true,
            can_transition_from: true,
            group: TransitionGroup::Default,
            inner: super::lifecycle::Terminal::new(Rc::new(TransitionInner {
                controller: RefCell::new(None),
                disposed: Cell::new(false),
                binding: super::lifecycle::Terminal::new(binding),
                pending_statuses: Rc::new(RefCell::new(Vec::new())),
                status_wake: RefCell::new(None),
                secondary: super::lifecycle::Terminal::new(Rc::new(ProxyAnimation::new(
                    always_dismissed(),
                ))),
                secondary_parent: RefCell::new(SecondaryParent::Dismissed),
                popped: AtomicBool::new(false),
                pop_finalized: AtomicBool::new(false),
                opaque: AtomicBool::new(false),
                completed: super::lifecycle::Terminal::new(Rc::new(CompletedSignal::default())),
                finalize_calls: AtomicUsize::new(0),
            })),
            _output: PhantomData,
        }
    }

    /// Name the route (its `RouteSettings::name`).
    #[must_use]
    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.settings = RouteSettings::named(name);
        self
    }

    /// The entrance duration. Read once, in `install()`, so a builder may change
    /// it any time before the push.
    pub(crate) fn duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    /// The exit duration; defaults to the entrance duration.
    pub(crate) fn reverse_duration(mut self, duration: Duration) -> Self {
        self.reverse_duration = Some(duration);
        self
    }

    pub(crate) fn with_current_result(mut self, result: T) -> Self {
        self.current_result = Some(result);
        self
    }

    /// Whether this route may coordinate with the route above; default `true`. No
    /// public route sets it: `PageRoute`'s family restriction is a [`TransitionGroup`], not a bool.
    #[must_use]
    pub fn can_transition_to(mut self, allow: bool) -> Self {
        self.can_transition_to = allow;
        self
    }

    /// Whether the route below may coordinate with this one; default `true`. See
    /// [`can_transition_to`](Self::can_transition_to).
    #[must_use]
    pub fn can_transition_from(mut self, allow: bool) -> Self {
        self.can_transition_from = allow;
        self
    }

    /// The transition family — `PageRoute` coordinates only with other
    /// `PageRoute`s.
    pub(crate) fn group(mut self, group: TransitionGroup) -> Self {
        self.group = group;
        self.inner.binding.set_group(group);
        self
    }

    /// Written to the route's overlay entry when the entrance transition
    /// completes, and cleared while it moves.
    pub(crate) fn opaque(self, opaque: bool) -> Self {
        self.inner.opaque.store(opaque, Ordering::Relaxed);
        self
    }

    /// A cloneable view of the route's animation state, obtainable **before** the
    /// route is moved into `NavigatorHandle::push`.
    ///
    /// The controller is created in `install()`, so a caller cannot hold it up
    /// front; the handle resolves it lazily. Test-facing: a unit test drives
    /// the transition by hand through this handle (`set_value`) rather than
    /// awaiting the `AnimationRunFuture` `did_push` returns; awaiting it needs real
    /// elapsed time driven through a `Vsync`.
    #[must_use]
    pub fn handle(&self) -> TransitionHandle {
        TransitionHandle {
            inner: Rc::clone(&self.inner),
        }
    }

    pub(crate) fn set_status_wake(&self, wake: Rc<ChangeNotifier>) {
        let _prev = self.inner.status_wake.borrow_mut().replace(wake);
    }

    /// Point the secondary animation at the next route's primary animation.
    ///
    /// Reads the next route's `TransitionPeer` (its primary animation and its
    /// `canTransitionFrom`), gates on both predicates, and either points the proxy
    /// straight at it or — when the two animations are at different values and the
    /// target is moving — installs an [`AnimationSwitch`] that hops when they
    /// cross.
    fn update_secondary_animation(&self, next: Option<RouteId>) {
        let Some(binding) = self.inner.binding.get() else {
            return;
        };

        // Coordinate only when the next route is a transition route and both
        // `can_transition_*` predicates allow it. A non-transition route has no
        // peer, and a route of another family never coordinates — see
        // [`TransitionGroup`].
        let target = next.and_then(|id| binding.peer(id).map(|peer| (id, peer)));
        let Some((next_id, peer)) = target.filter(|(_, peer)| {
            self.can_transition_to && peer.can_transition_from && peer.group == self.group
        }) else {
            self.set_secondary(SecondaryParent::Dismissed, always_dismissed());
            return;
        };

        let mut parent = self.inner.secondary_parent.borrow_mut();

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
        let next_animation = Rc::clone(peer.animation());

        // Jump when the two trains are at the same value or the next one is not moving.
        //
        // **Not** `Animation::is_animating`, which for an `AnimationController` is
        // *overridden* to mean "the ticker is running", and stays true after a
        // controller has settled at `Completed`. Using the override here makes a
        // settled route look like a moving train and forces a spurious hop,
        // letting a stale train clobber a newer parent. The controller's
        // override is a separate, recorded divergence.
        let is_moving = matches!(
            next_animation.status(),
            AnimationStatus::Forward | AnimationStatus::Reverse
        );
        let jump = match &current_train {
            None => true,
            Some(train) => {
                (train.value() - next_animation.value()).abs() < f64::EPSILON || !is_moving
            }
        };

        let previous = std::mem::replace(&mut *parent, SecondaryParent::Dismissed);

        if jump {
            self.inner.secondary.set_parent(Rc::clone(&next_animation));
            *parent = SecondaryParent::Direct(next_id);
        } else {
            let train = current_train.expect("jump == false implies a current train");
            // Weak: the proxy parents this switch, so a strong capture would form
            // `proxy -> switch -> callback -> proxy` and outlive a route dropped
            // without `dispose`.
            let proxy = Rc::downgrade(&self.inner.secondary);
            let target_for_hop = super::lifecycle::Terminal::new(Rc::clone(&next_animation));
            let switch = AnimationSwitch::new(train, Some(Rc::clone(&next_animation)))
                // On the switch: point the proxy **directly** at the target and
                // drop the hopper.
                .on_switched(move || {
                    if let Some(proxy) = proxy.upgrade() {
                        proxy.set_parent(Rc::clone(&target_for_hop));
                    }
                });
            self.inner
                .secondary
                .set_parent(Rc::new(switch.clone()) as std::rc::Rc<dyn Animation<f64>>);
            *parent = SecondaryParent::Hopping {
                target: next_id,
                switch,
            };
        }

        // The old hopper is disposed only after its replacement exists.
        //
        // No test reaches this ordering: `ProxyAnimation::set_parent` re-subscribes
        // eagerly, so once the new parent is installed the proxy holds no reference
        // to the old hopper and disposing it early is invisible. Kept because it is
        // free; stated rather than claimed.
        drop(parent);
        if let SecondaryParent::Hopping { switch, .. } = previous {
            switch.dispose();
        }

        // Release the reference when the route above is disposed, but only if we
        // are still pointing at it — a stale disposal must not clobber a newer parent.
        let inner = Rc::downgrade(&self.inner);
        peer.completed.on_completed(Rc::new(move || {
            let Some(inner) = inner.upgrade() else { return };
            let mut parent = inner.secondary_parent.borrow_mut();
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

    fn set_secondary(&self, kind: SecondaryParent, animation: std::rc::Rc<dyn Animation<f64>>) {
        let previous = std::mem::replace(&mut *self.inner.secondary_parent.borrow_mut(), kind);
        self.inner.secondary.set_parent(animation);
        if let SecondaryParent::Hopping { switch, .. } = previous {
            switch.dispose();
        }
    }
}

/// A cloneable view of a [`TransitionRoute`]'s animation state.
#[derive(Clone)]
pub struct TransitionHandle {
    inner: Rc<TransitionInner>,
}

impl fmt::Debug for TransitionHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TransitionHandle").finish_non_exhaustive()
    }
}

impl TransitionHandle {
    /// The controller driving the route's **primary** animation, once `install`
    /// has created it.
    #[must_use]
    pub fn controller(&self) -> Option<AnimationController> {
        self.inner.controller()
    }

    /// The controller, erased. The always-dismissed animation before `install()` —
    /// a route that is not yet pushed has no controller.
    pub(crate) fn primary_animation(&self) -> std::rc::Rc<dyn Animation<f64>> {
        match self.controller() {
            Some(controller) => Rc::new(controller) as std::rc::Rc<dyn Animation<f64>>,
            None => always_dismissed(),
        }
    }

    /// This route's navigator capability, once it is pushed.
    pub(crate) fn binding(&self) -> Option<super::binding::RouteBinding> {
        self.inner.binding.get()
    }

    /// Apply animation statuses captured by the data-plane listener.
    ///
    /// This is owner-local by construction: callers are route/modal build paths
    /// or tests explicitly entering the owner scope. The status listener itself
    /// stores only data so `AnimationController` can stay `Send + Sync`.
    pub fn drain_pending_statuses(&self) {
        self.inner.drain_pending_statuses();
    }

    /// The secondary animation: a `ProxyAnimation` resting at the
    /// always-dismissed animation.
    #[must_use]
    pub fn secondary_animation(&self) -> Rc<ProxyAnimation<f64>> {
        Rc::clone(&self.inner.secondary)
    }

    /// Whether the pop has already been finalized.
    #[must_use]
    pub fn is_pop_finalized(&self) -> bool {
        self.inner.pop_finalized.load(Ordering::Acquire)
    }

    /// How many times the status listener raised `finalize()`.
    #[must_use]
    pub fn finalize_calls(&self) -> usize {
        self.inner.finalize_calls.load(Ordering::Relaxed)
    }

    /// Whether the secondary proxy currently rests at always-dismissed.
    #[must_use]
    pub fn secondary_is_dismissed(&self) -> bool {
        matches!(
            &*self.inner.secondary_parent.borrow_mut(),
            SecondaryParent::Dismissed
        )
    }

    /// Whether the secondary proxy is mid-hop (an `AnimationSwitch` is installed).
    #[must_use]
    pub fn secondary_is_hopping(&self) -> bool {
        matches!(
            &*self.inner.secondary_parent.borrow_mut(),
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

impl<T: Send + Clone + 'static> Route for TransitionRoute<T> {
    type Output = T;

    fn settings(&self) -> &RouteSettings {
        &self.settings
    }

    fn current_result(&mut self) -> Option<T> {
        self.current_result.clone()
    }

    /// Finished when the controller is dismissed and the pop is not yet finalized.
    ///
    /// False while the exit transition runs, so `handle_pop` leaves the entry in
    /// `Popping` and the overlay entry alive. True when the controller was
    /// **already** dismissed at pop time (the Cupertino dismiss gesture), which
    /// finalizes synchronously; `pop_finalized` then stops the status listener
    /// finalizing a second time.
    fn finished_when_popped(&self) -> bool {
        let dismissed = self
            .inner
            .controller()
            .as_ref()
            .is_some_and(AnimationController::is_dismissed);
        dismissed && !self.inner.pop_finalized.load(Ordering::Acquire)
    }

    /// Create the controller, attach the status listener, then let the overlay
    /// entry be created.
    ///
    /// The controller is created **here**, not in the constructor, which is what
    /// lets a route be constructed before it has a navigator.
    fn install(&mut self) {
        debug_assert!(
            self.inner.binding.is_bound(),
            "BUG: a TransitionRoute must be bound before install — \
             `NavigatorHandle::push` fills its `RouteBindingSlot` first"
        );

        let vsync = self.inner.binding.get().and_then(|binding| binding.vsync());
        let owner = AnimationController::builder(self.duration).build_on(vsync.as_ref());
        let controller = owner.controller().clone();
        if let Some(reverse) = self.reverse_duration {
            controller.set_reverse_duration(reverse);
        }

        let pending_statuses = Rc::clone(&self.inner.pending_statuses);
        let status_wake = self.inner.status_wake.borrow_mut().clone();
        controller.add_status_listener(std::rc::Rc::new(move |status| {
            pending_statuses.borrow_mut().push(status);
            if let Some(wake) = &status_wake {
                wake.notify_listeners();
            }
        }));

        // Publish the primary animation so the route below can coordinate.
        if let Some(binding) = self.inner.binding.get() {
            let weak = Rc::downgrade(&self.inner);
            binding.publish_peer(TransitionPeer {
                animation: Some(Rc::new(controller.clone()) as std::rc::Rc<dyn Animation<f64>>),
                can_transition_from: self.can_transition_from,
                group: self.group,
                completed: super::lifecycle::Terminal::new(Rc::clone(&self.inner.completed)),
                rebind_clock: Rc::new(move |vsync| {
                    if let Some(inner) = weak.upgrade() {
                        inner.rebind_clock(vsync);
                    }
                }),
            });

            // A controller that installs already completed never fires a status
            // change, so the status listener would never write `opaque`.
            if controller.is_completed() {
                binding.set_entry_opaque(self.inner.opaque.load(Ordering::Relaxed));
            }
        }

        let outgoing = self.inner.controller.borrow_mut().replace(owner);
        drop(outgoing);
    }

    /// Drive the controller forward and hand the navigator the resulting future.
    ///
    /// `forward()`'s only error is
    /// [`AnimationError::Disposed`](flui_animation::AnimationError::Disposed),
    /// which cannot occur here: `dispose()` `take()`s `self.inner.controller` before
    /// disposing it, and `did_push` cannot run after a route's own `dispose`
    /// (the flush that pushes a route always precedes the flush that could
    /// ever dispose it). Both failure shapes — no controller at all, or a
    /// disposed one — are therefore the same internal invariant, not two
    /// postures: one `expect`, not an `expect` plus a silently-degrading
    /// `error!` arm nothing can reach or test.
    fn did_push(&mut self) -> PushCompletion {
        let controller = self.inner.controller()
            .expect("BUG: install() runs before did_push and nothing disposes the controller before the push");
        let future = controller
            .forward()
            .expect("BUG: install() runs before did_push and nothing disposes the controller before the push");
        PushCompletion::Animating(future)
    }

    /// Jump to the end, no animation.
    fn did_add(&mut self) {
        if let Some(controller) = self.inner.controller() {
            controller.set_value(1.0);
        }
    }

    /// Drive the controller in reverse and consent. The route's `RouteResult` completes **now**, via
    /// `RouteRecord::did_pop`; only its disposal waits for `dismissed`.
    ///
    /// A gesture-driven pop rides its own pacing in here: `pop_paced` (see
    /// `navigator.rs`) publishes a one-shot `PopPacing`
    /// for exactly this route immediately before triggering the pop, and this
    /// is where it is consumed: the back gesture's drag end animates back with its
    /// own duration/curve instead of the route's plain `reverse()`. No pacing
    /// published (the ordinary, programmatic pop) falls back to the plain reverse.
    fn did_pop(&mut self) -> bool {
        self.inner.popped.store(true, Ordering::Release);
        let pacing = self
            .inner
            .binding
            .get()
            .and_then(|binding| binding.take_pop_pacing());
        if let Some(controller) = self.inner.controller() {
            let _ = match pacing {
                Some(pacing) => pacing.animate_back(&controller),
                None => controller.reverse(),
            };
        }
        true
    }

    /// Re-point the secondary animation at the new next route.
    fn did_change_next(&mut self, next: Option<RouteId>) {
        self.update_secondary_animation(next);
    }

    /// The route above was popped.
    ///
    /// The argument is the **popped** route, and the
    /// secondary is wired to *its* animation on purpose: this route animates back
    /// out as the one above reverses away. It is released when that route
    /// completes — see the module docs.
    fn did_pop_next(&mut self, popped: RouteId) {
        self.update_secondary_animation(Some(popped));
    }

    /// Detach the listener, unregister the
    /// clock, drop the peer, and dispose the controller **only if we own it**.
    fn dispose(&mut self) {
        self.inner.disposed.set(true);
        if let Some(binding) = self.inner.binding.get() {
            binding.withdraw_peer();
        }
        // Release every route below that is still proxying our animation, before
        // the controller is disposed under them.
        self.inner.completed.complete();
        self.set_secondary(SecondaryParent::Dismissed, always_dismissed());

        let outgoing = self.inner.controller.borrow_mut().take();
        drop(outgoing);
    }
}

impl<T: Send + Clone + 'static> NavigatorRoute for TransitionRoute<T> {
    fn content_builder(&self) -> RouteContentBuilder {
        Rc::clone(&self.builder)
    }

    fn binding_slot(&self) -> Option<&RouteBindingSlot> {
        Some(&self.inner.binding)
    }
}
