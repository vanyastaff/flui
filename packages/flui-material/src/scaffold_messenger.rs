//! [`ScaffoldMessenger`] — manages [`crate::SnackBar`] queuing/display for
//! every registered [`crate::Scaffold`] descendant, plus [`SnackBarController`]
//! (returned by [`ScaffoldMessengerHandle::show_snack_bar`]) and
//! [`SnackBarClosedReason`].
//!
//! # Scope
//!
//! Narrowed to the snack-bar half of a full messenger (no `MaterialBanner`
//! queue — a separate, undated feature).
//!
//! ## Named divergence: `AnimationController`-as-timer
//!
//! A real timer would drive the per-snackbar display duration. FLUI has no
//! frame-independent virtual-clock timer primitive yet (`flui-scheduler`
//! only drives the frame loop) — see `docs/ROADMAP.md`. This substrate
//! substitutes a second, per-snackbar [`AnimationController`] whose
//! `duration` is the snackbar's own configured display duration
//! ([`crate::SnackBar::duration`]): it is `forward()`-ed when the entrance
//! animation completes, and its own `Completed` status stands in for the
//! timer callback. **Honest cost**: unlike a real timer, this
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
//! There is no separate `enum` tracking Idle/Entering/Displayed/Exiting: the
//! entrance controller's own [`AnimationStatus`] already carries every bit of
//! that state (`Dismissed` = idle, `Forward` = entering, `Completed` =
//! displayed, `Reverse` = exiting) with the queue as the one piece it does
//! not — an earlier version of this module shadowed that status in a
//! `DrainState` cell, which nothing ever read except its own tests, exactly
//! the "two sources of truth, one inert" shape this module now avoids.
//! `MessengerCore::handle_entry_status` translates a *change* in that status
//! into the one queue-affecting consequence it has (`Dismissed` → pop and
//! advance to the next entry; `Completed` → start the display timer) — `pop
//! and advance` on `Dismissed`, then immediately re-entering if the queue is
//! still non-empty, IS the state machine.
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
//! ## Deferring `on_closed` out of the build phase
//!
//! `MessengerCore::reconcile` runs from two kinds of call site with
//! different obligations, captured in `ReconcileOrigin`:
//!
//! - **`ReconcileOrigin::Direct`** — synchronously, at the end of every
//!   public [`ScaffoldMessengerHandle`] method (`show_snack_bar`/
//!   `hide_current_snack_bar`/`remove_current_snack_bar`/`clear_snack_bars`),
//!   which only ever run from event-handler call stacks. `on_closed` fires
//!   immediately here — removal completes synchronously, in the same call.
//! - **`ReconcileOrigin::Build`** — from [`ScaffoldMessengerState::build`],
//!   reached when the Send-safe controller listeners scheduled a rebuild for
//!   a PURELY tick-driven status settle (no explicit API call in between —
//!   e.g. the entrance animation finishing, or the display timer expiring
//!   and starting the exit reverse, which later settles on its own). Firing
//!   arbitrary caller-supplied `on_closed` code inline, mid-`build`, is the
//!   same hazard that keeps `rebuild_handle`/`post_frame_handle` out of
//!   `build`: an `on_closed` that calls `show_snack_bar` would mutate the
//!   queue and schedule further rebuilds *after* this build's siblings have
//!   already built against the pre-mutation tree, silently.
//!   `MessengerCore::pop_and_advance` instead defers the fire through the
//!   [`flui_sdk::view::LocalPostFrameHandle`] acquired in
//!   [`ScaffoldMessengerState::init_state`] (ADR-0021) — the callback runs
//!   after this frame's build/layout/paint have committed, its own reentrant
//!   `show_snack_bar`/etc. call landing squarely in a safe, ordinary
//!   event-handler-shaped window. If the owner-local lane is unavailable or
//!   closed, completion is cancelled with a diagnostic; it never falls back
//!   to executing arbitrary user code during build. Disposing the messenger
//!   also cancels unfired callbacks, including ones already scheduled.
//!
//! State BOOKKEEPING (which entry is at the front, the recorded reason,
//! whether the display timer is running) is safe to mutate from `build` —
//! only the arbitrary-code DISPATCH of `on_closed` needs to leave it.
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
//! `SnackBarClosedReason.hide`, *not* `.remove` as its name might
//! suggest.
//!
//! ## Completion slot
//!
//! Each queued entry carries a `reason` cell the eventual pop reads (falling
//! back to [`SnackBarClosedReason::Remove`] only in the unreachable-in-
//! practice case of a pop with no reason ever recorded). It is set two
//! different ways, an intentional asymmetry:
//!
//! - `QueuedEntry::set_reason_once` (hide, timeout) — **provisional**: only
//!   applies if nothing has been recorded yet. A hide's completion is
//!   itself deferred until the reverse animation actually settles, so
//!   whatever reason is recorded here is only a promise about what a LATER
//!   completion will carry, not a completion itself.
//! - `QueuedEntry::set_reason` (remove) — **unconditional overwrite**.
//!   A remove completes the still-pending completion with ITS OWN reason
//!   immediately — a race it always wins against a hide/timeout that recorded a reason
//!   but has not yet actually completed (settled to `Dismissed`). So a
//!   `remove_current_snack_bar()` call arriving mid-reverse (after an
//!   earlier `hide_current_snack_bar()`) must report `Remove`, not the
//!   hide's own `Hide`.
//!
//! Both a provisional (hide/timeout) and a completed (remove/action) call
//! only ever *record* a reason on the entry — see the previous section for
//! when that recording turns into an actual `on_closed` fire.
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
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::time::Duration;

