//! [`ScaffoldMessenger`] — manages [`crate::SnackBar`] queuing/display for
//! every registered [`crate::Scaffold`] descendant, plus [`SnackBarController`]
//! (returned by [`ScaffoldMessengerHandle::show_snack_bar`]) and
//! [`SnackBarClosedReason`].
//!
//! # Flutter parity
//!
//! `material/scaffold.dart`'s `ScaffoldMessenger`/`ScaffoldMessengerState`
//! (oracle tag `3.44.0`), narrowed to the snack-bar half of that type (no
//! `MaterialBanner` queue — a separate, undated feature). Every citation
//! below is `scaffold.dart` unless noted.
//!
//! ## Named divergence: `AnimationController`-as-timer
//!
//! The oracle drives the per-snackbar display duration with a real
//! `dart:async` `Timer` (`_snackBarTimer`, `:619`). FLUI has no
//! frame-independent virtual-clock timer primitive yet (`flui-scheduler`
//! only drives the frame loop) — see `docs/ROADMAP.md`. This substrate
//! substitutes a second, per-snackbar [`AnimationController`] whose
//! `duration` is the snackbar's own configured display duration
//! ([`crate::SnackBar::duration`]): it is `forward()`-ed when the entrance
//! animation completes, and its own `Completed` status stands in for the
//! oracle's `Timer` callback. **Honest cost**: unlike a real timer, this
//! keeps the frame loop scheduled for the full display duration (a `Vsync`
//! registration ticks every frame, not just once at expiry) — accepted
//! because it makes the duration trivially controllable under a virtual
//! clock in tests, the same reason `crate::drawer`'s settle animation
//! already rides this mechanism. An event-driven timer is a named follow-up
//! once `flui-scheduler` grows one. The timer restarts fresh for each
//! snackbar and is cancelled on hide/remove/clear
//! (`MessengerCore::cancel_display_timer`).
//!
//! ## The drain state machine
//!
//! `DrainState` makes the oracle's implicit `_snackBarController` status
//! machine explicit: `Idle` (nothing showing) → `Entering` (250ms forward) →
//! `Displayed` (entrance complete, display timer running) → `Exiting` (250ms
//! reverse) → back to `Idle`, which immediately re-enters if the queue is
//! still non-empty (`MessengerCore::pop_and_advance`).
//!
//! ## Why status edges are polled, not pushed, from the controller
//!
//! `AnimationController::add_status_listener` requires its callback to be
//! `Send + Sync` (`StatusCallback = Arc<dyn Fn(AnimationStatus) + Send +
//! Sync>`) because the controller itself is a general-purpose, thread-safe
//! primitive. `MessengerCore` is deliberately **not** `Send`/`Sync` — it is
//! `Rc`-based, owner-affine, the same reasoning
//! `crate::drawer::DrawerHandle`'s module doc gives (and its queue's
//! `on_closed` slots are plain `Box<dyn FnOnce(..)>`, not `+ Send`, matching
//! every other UI callback in this crate). So the controllers' own listeners
//! (installed in `ScaffoldMessengerHandle::attach`/
//! `MessengerCore::start_display_timer`) do the ONE thing a `Send + Sync`
//! closure can safely do without capturing `Rc` state: reschedule
//! [`ScaffoldMessenger`]'s own rebuild via a plain `RebuildHandle` (itself
//! `Send + Sync`).
//!
//! The actual state-machine translation — "did a controller's status change
//! since we last looked, and if so what does that mean for the queue" —
//! lives in `MessengerCore::reconcile`, called from two places: **directly,
//! synchronously**, at the end of every queue-mutating method
//! (`show_snack_bar`/`hide_current`/`remove_current`/`clear`) so an explicit
//! call observes its own effect immediately (no frame boundary needed — the
//! "remove twice rapidly" test below asserts on this), and from
//! [`ScaffoldMessengerState::build`] (scheduled by the Send-safe listeners)
//! for the natural case where a controller settles purely from ticking, with
//! no explicit API call in between. `reconcile` loops until a full pass
//! detects no further change, so it correctly drains a multi-step cascade
//! (e.g. `Dismissed` → pop → `forward()` → `Forward`) within one call.
//! `reconcile` never holds the `queue` `RefCell` borrowed across a call back
//! into the controller or into caller-supplied code
//! (`SnackBarController::on_closed`) — this crate has shipped five separate
//! `RefCell`/lock-held-over-closure incidents, and `forward()`/`reverse()`/
//! `set_value()` change status *synchronously*, so an `on_closed` callback
//! that itself calls back into `show_snack_bar`/`remove_current_snack_bar`
//! reaches this same machine again, from the same call stack.
//!
//! **Queue-wedge invariant**: *the queue is non-empty ⇒ the entrance
//! controller is not `Dismissed`, except transiently inside
//! `MessengerCore::pop_and_advance` itself.* Maintaining this is what lets
//! `MessengerCore::hide_current` simply early-return when the controller is
//! already `Dismissed` (oracle parity — nothing to hide). But
//! `MessengerCore::remove_current`/`MessengerCore::clear` have no such
//! early return in the oracle (`removeCurrentSnackBar` only checks
//! `_snackBars.isEmpty`) — calling `set_value(0.0)` on an ALREADY-`Dismissed`
//! controller is a **no-op status change**: `set_value` only fires a status
//! *transition* on an actual change, so an edge-only drain would silently
//! wedge the queue forever the moment `remove`/`clear` is called while the
//! controller is already settled at `Dismissed` (reachable via the exact
//! re-entrancy this module warns about: an `on_closed` callback that itself
//! calls `remove_current_snack_bar`). Both methods therefore check
//! `status() == Dismissed` themselves and, when true, directly call
//! `pop_and_advance` instead of routing through `set_value`.
//! `MessengerCore::advancing` guards `pop_and_advance` itself against
//! double-entry from that same re-entrant path.
//!
//! ## Multi-scaffold fan-out
//!
//! `show_snack_bar` shows the current entry on every registered
//! [`crate::Scaffold`] simultaneously (oracle: `_updateScaffolds` iterates
//! `_scaffolds`, `:231-238`) — [`crate::Scaffold`] itself reads
//! `ScaffoldMessengerHandle::current_entry` (private) in its own `build`. **Named
//! divergence**: the oracle additionally narrows this to the *root* scaffold
//! of a nested set (`_isRoot`, `:242-245`, comparing
//! `findAncestorStateOfType<ScaffoldState>`); this substrate has no
//! ancestor-state lookup by concrete `ViewState` type, so nested-`Scaffold`
//! double-show is not filtered — every registered scaffold shows the current
//! snack bar, nested or not. Tracked, not silently dropped.
//!
//! ## `clearSnackBars`
//!
//! Drops every queued (not-yet-shown) entry's `on_closed` **silently** — no
//! callback fires, matching the oracle dropping their `Completer`s
//! unfulfilled (`:463-472`; a never-completed `Future` observably never
//! resolves, the same as a callback that never runs). The current entry is
//! then hidden via `MessengerCore::hide_current` with
//! [`SnackBarClosedReason::Hide`] — `clearSnackBars` delegates to
//! `hideCurrentSnackBar()`, whose reason parameter defaults to
//! `SnackBarClosedReason.hide` (`:441`), *not* `.remove` as its name might
//! suggest.
//!
//! ## Completion slot
//!
//! Each queued entry's [`SnackBarClosedReason`] is recorded exactly once
//! (`QueuedEntry::set_reason_once`) at hide/remove/action time — a slot the
//! drain pop reads but never invents or overwrites (falling back to
//! [`SnackBarClosedReason::Remove`] only in the unreachable-in-practice case
//! of a pop with no reason ever recorded).
//!
//! ## V1 scope
//!
//! Reachable in V1: [`SnackBarClosedReason::Timeout`],
//! [`SnackBarClosedReason::Hide`], [`SnackBarClosedReason::Remove`],
//! [`SnackBarClosedReason::Action`]. [`SnackBarClosedReason::Dismiss`] and
//! [`SnackBarClosedReason::Swipe`] exist for oracle parity (a future
//! close-icon / swipe-to-dismiss feature reaches them) but nothing in this
//! crate produces them yet. `SnackBar.persist`/`ModalRoute`-pausing
//! (`:614-616`) are deferred, named — the display timer always runs,
//! regardless of whether the snack bar carries an action.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use flui_animation::{
    Animation, AnimationController, AnimationStatus, Scheduler, Vsync, VsyncRegistration,
};
use flui_foundation::ElementId;
use flui_view::prelude::*;
use flui_view::{RebuildHandle, impl_inherited_view};
use flui_widgets::animated::VsyncScope;

use crate::snack_bar::SnackBar;

/// The shared entrance/exit controller's duration — Flutter's
/// `_snackBarTransitionDuration` (`snack_bar.dart`).
const ENTRY_TRANSITION_DURATION: Duration = Duration::from_millis(250);

/// Specifies how a [`SnackBar`] was closed.
///
/// Flutter parity: `SnackBarClosedReason` (`snack_bar.dart`). See the module
/// docs' "V1 scope" section for which variants are reachable today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SnackBarClosedReason {
    /// Closed after the user pressed the [`crate::snack_bar::SnackBarAction`].
    Action,
    /// Closed through an accessibility dismiss action. Unreachable in V1.
    Dismiss,
    /// Closed by a user swipe. Unreachable in V1 (no swipe-to-dismiss yet).
    Swipe,
    /// Closed by [`ScaffoldMessengerHandle::hide_current_snack_bar`] — also
    /// what [`ScaffoldMessengerHandle::clear_snack_bars`] hides the
    /// surviving current entry with.
    Hide,
    /// Closed by [`ScaffoldMessengerHandle::remove_current_snack_bar`] —
    /// abrupt, no exit animation.
    Remove,
    /// Closed because its display-duration timer expired.
    Timeout,
}

/// The shared, once-only completion-callback slot [`SnackBarController`] and
/// [`QueuedEntry`] both hold a clone of — set by
/// [`SnackBarController::on_closed`], taken (and invoked) exactly once by
/// [`QueuedEntry::complete`].
type ClosedCallbackSlot = Rc<RefCell<Option<Box<dyn FnOnce(SnackBarClosedReason)>>>>;

/// Handle returned by [`ScaffoldMessengerHandle::show_snack_bar`] — lets the
/// caller register a one-shot callback for when this particular entry
/// closes.
///
/// Flutter parity: `ScaffoldFeatureController<SnackBar,
/// SnackBarClosedReason>` narrowed to the one capability V1 exposes: the
/// oracle's `Completer<SnackBarClosedReason>`/`Future` pair becomes a
/// synchronous one-shot callback slot (this substrate has no async
/// `Future`-in-the-view-tree plumbing to hang a `Future` on).
#[derive(Clone)]
pub struct SnackBarController {
    on_closed: ClosedCallbackSlot,
}

impl SnackBarController {
    /// Registers `callback`, run exactly once when this entry closes
    /// (whatever the reason) — never for a queued entry silently dropped by
    /// [`ScaffoldMessengerHandle::clear_snack_bars`] (see that method's
    /// doc). A later call replaces an earlier, unfired callback.
    pub fn on_closed(&self, callback: impl FnOnce(SnackBarClosedReason) + 'static) {
        *self.on_closed.borrow_mut() = Some(Box::new(callback));
    }
}

impl std::fmt::Debug for SnackBarController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SnackBarController").finish_non_exhaustive()
    }
}

/// One queued (or currently showing) snack bar plus its once-only completion
/// slot. See the module docs' "Completion slot" section.
struct QueuedEntry {
    snack_bar: SnackBar,
    reason: Cell<Option<SnackBarClosedReason>>,
    on_closed: ClosedCallbackSlot,
}