use flui_sdk::animation::{
    Animation, AnimationController, AnimationStatus, UpdateScheduler, Vsync, VsyncRegistration,
};
use flui_sdk::foundation::ElementId;
use flui_sdk::view::LocalPostFrameHandle;
use flui_sdk::view::prelude::*;
use flui_sdk::view::{RebuildHandle, impl_inherited_view};
use flui_sdk::widgets::animated::VsyncScope;

use crate::snack_bar::SnackBar;

/// The shared entrance/exit controller's duration.
const ENTRY_TRANSITION_DURATION: Duration = Duration::from_millis(250);

/// Specifies how a [`SnackBar`] was closed.
///
/// See the module docs' "V1 scope" section for which variants are
/// reachable today.
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
/// Narrowed to the one capability V1 exposes: the completion is a
/// synchronous one-shot callback slot (this substrate has no async
/// `Future`-in-the-view-tree plumbing to hang a `Future` on).
#[derive(Clone)]
pub struct SnackBarController {
    on_closed: ClosedCallbackSlot,
    messenger: Weak<MessengerCore>,
}

impl SnackBarController {
    /// Registers `callback`, run exactly once when this entry closes
    /// (whatever the reason) — never for a queued entry silently dropped by
    /// [`ScaffoldMessengerHandle::clear_snack_bars`] (see that method's
    /// doc). A later call replaces an earlier, unfired callback.
    /// The event context belongs to this messenger's presentation. Disposal
    /// cancels callbacks that have not fired, including post-frame completions.
    pub fn on_closed<R: EventOutcome>(
        &self,
        callback: impl FnOnce(&mut EventCx<'_>, SnackBarClosedReason) -> R + 'static,
    ) {
        let messenger = self.messenger.clone();
        let _prev = self.on_closed.borrow_mut().replace(Box::new(move |reason| {
            let Some(messenger) = messenger.upgrade() else {
                return;
            };
            let writer = messenger.writer.borrow().clone();
            if let Some(writer) = writer {
                writer.write(|cx| callback(cx, reason).report());
            }
        }));
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
    /// provisional guard [`hide_current`](MessengerCore::hide_current)/timeout
    /// use. See the module docs' "Completion slot" section.
    fn set_reason_once(&self, reason: SnackBarClosedReason) {
        if self.reason.get().is_none() {
            self.reason.set(Some(reason));
        }
    }

    /// Unconditionally overwrites the recorded reason —
    /// [`remove_current`](MessengerCore::remove_current)'s own use, which
    /// always wins over a not-yet-completed provisional reason. See the
    /// module docs' "Completion slot" section.
    fn set_reason(&self, reason: SnackBarClosedReason) {
        self.reason.set(Some(reason));
    }

    /// Fires `on_closed` with the recorded reason (falling back to
    /// [`SnackBarClosedReason::Remove`] — see the module docs' "Completion
    /// slot" section for why this fallback is not expected to be reachable).
    fn complete(&self) {
        let reason = self.reason.get().unwrap_or(SnackBarClosedReason::Remove);
        let taken = self.on_closed.borrow_mut().take();
        if let Some(callback) = taken {
            callback(reason);
        }
    }

    /// Drops `on_closed` WITHOUT invoking it — `clear_snack_bars`' silent
    /// drop of a still-queued entry (oracle parity: an abandoned
    /// `Completer`).
    fn complete_silently(&self) {
        let _prev = self.on_closed.borrow_mut().take();
    }
}

/// Where a [`MessengerCore::reconcile`] call originated — see the module
/// docs' "Deferring `on_closed` out of the build phase" section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReconcileOrigin {
    /// A public [`ScaffoldMessengerHandle`] method, called synchronously
    /// from an event-handler call stack — `on_closed` fires immediately.
    Direct,
    /// [`ScaffoldMessengerState::build`], reached from a purely tick-driven
    /// status settle — `on_closed` is deferred past this frame.
    Build,
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
    /// Acquired in [`ScaffoldMessengerHandle::attach`] (`init_state`, per
    /// ADR-0021). `None` until then, or if no binding installed
    /// one — see the module docs' "Deferring `on_closed` out of the build
    /// phase" section for cancellation when no lane is available.
    post_frame: RefCell<Option<LocalPostFrameHandle>>,
    writer: RefCell<Option<WriterSource>>,
    queue: RefCell<VecDeque<Rc<QueuedEntry>>>,
    last_entry_status: Cell<AnimationStatus>,
    last_duration_status: Cell<AnimationStatus>,
    /// Reentrancy guard for [`Self::pop_and_advance`] — see the module docs'
    /// "The drain state machine" section.
    advancing: Cell<bool>,
    scaffolds: RefCell<HashMap<ElementId, RebuildHandle>>,
}

type CompletionFailure = Option<Box<dyn std::any::Any + Send>>;

fn remember_completion_failure(first: &mut CompletionFailure, result: std::thread::Result<()>) {
    if let Err(payload) = result {
        if first.is_none() {
            *first = Some(payload);
        } else {
            flui_sdk::foundation::panic::retain_opaque_payload(payload);
        }
    }
}

impl MessengerCore {
    fn schedule_rebuild_on_scaffolds(&self) {
        self.deliver_scaffold_rebuilds(false);
    }