impl QueuedEntry {
    /// Records `reason` only if nothing has been recorded yet — the
    /// once-only guard the module docs describe.
    fn set_reason_once(&self, reason: SnackBarClosedReason) {
        if self.reason.get().is_none() {
            self.reason.set(Some(reason));
        }
    }

    /// Fires `on_closed` with the recorded reason (falling back to
    /// [`SnackBarClosedReason::Remove`] — see the module docs' "Completion
    /// slot" section for why this fallback is not expected to be reachable).
    fn complete(&self) {
        let reason = self.reason.get().unwrap_or(SnackBarClosedReason::Remove);
        if let Some(callback) = self.on_closed.borrow_mut().take() {
            callback(reason);
        }
    }

    /// Drops `on_closed` WITHOUT invoking it — `clear_snack_bars`' silent
    /// drop of a still-queued entry (oracle parity: an abandoned
    /// `Completer`).
    fn complete_silently(&self) {
        self.on_closed.borrow_mut().take();
    }
}

/// The explicit drain state — see the module docs' "The drain state
/// machine" section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DrainState {
    /// Nothing showing, queue empty (or about to be re-entered).
    Idle,
    /// The shared controller is running forward.
    Entering,
    /// The shared controller is at `1.0`; the display timer is running.
    Displayed,
    /// The shared controller is running in reverse.
    Exiting,
}

/// The messenger's interior-mutable state, `Rc`-shared between
/// [`ScaffoldMessengerHandle`] clones. See the module docs' "Why status
/// edges are polled, not pushed, from the controller" section for why this
/// type never appears inside an `AnimationController`'s own listener list.
struct MessengerCore {
    entry_controller: AnimationController,
    duration_controller: RefCell<Option<AnimationController>>,
    vsync: RefCell<Option<Vsync>>,
    entry_vsync_registration: RefCell<Option<VsyncRegistration>>,
    duration_vsync_registration: RefCell<Option<VsyncRegistration>>,
    /// [`ScaffoldMessenger`]'s own rebuild handle — cloned into both
    /// controllers' Send-safe "reschedule" listeners, so a purely
    /// tick-driven status settle (no explicit API call in between) still
    /// reaches [`Self::reconcile`] via [`ScaffoldMessengerState::build`].
    /// `None` until [`ScaffoldMessengerHandle::attach`] runs.
    rebuild: RefCell<Option<RebuildHandle>>,
    queue: RefCell<VecDeque<Rc<QueuedEntry>>>,
    state: Cell<DrainState>,
    last_entry_status: Cell<AnimationStatus>,
    last_duration_status: Cell<AnimationStatus>,
    /// Reentrancy guard for [`Self::pop_and_advance`] — see the module docs'
    /// "The drain state machine" section.
    advancing: Cell<bool>,
    scaffolds: RefCell<HashMap<ElementId, RebuildHandle>>,
}

impl MessengerCore {
    fn schedule_rebuild_on_scaffolds(&self) {
        for rebuild in self.scaffolds.borrow().values() {
            rebuild.schedule();
        }
    }

    /// Polls both controllers against the last-observed status and
    /// translates every change into a state transition/pop, looping until a
    /// full pass detects no further change. See the module docs' "Why
    /// status edges are polled, not pushed, from the controller" section.
    fn reconcile(&self) {
        loop {
            let mut changed = false;

            let entry_status = self.entry_controller.status();
            if entry_status != self.last_entry_status.get() {
                self.last_entry_status.set(entry_status);
                self.handle_entry_status(entry_status);
                changed = true;
            }

            let duration_status = self
                .duration_controller
                .borrow()
                .as_ref()
                .map(Animation::status);
            match duration_status {
                Some(status) if status != self.last_duration_status.get() => {
                    self.last_duration_status.set(status);
                    if status == AnimationStatus::Completed {
                        self.hide_current(SnackBarClosedReason::Timeout);
                    }
                    changed = true;
                }
                None => self.last_duration_status.set(AnimationStatus::Dismissed),
                Some(_) => {}
            }

            if !changed {
                break;
            }
        }
    }

    /// Translates one entrance-controller status into a state
    /// transition/pop.
    fn handle_entry_status(&self, status: AnimationStatus) {
        match status {
            AnimationStatus::Dismissed => self.pop_and_advance(),
            AnimationStatus::Completed => {
                self.state.set(DrainState::Displayed);
                self.start_display_timer();
            }
            AnimationStatus::Forward => self.state.set(DrainState::Entering),
            AnimationStatus::Reverse => self.state.set(DrainState::Exiting),
            // `AnimationStatus` is `#[non_exhaustive]`; every variant it has
            // today is handled above.
            _ => {}
        }
    }

    /// Pops the just-exited front entry (if any), fires its `on_closed`,
    /// then begins the next entry's entrance if the queue is still
    /// non-empty. The single place that ever pops the queue.
    fn pop_and_advance(&self) {
        if self.advancing.get() {
            // A re-entrant call from inside the `on_closed` callback this
            // very function is about to invoke below — the outer call owns
            // finishing the advance; see the module docs.
            return;
        }
        self.advancing.set(true);

        self.cancel_display_timer();
        let popped = self.queue.borrow_mut().pop_front();
        if let Some(entry) = popped {
            entry.complete();
        }

        self.state.set(DrainState::Idle);
        let has_next = !self.queue.borrow().is_empty();
        if has_next {
            self.state.set(DrainState::Entering);
            let _ = self.entry_controller.forward();
        }

        self.advancing.set(false);
        self.schedule_rebuild_on_scaffolds();
    }

    fn start_display_timer(&self) {
        let Some(front) = self.queue.borrow().front().cloned() else {
            return;
        };
        let controller = AnimationController::new(
            front.snack_bar.configured_duration(),
            Arc::new(Scheduler::new()),
        );
        if let Some(vsync) = self.vsync.borrow().as_ref() {
            let registration = vsync.register(controller.clone());
            *self.duration_vsync_registration.borrow_mut() = Some(registration);
        }
        if let Some(rebuild) = self.rebuild.borrow().clone() {
            controller.add_status_listener(Arc::new(move |_status| {
                rebuild.schedule();
            }));
        }
        self.last_duration_status.set(AnimationStatus::Dismissed);
        let _ = controller.forward();
        *self.duration_controller.borrow_mut() = Some(controller);
    }

    fn cancel_display_timer(&self) {
        if let Some(registration) = self.duration_vsync_registration.borrow_mut().take()
            && let Some(vsync) = self.vsync.borrow().as_ref()
        {
            vsync.unregister(registration);
        }
        if let Some(controller) = self.duration_controller.borrow_mut().take() {
            controller.dispose();
        }
        self.last_duration_status.set(AnimationStatus::Dismissed);
    }

    /// Flutter parity: `hideCurrentSnackBar` (`:441-459`), minus the
    /// `accessibleNavigation` immediate-complete branch (not ported — no
    /// `MediaQuery.accessibleNavigation` consumer exists yet in this
    /// substrate; always takes the animated-reverse path).
    fn hide_current(&self, reason: SnackBarClosedReason) {
        if self.entry_controller.status() == AnimationStatus::Dismissed {
            // Oracle: `if (_snackBars.isEmpty || controller.isDismissed)
            // return;` — nothing showing to hide, including the empty-queue
            // case (an empty queue always leaves the controller Dismissed
            // under this module's invariant).
            return;
        }
        let Some(front) = self.queue.borrow().front().cloned() else {
            return;
        };
        front.set_reason_once(reason);
        self.cancel_display_timer();
        let _ = self.entry_controller.reverse();
    }

    /// Flutter parity: `removeCurrentSnackBar` (`:424-436`) — no
    /// `isDismissed` early return in the oracle, which is exactly why this
    /// needs the direct-pop fallback the module docs describe.
    fn remove_current(&self, reason: SnackBarClosedReason) {
        let Some(front) = self.queue.borrow().front().cloned() else {
            return;
        };
        front.set_reason_once(reason);
        self.cancel_display_timer();
        if self.entry_controller.status() == AnimationStatus::Dismissed {
            self.pop_and_advance();
        } else {
            self.entry_controller.set_value(0.0);
        }
    }

    /// Flutter parity: `clearSnackBars` (`:463-472`) — see the module docs'
    /// "`clearSnackBars`" section for the exact reason `hide_current` closes
    /// the surviving current entry with.
    fn clear(&self) {
        if self.queue.borrow().is_empty()
            || self.entry_controller.status() == AnimationStatus::Dismissed
        {
            return;
        }
        let dropped: Vec<Rc<QueuedEntry>> = {
            let mut queue = self.queue.borrow_mut();
            let current = queue
                .pop_front()
                .expect("BUG: emptiness checked immediately above");
            let dropped = queue.drain(..).collect();
            queue.push_back(current);
            dropped
        };
        for entry in dropped {
            entry.complete_silently();
        }
        self.hide_current(SnackBarClosedReason::Hide);
    }
}

/// An owned, `Rc`-based (owner-affine, **not** `Send`/`Sync` — same reasoning
/// [`crate::drawer::DrawerHandle`]'s module doc gives) capability to show,
/// hide, remove, or clear [`SnackBar`]s across every registered
/// [`crate::Scaffold`]. Published via [`ScaffoldMessengerScope`]
/// (`ScaffoldMessengerScope::of`/`maybe_of`).
#[derive(Clone)]
pub struct ScaffoldMessengerHandle {
    shared: Rc<MessengerCore>,
}

impl std::fmt::Debug for ScaffoldMessengerHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScaffoldMessengerHandle")
            .finish_non_exhaustive()
    }
}

impl ScaffoldMessengerHandle {
    /// A handle with an empty queue and no `Vsync`/rebuild wiring yet —
    /// [`Self::attach`] (`ViewState::init_state`, per ADR-0018 — a
    /// frame-phase-only capability may not be acquired from `build`) does
    /// that.
    fn new() -> Self {
        let entry_controller =
            AnimationController::new(ENTRY_TRANSITION_DURATION, Arc::new(Scheduler::new()));
        let shared = Rc::new(MessengerCore {
            entry_controller,
            duration_controller: RefCell::new(None),
            vsync: RefCell::new(None),
            entry_vsync_registration: RefCell::new(None),
            duration_vsync_registration: RefCell::new(None),
            rebuild: RefCell::new(None),
            queue: RefCell::new(VecDeque::new()),
            state: Cell::new(DrainState::Idle),
            last_entry_status: Cell::new(AnimationStatus::Dismissed),
            last_duration_status: Cell::new(AnimationStatus::Dismissed),
            advancing: Cell::new(false),
            scaffolds: RefCell::new(HashMap::new()),
        });
        Self { shared }
    }

    /// Wires the ambient `Vsync` and installs the entrance controller's
    /// Send-safe "reschedule [`ScaffoldMessenger`]'s rebuild" listener — see
    /// [`Self::new`]'s doc for why this is deferred out of construction.
    pub(crate) fn attach(&self, ctx: &dyn BuildContext) {
        let rebuild = ctx.rebuild_handle();
        let rebuild_for_listener = rebuild.clone();
        self.shared
            .entry_controller
            .add_status_listener(Arc::new(move |_status| {
                rebuild_for_listener.schedule();
            }));
        *self.shared.rebuild.borrow_mut() = Some(rebuild);

        let vsync = ctx.get::<VsyncScope, _>(|scope| scope.vsync().clone());
        if let Some(vsync) = &vsync {
            let registration = vsync.register(self.shared.entry_controller.clone());
            *self.shared.entry_vsync_registration.borrow_mut() = Some(registration);
        }
        *self.shared.vsync.borrow_mut() = vsync;
    }