    fn deliver_scaffold_rebuilds(&self, retain_after_failure: bool) {
        // Host wake callbacks may synchronously register or dispose scaffolds.
        let handles: Vec<_> = self.scaffolds.borrow().values().cloned().collect();
        let mut first = None;
        for rebuild in &handles {
            remember_completion_failure(
                &mut first,
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    rebuild.schedule(flui_sdk::view::RebuildReason::StateChange);
                })),
            );
        }
        if retain_after_failure || first.is_some() {
            std::mem::forget(handles);
        }
        if let Some(payload) = first {
            std::panic::resume_unwind(payload);
        }
    }

    /// Polls both controllers against the last-observed status and
    /// translates every change into a state transition/pop, looping until a
    /// full pass detects no further change. See the module docs' "Why
    /// status edges are polled, not pushed, from the controller" section.
    fn reconcile(&self, origin: ReconcileOrigin) {
        loop {
            let mut changed = false;

            let entry_status = self.entry_controller.status();
            if entry_status != self.last_entry_status.get() {
                self.last_entry_status.set(entry_status);
                self.handle_entry_status(entry_status, origin);
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
    fn handle_entry_status(&self, status: AnimationStatus, origin: ReconcileOrigin) {
        match status {
            AnimationStatus::Dismissed => self.pop_and_advance(origin),
            AnimationStatus::Completed => self.start_display_timer(),
            // Forward/Reverse carry no further consequence here — the
            // controller's own `status()` already IS the observable state
            // (see the module docs' "The drain state machine" section).
            // `AnimationStatus` is `#[non_exhaustive]`; every variant it has
            // today is handled above.
            _ => {}
        }
    }

    /// Pops the just-exited front entry (if any), fires (or, for
    /// [`ReconcileOrigin::Build`], defers) its `on_closed`, then begins the
    /// next entry's entrance if the queue is still non-empty. The single
    /// place that ever pops the queue.
    fn pop_and_advance(&self, origin: ReconcileOrigin) {
        if self.advancing.get() {
            // A re-entrant call from inside the `on_closed` callback this
            // very function is about to invoke below — the outer call owns
            // finishing the advance; see the module docs.
            return;
        }
        self.advancing.set(true);
        struct AdvanceGuard<'a>(&'a Cell<bool>);
        impl Drop for AdvanceGuard<'_> {
            fn drop(&mut self) {
                self.0.set(false);
            }
        }
        let advancing_guard = AdvanceGuard(&self.advancing);

        self.cancel_display_timer();
        let popped = self.queue.borrow_mut().pop_front();
        let mut first = None;
        if let Some(entry) = &popped {
            // Pin the opaque entry outside invocation: the clone's unwind
            // cannot retire the last owner of its snack-bar value.
            remember_completion_failure(
                &mut first,
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    self.complete_entry(Rc::clone(entry), origin);
                })),
            );
        }

        let has_next = !self.queue.borrow().is_empty();
        if has_next {
            remember_completion_failure(
                &mut first,
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let _ = self.entry_controller.forward();
                })),
            );
        }
        let retain_after_failure = first.is_some();
        remember_completion_failure(
            &mut first,
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.deliver_scaffold_rebuilds(retain_after_failure);
            })),
        );
        if let Some(payload) = first {
            // The completed entry owns no remaining queue obligation, but its
            // opaque value may have competing panicking destructors.
            std::mem::forget(popped);
            drop(advancing_guard);
            std::panic::resume_unwind(payload);
        }
    }

    /// Fires `entry`'s `on_closed` per `origin` — immediately for
    /// [`ReconcileOrigin::Direct`], or deferred through [`Self::post_frame`]
    /// for [`ReconcileOrigin::Build`]. Without a live post-frame lane the
    /// callback is cancelled, never executed during build. See "Deferring
    /// `on_closed` out of the build phase" section.
    fn complete_entry(&self, entry: Rc<QueuedEntry>, origin: ReconcileOrigin) {
        if origin == ReconcileOrigin::Direct {
            entry.complete();
            return;
        }
        let Some(post_frame) = self.post_frame.borrow().clone() else {
            tracing::warn!("SnackBar on_closed cancelled: no owner-local post-frame lane");
            entry.complete_silently();
            return;
        };
        let cancelled_entry = Rc::clone(&entry);
        let scheduled = post_frame.schedule_local(move |_timing| entry.complete());
        if let Err(error) = scheduled {
            tracing::warn!(
                %error,
                "SnackBar on_closed cancelled: owner-local post-frame lane is closed"
            );
            cancelled_entry.complete_silently();
        }
    }

    fn start_display_timer(&self) {
        let Some(front) = self.queue.borrow().front().cloned() else {
            return;
        };
        let controller = AnimationController::new(
            front.snack_bar.configured_duration(),
            &UpdateScheduler::new(),
        );
        if let Some(vsync) = self.vsync.borrow().as_ref() {
            let registration = vsync.register(controller.clone());
            *self.duration_vsync_registration.borrow_mut() = Some(registration);
        }
        if let Some(rebuild) = self.rebuild.borrow().clone() {
            controller.add_status_listener(Arc::new(move |_status| {
                rebuild.schedule(flui_sdk::view::RebuildReason::AnimationTick);
            }));
        }
        self.last_duration_status.set(AnimationStatus::Dismissed);
        let _ = controller.forward();
        let _prev = self.duration_controller.borrow_mut().replace(controller);
    }

    fn cancel_display_timer(&self) {
        // Take both out before unregistering: dropping the controller there may
        // run its retired callbacks, which must not find these cells borrowed.
        let registration = self.duration_vsync_registration.borrow_mut().take();
        let vsync = self.vsync.borrow().clone();
        if let Some(registration) = registration
            && let Some(vsync) = vsync
        {
            vsync.unregister(&registration);
        }
        let taken = self.duration_controller.borrow_mut().take();
        if let Some(controller) = taken {
            controller.dispose();
        }
        self.last_duration_status.set(AnimationStatus::Dismissed);
    }

    /// Always takes the animated-reverse path: there is no
    /// `accessibleNavigation` immediate-complete branch (no
    /// `MediaQuery.accessibleNavigation` consumer exists yet in this
    /// substrate).
    fn hide_current(&self, reason: SnackBarClosedReason) {
        if self.entry_controller.status() == AnimationStatus::Dismissed {
            // Nothing showing to hide, including the empty-queue
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

    /// There is no `isDismissed` early return here, which is exactly why this
    /// needs the direct-pop fallback the module docs describe. Overwrites
    /// the reason unconditionally, not once-only — see the module docs'
    /// "Completion slot" section for why `remove` always wins over a
    /// not-yet-completed `hide`/timeout. Only ever reached from
    /// [`ScaffoldMessengerHandle::remove_current_snack_bar`], an
    /// event-handler call stack, so the direct-pop fallback fires
    /// synchronously ([`ReconcileOrigin::Direct`]).
    fn remove_current(&self, reason: SnackBarClosedReason) {
        let Some(front) = self.queue.borrow().front().cloned() else {
            return;
        };
        front.set_reason(reason);
        self.cancel_display_timer();
        if self.entry_controller.status() == AnimationStatus::Dismissed {
            self.pop_and_advance(ReconcileOrigin::Direct);
        } else {
            self.entry_controller.set_value(0.0);
        }
    }

    /// See the module docs' "`clearSnackBars`" section for the exact reason `hide_current` closes
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
            AnimationController::new(ENTRY_TRANSITION_DURATION, &UpdateScheduler::new());
        let shared = Rc::new(MessengerCore {
            entry_controller,
            duration_controller: RefCell::new(None),
            vsync: RefCell::new(None),
            entry_vsync_registration: RefCell::new(None),
            duration_vsync_registration: RefCell::new(None),
            rebuild: RefCell::new(None),
            post_frame: RefCell::new(None),
            writer: RefCell::new(None),
            queue: RefCell::new(VecDeque::new()),
            last_entry_status: Cell::new(AnimationStatus::Dismissed),
            last_duration_status: Cell::new(AnimationStatus::Dismissed),
            advancing: Cell::new(false),
            scaffolds: RefCell::new(HashMap::new()),
        });
        Self { shared }
    }

    /// Wires the ambient `Vsync`, installs the entrance controller's
    /// Send-safe "reschedule [`ScaffoldMessenger`]'s rebuild" listener, and
    /// acquires the binding's post-frame capability (ADR-0021) — see
    /// [`Self::new`]'s doc for why this is deferred out of construction, and
    /// the module docs' "Deferring `on_closed` out of the build phase"
    /// section for what the post-frame handle is for.
    pub(crate) fn attach(&self, ctx: &dyn LifecycleContext) {
        *self.shared.writer.borrow_mut() = Some(ctx.writer_source());
        let rebuild = ctx.rebuild_handle();
        let rebuild_for_listener = rebuild.clone();
        self.shared
            .entry_controller
            .add_status_listener(Arc::new(move |_status| {
                rebuild_for_listener.schedule(flui_sdk::view::RebuildReason::AnimationTick);
            }));
        let _prev = self.shared.rebuild.borrow_mut().replace(rebuild);
        *self.shared.post_frame.borrow_mut() = ctx.local_post_frame_handle();

        let vsync = ctx.get::<VsyncScope, _>(|scope| scope.vsync().clone());
        if let Some(vsync) = &vsync {
            let registration = vsync.register(self.shared.entry_controller.clone());
            *self.shared.entry_vsync_registration.borrow_mut() = Some(registration);
        }
        let _prev = std::mem::replace(&mut *self.shared.vsync.borrow_mut(), vsync);
    }

    /// Unregisters from `Vsync` and disposes both controllers.
    pub(crate) fn detach(&self) {
        self.shared.writer.borrow_mut().take();
        self.shared.post_frame.borrow_mut().take();
        let entries = std::mem::take(&mut *self.shared.queue.borrow_mut());
        for entry in entries {
            entry.complete_silently();
        }
        self.shared.cancel_display_timer();
        let registration = self.shared.entry_vsync_registration.borrow_mut().take();
        let vsync = self.shared.vsync.borrow_mut().take();
        if let Some(registration) = registration
            && let Some(vsync) = vsync
        {
            vsync.unregister(&registration);
        }
        self.shared.entry_controller.dispose();
    }

    /// Re-runs the state-machine reconciliation for a purely tick-driven
    /// settle — called from [`ScaffoldMessengerState::build`] whenever the
    /// Send-safe listeners scheduled a rebuild. Any `on_closed` this
    /// reaches is deferred, never fired inline mid-`build` — see the module
    /// docs' "Deferring `on_closed` out of the build phase" section.
    pub(crate) fn reconcile_after_tick_driven_settle(&self) {
        self.shared.reconcile(ReconcileOrigin::Build);
    }

    /// Registers a [`crate::Scaffold`] so it receives `schedule_rebuild`
    /// calls whenever the current entry's identity changes. Idempotent —
    /// keyed by [`ElementId`], a later call with the same id simply replaces
    /// the stored [`RebuildHandle`]. If a snack bar is already
    /// showing/queued, schedules an immediate rebuild so the newly
    /// registered scaffold picks it up.
    pub(crate) fn register_scaffold(&self, element_id: ElementId, rebuild: RebuildHandle) {
        if !self.shared.queue.borrow().is_empty() {
            rebuild.schedule(flui_sdk::view::RebuildReason::StateChange);
        }
        self.shared
            .scaffolds
            .borrow_mut()
            .insert(element_id, rebuild);
    }

    /// Unregisters a [`crate::Scaffold`] — called from its `dispose`.
    pub(crate) fn unregister_scaffold(&self, element_id: ElementId) {
        let _prev = self.shared.scaffolds.borrow_mut().remove(&element_id);
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
    /// Narrowed to the one
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
            let _ = self.shared.entry_controller.forward();
            self.shared.schedule_rebuild_on_scaffolds();
        }
        self.shared.reconcile(ReconcileOrigin::Direct);

        SnackBarController {
            on_closed,
            messenger: Rc::downgrade(&self.shared),
        }
    }

    /// Removes the current snack bar (if any) by running its normal exit
    /// animation, with [`SnackBarClosedReason::Hide`]. A no-op if nothing is
    /// currently showing.
    pub fn hide_current_snack_bar(&self) {
        self.shared.hide_current(SnackBarClosedReason::Hide);
        self.shared.reconcile(ReconcileOrigin::Direct);
    }

    /// Reason-carrying counterpart of [`Self::hide_current_snack_bar`], for
    /// callers inside this crate that close with a specific reason (e.g.
    /// [`crate::snack_bar::SnackBarAction`]'s single-fire press).
    pub(crate) fn hide_current_snack_bar_because(&self, reason: SnackBarClosedReason) {
        self.shared.hide_current(reason);
        self.shared.reconcile(ReconcileOrigin::Direct);
    }

    /// Removes the current snack bar (if any) immediately, with no exit
    /// animation, with [`SnackBarClosedReason::Remove`]. If any snack bars
    /// are queued, the next begins its entrance immediately.
    pub fn remove_current_snack_bar(&self) {
        self.shared.remove_current(SnackBarClosedReason::Remove);
        self.shared.reconcile(ReconcileOrigin::Direct);
    }

    /// Drops every queued (not-yet-shown) snack bar silently (their
    /// `on_closed` never fires) and hides the current one with its normal
    /// exit animation. See the module docs' "`clearSnackBars`" section.
    pub fn clear_snack_bars(&self) {
        self.shared.clear();
        self.shared.reconcile(ReconcileOrigin::Direct);
    }
}