    /// Unregisters from `Vsync` and disposes both controllers.
    pub(crate) fn detach(&self) {
        self.shared.cancel_display_timer();
        if let Some(registration) = self.shared.entry_vsync_registration.borrow_mut().take()
            && let Some(vsync) = self.shared.vsync.borrow_mut().take()
        {
            vsync.unregister(registration);
        }
        self.shared.entry_controller.dispose();
    }

    /// Re-runs the state-machine reconciliation — called from
    /// [`ScaffoldMessengerState::build`] whenever the Send-safe listeners
    /// scheduled a rebuild.
    pub(crate) fn reconcile(&self) {
        self.shared.reconcile();
    }

    /// Registers a [`crate::Scaffold`] so it receives `schedule_rebuild`
    /// calls whenever the current entry's identity changes. Idempotent —
    /// keyed by [`ElementId`], a later call with the same id simply replaces
    /// the stored [`RebuildHandle`]. If a snack bar is already
    /// showing/queued, schedules an immediate rebuild so the newly
    /// registered scaffold picks it up (Flutter parity: `_register`,
    /// `:211-223`).
    pub(crate) fn register_scaffold(&self, element_id: ElementId, rebuild: RebuildHandle) {
        if !self.shared.queue.borrow().is_empty() {
            rebuild.schedule();
        }
        self.shared
            .scaffolds
            .borrow_mut()
            .insert(element_id, rebuild);
    }

    /// Unregisters a [`crate::Scaffold`] — called from its `dispose`.
    pub(crate) fn unregister_scaffold(&self, element_id: ElementId) {
        self.shared.scaffolds.borrow_mut().remove(&element_id);
    }

    /// The number of currently-registered [`crate::Scaffold`]s — mainly a
    /// test seam proving `Self::unregister_scaffold` actually ran (an
    /// unmounted element's stale `RebuildHandle` would silently no-op
    /// forever either way, per that type's own doc, so a mere
    /// absence-of-panic assertion cannot distinguish "unregistered" from
    /// "leaked").
    #[must_use]
    pub fn registered_scaffold_count(&self) -> usize {
        self.shared.scaffolds.borrow().len()
    }

    /// Whether `self` and `other` name the same underlying messenger — the
    /// `Rc::ptr_eq` identity check [`crate::Scaffold`]'s
    /// `did_change_dependencies` uses to decide whether to re-home its
    /// registration.
    #[must_use]
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.shared, &other.shared)
    }

    /// The entry every registered [`crate::Scaffold`] should currently
    /// display, if any: the config plus a clone of the shared entrance/exit
    /// controller driving its height animation.
    #[must_use]
    pub(crate) fn current_entry(&self) -> Option<(SnackBar, AnimationController)> {
        self.shared.queue.borrow().front().map(|entry| {
            (
                entry.snack_bar.clone(),
                self.shared.entry_controller.clone(),
            )
        })
    }

    /// Shows `snack_bar` across every registered [`crate::Scaffold`]. If one
    /// is already showing, `snack_bar` queues behind it (FIFO) and is shown
    /// once every earlier entry has closed.
    ///
    /// Flutter parity: `showSnackBar` (`:314-384`), narrowed to the one
    /// snack bar per call this substrate exposes (no `snackBarAnimationStyle`
    /// override — the transition duration is fixed at
    /// `ENTRY_TRANSITION_DURATION`).
    pub fn show_snack_bar(&self, snack_bar: SnackBar) -> SnackBarController {
        let on_closed = Rc::new(RefCell::new(None));
        let entry = Rc::new(QueuedEntry {
            snack_bar,
            reason: Cell::new(None),
            on_closed: Rc::clone(&on_closed),
        });

        let should_enter = {
            let mut queue = self.shared.queue.borrow_mut();
            queue.push_back(entry);
            queue.len() == 1
        };
        if should_enter {
            self.shared.state.set(DrainState::Entering);
            let _ = self.shared.entry_controller.forward();
            self.shared.schedule_rebuild_on_scaffolds();
        }
        self.shared.reconcile();

        SnackBarController { on_closed }
    }

    /// Removes the current snack bar (if any) by running its normal exit
    /// animation, with [`SnackBarClosedReason::Hide`]. A no-op if nothing is
    /// currently showing.
    pub fn hide_current_snack_bar(&self) {
        self.shared.hide_current(SnackBarClosedReason::Hide);
        self.shared.reconcile();
    }

    /// Reason-carrying counterpart of [`Self::hide_current_snack_bar`], for
    /// callers inside this crate that close with a specific reason (e.g.
    /// [`crate::snack_bar::SnackBarAction`]'s single-fire press).
    pub(crate) fn hide_current_snack_bar_because(&self, reason: SnackBarClosedReason) {
        self.shared.hide_current(reason);
        self.shared.reconcile();
    }

    /// Removes the current snack bar (if any) immediately, with no exit
    /// animation, with [`SnackBarClosedReason::Remove`]. If any snack bars
    /// are queued, the next begins its entrance immediately.
    pub fn remove_current_snack_bar(&self) {
        self.shared.remove_current(SnackBarClosedReason::Remove);
        self.shared.reconcile();
    }

    /// Drops every queued (not-yet-shown) snack bar silently (their
    /// `on_closed` never fires) and hides the current one with its normal
    /// exit animation. See the module docs' "`clearSnackBars`" section.
    pub fn clear_snack_bars(&self) {
        self.shared.clear();
        self.shared.reconcile();
    }
}

/// Publishes a [`ScaffoldMessengerHandle`] to its subtree.
///
/// Flutter parity: `_ScaffoldMessengerScope` (`scaffold.dart`).
#[derive(Clone)]
pub struct ScaffoldMessengerScope {
    handle: ScaffoldMessengerHandle,
    child: BoxedView,
}

impl std::fmt::Debug for ScaffoldMessengerScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScaffoldMessengerScope")
            .finish_non_exhaustive()
    }
}

impl ScaffoldMessengerScope {
    /// The nearest ancestor [`ScaffoldMessenger`]'s handle.
    ///
    /// # Panics
    ///
    /// Panics if there is no [`ScaffoldMessenger`] ancestor. Use
    /// [`maybe_of`](Self::maybe_of) for a non-panicking variant.
    #[must_use]
    pub fn of(ctx: &dyn BuildContext) -> ScaffoldMessengerHandle {
        Self::maybe_of(ctx).expect(
            "ScaffoldMessengerScope::of called with no ScaffoldMessenger ancestor in the tree — \
             wrap the subtree in a ScaffoldMessenger, or use ScaffoldMessengerScope::maybe_of \
             with a caller-chosen fallback",
        )
    }

    /// Looks up the nearest ancestor [`ScaffoldMessenger`]'s handle, or
    /// `None` if there is none.
    ///
    /// No dependency is registered — same reasoning `crate::ScaffoldScope::maybe_of`'s
    /// doc gives for `DrawerHandle`: the handle is a stable capability
    /// object, not a value whose identity changing should trigger a rebuild.
    #[must_use]
    pub fn maybe_of(ctx: &dyn BuildContext) -> Option<ScaffoldMessengerHandle> {
        ctx.get::<Self, _>(|scope| scope.handle.clone())
    }
}

impl InheritedView for ScaffoldMessengerScope {
    type Data = ScaffoldMessengerHandle;

    fn data(&self) -> &Self::Data {
        &self.handle
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, _old: &Self) -> bool {
        // The handle is the same stable object across every rebuild — see
        // `Self::maybe_of`'s doc.
        false
    }
}

impl_inherited_view!(ScaffoldMessengerScope);

/// Manages [`SnackBar`] queuing/display for every registered
/// [`crate::Scaffold`] descendant. Mount once, above every [`crate::Scaffold`]
/// that should share a queue.
///
/// Flutter parity: `ScaffoldMessenger` (`scaffold.dart`, oracle tag `3.44.0`).
///
/// # Examples
///
/// ```rust
/// use flui_material::{Scaffold, ScaffoldMessenger};
/// use flui_widgets::Text;
///
/// let _app = ScaffoldMessenger::new(Scaffold::new().body(Text::new("Hello")));
/// ```
#[derive(Clone, StatefulView)]
pub struct ScaffoldMessenger {
    child: BoxedView,
}

impl ScaffoldMessenger {
    /// Wraps `child`, publishing a fresh [`ScaffoldMessengerHandle`] to its
    /// subtree.
    #[must_use]
    pub fn new(child: impl IntoView) -> Self {
        Self {
            child: child.into_view().boxed(),
        }
    }
}

impl std::fmt::Debug for ScaffoldMessenger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScaffoldMessenger").finish_non_exhaustive()
    }
}

/// Persistent state behind [`ScaffoldMessenger`] — owns the
/// [`ScaffoldMessengerHandle`] for the widget's whole life.
pub struct ScaffoldMessengerState {
    handle: ScaffoldMessengerHandle,
}

impl std::fmt::Debug for ScaffoldMessengerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScaffoldMessengerState")
            .finish_non_exhaustive()
    }
}

impl StatefulView for ScaffoldMessenger {
    type State = ScaffoldMessengerState;

    fn create_state(&self) -> Self::State {
        ScaffoldMessengerState {
            handle: ScaffoldMessengerHandle::new(),
        }
    }
}