/// Publishes a [`ScaffoldMessengerHandle`] to its subtree.
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
/// # Examples
///
/// ```rust
/// use flui_material::{Scaffold, ScaffoldMessenger};
/// use flui_sdk::widgets::Text;
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
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.handle.attach(ctx);
    }

    fn build(&self, view: &ScaffoldMessenger, _ctx: &dyn BuildContext) -> impl IntoView {
        // Absorbs any tick-driven status settle the Send-safe listeners
        // scheduled this rebuild for — see the module docs' "Deferring
        // `on_closed` out of the build phase" section for why any
        // `on_closed` this reaches is deferred, not fired inline here.
        self.handle.reconcile_after_tick_driven_settle();
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

    use flui_sdk::widgets::Text;

    use super::*;

    fn snack_bar(label: &str) -> SnackBar {
        SnackBar::new(Text::new(label.to_string()))
    }

    #[derive(Clone, StatelessView)]
    struct CaptureMessenger {
        captured: Rc<RefCell<Option<ScaffoldMessengerHandle>>>,
    }

    impl StatelessView for CaptureMessenger {
        fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
            *self.captured.borrow_mut() = ScaffoldMessengerScope::maybe_of(ctx);
            flui_sdk::widgets::SizedBox::shrink()
        }
    }

    fn mounted_handle() -> (
        flui_testing::widgets::harness::Harness,
        ScaffoldMessengerHandle,
    ) {
        let captured = Rc::new(RefCell::new(None));
        let harness =
            flui_testing::widgets::harness::mount(ScaffoldMessenger::new(CaptureMessenger {
                captured: Rc::clone(&captured),
            }));
        let handle = captured.borrow().clone().expect("messenger mounted");
        (harness, handle)
    }

    mod failure_cases;

    #[test]
    fn a_panicking_completion_does_not_lock_future_queue_operations() {
        failure_cases::run();
    }
}