impl ViewState<ScaffoldMessenger> for ScaffoldMessengerState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        self.handle.attach(ctx);
    }

    fn build(&self, view: &ScaffoldMessenger, _ctx: &dyn BuildContext) -> impl IntoView {
        // Absorbs any tick-driven status settle the Send-safe listeners
        // scheduled this rebuild for — see the module docs' "Why status
        // edges are polled, not pushed, from the controller" section.
        self.handle.reconcile();
        ScaffoldMessengerScope {
            handle: self.handle.clone(),
            child: view.child.clone(),
        }
    }

    fn dispose(&mut self) {
        self.handle.detach();
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use flui_widgets::Text;

    use super::*;

    fn snack_bar(label: &str) -> SnackBar {
        SnackBar::new(Text::new(label.to_string()))
    }

    #[test]
    fn show_snack_bar_on_an_empty_queue_starts_entering() {
        let handle = ScaffoldMessengerHandle::new();
        handle.show_snack_bar(snack_bar("a"));
        assert_eq!(handle.shared.state.get(), DrainState::Entering);
        assert_eq!(
            handle.shared.entry_controller.status(),
            AnimationStatus::Forward
        );
    }

    #[test]
    fn show_snack_bar_while_draining_queues_without_touching_the_controller() {
        let handle = ScaffoldMessengerHandle::new();
        handle.show_snack_bar(snack_bar("a"));
        let value_before = handle.shared.entry_controller.value();
        handle.show_snack_bar(snack_bar("b"));
        assert_eq!(handle.shared.queue.borrow().len(), 2);
        assert_eq!(handle.shared.entry_controller.value(), value_before);
    }

    #[test]
    fn fifo_drain_shows_each_entry_once_in_order() {
        let handle = ScaffoldMessengerHandle::new();
        let order = Rc::new(RefCell::new(Vec::new()));

        let order_a = Rc::clone(&order);
        handle
            .show_snack_bar(snack_bar("a"))
            .on_closed(move |reason| order_a.borrow_mut().push(("a", reason)));
        let order_b = Rc::clone(&order);
        handle
            .show_snack_bar(snack_bar("b"))
            .on_closed(move |reason| order_b.borrow_mut().push(("b", reason)));

        // "a" entering.
        assert_eq!(
            handle.shared.entry_controller.status(),
            AnimationStatus::Forward
        );
        handle.shared.entry_controller.set_value(1.0); // settle "a"'s entrance -> Completed
        handle.shared.reconcile();
        assert_eq!(handle.shared.state.get(), DrainState::Displayed);

        handle.remove_current_snack_bar(); // "a" removed abruptly -> "b" starts entering
        assert_eq!(
            order.borrow().as_slice(),
            &[("a", SnackBarClosedReason::Remove)]
        );
        assert_eq!(
            handle.shared.entry_controller.status(),
            AnimationStatus::Forward
        );

        handle.remove_current_snack_bar(); // "b" removed abruptly -> queue empty
        assert_eq!(
            order.borrow().as_slice(),
            &[
                ("a", SnackBarClosedReason::Remove),
                ("b", SnackBarClosedReason::Remove)
            ]
        );
        assert_eq!(
            handle.shared.entry_controller.status(),
            AnimationStatus::Dismissed
        );
        assert!(handle.shared.queue.borrow().is_empty());
    }

    #[test]
    fn hide_current_snack_bar_reverses_instead_of_jumping() {
        let handle = ScaffoldMessengerHandle::new();
        handle.show_snack_bar(snack_bar("a"));
        handle.shared.entry_controller.set_value(1.0);
        handle.shared.reconcile();

        handle.hide_current_snack_bar();

        assert_eq!(
            handle.shared.entry_controller.status(),
            AnimationStatus::Reverse
        );
        assert_eq!(handle.shared.state.get(), DrainState::Exiting);
        // Still queued — the entry pops only once the reverse animation
        // actually settles at Dismissed.
        assert_eq!(handle.shared.queue.borrow().len(), 1);
    }

    #[test]
    fn hide_current_snack_bar_on_an_empty_queue_is_a_no_op() {
        let handle = ScaffoldMessengerHandle::new();
        handle.hide_current_snack_bar(); // must not panic
        assert_eq!(
            handle.shared.entry_controller.status(),
            AnimationStatus::Dismissed
        );
    }

    /// Remove the current entry, then remove again immediately (before any
    /// tick) — the second call's target must observably differ from the
    /// first, and neither entry may go unclosed. This does NOT by itself
    /// exercise `MessengerCore::remove_current`'s already-`Dismissed`
    /// fallback branch (each `remove_current_snack_bar` call's own trailing
    /// `reconcile()` already fully drains and re-advances the controller
    /// before the next call begins, so it never observes an already-settled
    /// controller here) — see
    /// `remove_current_reentrantly_from_on_closed_still_drains_the_queue`
    /// for a test that reaches that branch through genuine re-entrancy, and
    /// `remove_current_direct_pops_when_the_controller_is_already_dismissed`
    /// for the direct (non-reentrant) red-check on that branch.
    #[test]
    fn remove_twice_rapidly_drains_both_entries() {
        let handle = ScaffoldMessengerHandle::new();
        let closed = Rc::new(RefCell::new(0));

        let closed_a = Rc::clone(&closed);
        handle
            .show_snack_bar(snack_bar("a"))
            .on_closed(move |_| *closed_a.borrow_mut() += 1);
        let closed_b = Rc::clone(&closed);
        handle
            .show_snack_bar(snack_bar("b"))
            .on_closed(move |_| *closed_b.borrow_mut() += 1);

        handle.remove_current_snack_bar();
        handle.remove_current_snack_bar();

        assert_eq!(*closed.borrow(), 2, "both queued entries must have closed");
        assert!(
            handle.shared.queue.borrow().is_empty(),
            "the queue must not wedge"
        );
        assert_eq!(
            handle.shared.entry_controller.status(),
            AnimationStatus::Dismissed
        );
    }

    /// The wedge pin, direct: call `remove_current` while the controller is
    /// ALREADY `Dismissed` with a non-empty queue (reproducing, without real
    /// re-entrancy, the state the module docs describe) and confirm the
    /// queue still drains rather than sitting forever with a
    /// recorded-but-unfired reason.
    #[test]
    fn remove_current_direct_pops_when_the_controller_is_already_dismissed() {
        let handle = ScaffoldMessengerHandle::new();
        handle
            .shared
            .queue
            .borrow_mut()
            .push_back(Rc::new(QueuedEntry {
                snack_bar: snack_bar("a"),
                reason: Cell::new(None),
                on_closed: Rc::new(RefCell::new(None)),
            }));
        assert_eq!(
            handle.shared.entry_controller.status(),
            AnimationStatus::Dismissed
        );

        handle.remove_current_snack_bar();

        assert!(handle.shared.queue.borrow().is_empty());
    }

    /// The queue-wedge pin, via genuine re-entrancy: an `on_closed` callback
    /// that itself calls `remove_current_snack_bar` reaches
    /// `MessengerCore::remove_current` while `entry_controller` is ALREADY
    /// `Dismissed` (the outer `pop_and_advance` hasn't yet reached its own
    /// `forward()` call for the next entry) — exactly the scenario the
    /// module docs' "Queue-wedge invariant" section describes.
    ///
    /// Red-check A: remove the `status() == Dismissed` branch from
    /// `MessengerCore::remove_current` (always call `set_value(0.0)`) — this
    /// test fails: the reentrant call's `set_value(0.0)` is a genuine no-op
    /// (status unchanged, so no edge fires and nothing advances past
    /// `Dismissed`) — the assertion on `entry_controller.status()` below
    /// reads `Dismissed` instead of `Forward`.
    ///
    /// Red-check B: remove `MessengerCore::advancing`'s guard in
    /// `pop_and_advance` — this test fails differently: the reentrant call's
    /// direct-pop path now ALSO pops "b" (queue empties before the OUTER
    /// `pop_and_advance` resumes), so the outer call's own `has_next` check
    /// finds nothing to `forward()` — "b" is silently dropped without ever
    /// entering, and `entry_controller.status()` again reads `Dismissed`
    /// instead of `Forward`.
    #[test]
    fn remove_current_reentrantly_from_on_closed_still_drains_the_queue() {
        let handle = ScaffoldMessengerHandle::new();
        let order = Rc::new(RefCell::new(Vec::new()));

        let order_a = Rc::clone(&order);
        let handle_for_reentrant_call = handle.clone();
        handle
            .show_snack_bar(snack_bar("a"))
            .on_closed(move |reason| {
                order_a.borrow_mut().push(("a", reason));
                // Reentrant: fires while `entry_controller` is still
                // `Dismissed` from THIS SAME `set_value` call, before the
                // outer `pop_and_advance` has forwarded to "b".
                handle_for_reentrant_call.remove_current_snack_bar();
            });
        let order_b = Rc::clone(&order);
        handle
            .show_snack_bar(snack_bar("b"))
            .on_closed(move |reason| order_b.borrow_mut().push(("b", reason)));

        handle.shared.entry_controller.set_value(1.0); // settle "a"'s entrance
        handle.shared.reconcile();

        handle.remove_current_snack_bar(); // triggers the reentrant chain above

        assert_eq!(
            order.borrow().as_slice(),
            &[("a", SnackBarClosedReason::Remove)],
            "\"a\" must close exactly once; \"b\" must NOT have closed yet — it was re-tagged \
             Remove while still queued, not popped"
        );
        assert_eq!(
            handle.shared.queue.borrow().len(),
            1,
            "\"b\" must still be queued, not dropped"
        );
        assert_eq!(
            handle.shared.entry_controller.status(),
            AnimationStatus::Forward,
            "\"b\" must have started entering — the outer pop_and_advance's forward() call must \
             not be skipped because of the reentrant call"
        );

        // "b" was pre-tagged Remove by the reentrant call before it ever
        // entered — the once-only guard means its eventual close still
        // reports Remove, not whatever later closes it.
        handle.shared.entry_controller.set_value(1.0);
        handle.shared.reconcile();
        handle.remove_current_snack_bar();
        assert_eq!(
            order.borrow().as_slice(),
            &[
                ("a", SnackBarClosedReason::Remove),
                ("b", SnackBarClosedReason::Remove)
            ]
        );
    }

    #[test]
    fn clear_snack_bars_drops_queued_entries_silently_and_hides_the_current_one() {
        let handle = ScaffoldMessengerHandle::new();
        let current_closed = Rc::new(RefCell::new(None));
        let queued_closed = Rc::new(RefCell::new(false));

        let current_closed_for_cb = Rc::clone(&current_closed);
        handle
            .show_snack_bar(snack_bar("current"))
            .on_closed(move |reason| *current_closed_for_cb.borrow_mut() = Some(reason));
        let queued_closed_for_cb = Rc::clone(&queued_closed);
        handle
            .show_snack_bar(snack_bar("queued"))
            .on_closed(move |_| *queued_closed_for_cb.borrow_mut() = true);

        handle.shared.entry_controller.set_value(1.0); // "current" fully shown
        handle.shared.reconcile();

        handle.clear_snack_bars();

        assert_eq!(
            handle.shared.queue.borrow().len(),
            1,
            "only the current entry survives"
        );
        assert_eq!(
            handle.shared.entry_controller.status(),
            AnimationStatus::Reverse
        );
        assert!(
            !*queued_closed.borrow(),
            "a queued (never-shown) entry's on_closed must not fire"
        );
        assert!(
            current_closed.borrow().is_none(),
            "current entry closes when the reverse settles, not immediately"
        );

        handle.shared.entry_controller.set_value(0.0); // settle the reverse
        handle.shared.reconcile();
        assert_eq!(*current_closed.borrow(), Some(SnackBarClosedReason::Hide));
    }

    #[test]
    fn clear_snack_bars_on_an_empty_queue_is_a_no_op() {
        let handle = ScaffoldMessengerHandle::new();
        handle.clear_snack_bars();
        assert_eq!(
            handle.shared.entry_controller.status(),
            AnimationStatus::Dismissed
        );
    }

    /// Racing timeout + hide: the display timer's `Completed` edge fires
    /// first (recording `Timeout` and starting the exit reverse); a later
    /// `hide_current_snack_bar()` call before that reverse settles must NOT
    /// overwrite the already-recorded reason.
    ///
    /// Red-check: drop the once-only guard from `QueuedEntry::set_reason_once`
    /// (always overwrite) — this test fails: the final reason reads `Hide`,
    /// not `Timeout`.
    #[test]
    fn reason_is_recorded_once_under_racing_timeout_and_hide() {
        let handle = ScaffoldMessengerHandle::new();
        let reason = Rc::new(RefCell::new(None));
        let reason_for_cb = Rc::clone(&reason);
        handle
            .show_snack_bar(snack_bar("a"))
            .on_closed(move |r| *reason_for_cb.borrow_mut() = Some(r));

        handle.shared.entry_controller.set_value(1.0); // fully shown, display timer starts
        handle.shared.reconcile();
        assert_eq!(handle.shared.state.get(), DrainState::Displayed);

        // The display timer expires first: reconcile observes its Completed
        // edge, records Timeout, and starts the exit reverse.
        handle
            .shared
            .duration_controller
            .borrow()
            .as_ref()
            .expect("Displayed state must have started the display timer")
            .set_value(1.0);
        handle.shared.reconcile();
        assert_eq!(
            handle.shared.entry_controller.status(),
            AnimationStatus::Reverse
        );

        // A racing hide call before the reverse settles must not steal the
        // already-recorded reason.
        handle.hide_current_snack_bar();

        handle.shared.entry_controller.set_value(0.0); // settle the reverse
        handle.shared.reconcile();

        assert_eq!(*reason.borrow(), Some(SnackBarClosedReason::Timeout));
    }
}
