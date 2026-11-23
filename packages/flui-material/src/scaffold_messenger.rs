//! [`ScaffoldMessenger`] — queues [`crate::SnackBar`]s and shows the current
//! one in the root [`crate::Scaffold`] of each nested set below it — plus
//! [`SnackBarController`] (returned by
//! [`ScaffoldMessengerHandle::show_snack_bar`]) and [`SnackBarClosedReason`].
//!
//! The messenger holds snack bars only; it has no `MaterialBanner` queue.
//!
//! ## The entrance controller and the display timer
//!
//! One [`AnimationController`] (`ENTRY_TRANSITION_DURATION`) runs every
//! entry's entrance and exit. Its [`AnimationStatus`] is the drain's state:
//! `ReverseSettled` is idle or exited, `Forward` entering, `ForwardSettled`
//! displayed, `Reverse` exiting.
//!
//! Once an entry is displayed, a task spawned on the UI runtime's
//! [`AsyncDriver`] sleeps for its [`crate::SnackBar::duration`] on the
//! runtime's clock ([`TimerHandle`]; the testing kit's virtual clock in a
//! test) and then hides the entry with [`SnackBarClosedReason::Timeout`]
//! (`flutter/flutter@3.50.1 packages/flutter/lib/src/material/scaffold.dart:619`).
//! A resting snack bar therefore asks for no frame: nothing ticks between its
//! entrance and its timeout. The task's [`TaskToken`] is the timer: hide,
//! remove, clear, the pop of its entry and dispose drop it, which cancels the
//! task. The timer names the entry it counts for, so it never closes another
//! one, and an entry gets one timer however often its entrance is seen to
//! settle. Without a UI runtime (a bare element tree) there is no clock, and
//! the snack bar stays until it is hidden or removed.
//!
//! Hovering a presenter pauses the timer of the entry it shows: the running
//! task is cancelled and the time left is kept, and the last hover to end
//! starts a new task for that remainder. A presenter hovered when its entry's
//! entrance settles starts the timer paused. Flutter has no hover pause.
//!
//! `SnackBar.persist` and the pause while a covering `ModalRoute` is current
//! (`flutter/flutter@3.50.1 packages/flutter/lib/src/material/scaffold.dart:616`,
//! `flutter/flutter@3.50.1 packages/flutter/lib/src/material/scaffold.dart:622`) are not
//! built: the timer always runs, whether or not the snack bar carries an
//! action. Neither is `accessibleNavigation`: a hide always runs the exit
//! animation (`flutter/flutter@3.50.1 packages/flutter/lib/src/material/scaffold.dart:446`).
//!
//! ## The drain
//!
//! `MessengerCore::reconcile` reads the entrance controller's status and the
//! queue, and acts until there is nothing left to do:
//!
//! - `ReverseSettled` with a non-empty queue: the front entry has exited.
//!   `MessengerCore::pop_and_advance` pops it, completes it, starts the next
//!   entry's entrance if there is one and wakes the scaffolds. It is the one
//!   place that pops the queue.
//! - `ForwardSettled` with no timer for the front entry: start its timer.
//!
//! **Queue-wedge invariant**: *the queue is non-empty ⇒ the entrance
//! controller is not `ReverseSettled`, except inside `pop_and_advance` between
//! the pop and the next entry's `forward()`.* So `ReverseSettled` with a
//! non-empty queue always means the front entry is done, and the reconcile
//! reads it from the status alone. That is what lets `remove_current_snack_bar`
//! on an entry that is already `ReverseSettled` complete it, where
//! `set_value(0.0)` would change no status (Flutter's `removeCurrentSnackBar`
//! checks only that the queue is non-empty,
//! `flutter/flutter@3.50.1 packages/flutter/lib/src/material/scaffold.dart:424`). An entry
//! whose entrance the controller refuses (a messenger already disposed) is
//! completed at once instead of wedging the queue behind it.
//!
//! The reconcile runs from two places. **Directly**, at the end of every
//! public [`ScaffoldMessengerHandle`] method, so a call observes its own
//! effect at once. **From a tick**: the entrance controller's status listener
//! and the display timer's task, for a change no call made. Never from
//! `build`, where the controller refuses mutation (ADR-0074 §5.2).
//!
//! `MessengerCore::drain` is the one record of what runs on the call stack:
//!
//! - `Drain::Idle`: a status listener reconciles the change it reports.
//! - `Drain::Reconciling`: a queue operation or a reconcile runs. A status
//!   listener that its own `forward()`/`reverse()`/`set_value()` fires
//!   returns at once (they change status synchronously), and the running
//!   reconcile reads the new status.
//! - `Drain::Advancing`: `pop_and_advance` runs, its `on_closed` included. A
//!   queue operation that `on_closed` calls (`show_snack_bar`,
//!   `remove_current_snack_bar`, …) runs and reconciles, but a nested
//!   advance returns: the outer one finishes starting the next entry. A
//!   remove that `on_closed` asks of the entry about to enter records its
//!   reason and nothing more, and that entry shows and closes with it.
//!
//! A step entered inside another keeps the further phase (the variants are
//! ordered), and its guard restores the outer phase on return and on unwind.
//! No `RefCell` borrow of the queue is held across a call into the
//! controller, a wake or `on_closed`.
//!
//! ## Deferring `on_closed` out of the frame
//!
//! `ReconcileOrigin` says what a completion may do:
//!
//! - **`ReconcileOrigin::Direct`**: a public [`ScaffoldMessengerHandle`]
//!   method, called from an event handler's stack. `on_closed` fires at once,
//!   in the same call.
//! - **`ReconcileOrigin::Tick`**: a status listener or the display timer,
//!   inside a frame. An `on_closed` that calls `show_snack_bar` there would
//!   change the queue in the middle of the frame's animation pass, so
//!   `MessengerCore::complete_entry` defers it through the
//!   [`PostFrameHandle`] acquired in `create_state` (ADR-0021): it runs after
//!   the frame has committed, where its own queue operations are an ordinary
//!   event handler's. Without a live post-frame lane the completion is
//!   cancelled with a diagnostic; it never runs user code inside the frame.
//!   Disposing the messenger cancels callbacks that have not fired, scheduled
//!   ones included: their writer is gone.
//!
//! ## Panics
//!
//! Each step that runs code the messenger does not own is contained with
//! [`FirstPanic`]: the popped entry's display-timer cancellation, its
//! completion, the next entrance and each scaffold's wake
//! run even when an earlier one panics, the first panic is resumed after the
//! drain's guard is released, a later payload is discarded, and the values
//! they leave behind (the popped entry, the wake handles) are destroyed one
//! at a time under the same containment (ADR-0281 §3).
//!
//! ## The root scaffold of a nested set
//!
//! The current entry shows in every [`crate::Scaffold`] registered with the
//! messenger (`flutter/flutter@3.50.1 packages/flutter/lib/src/material/scaffold.dart:231`). A
//! scaffold registers only when no ancestor scaffold belongs to the same
//! messenger, so of a nested set only the root shows the snack bar
//! (`_isRoot`, `flutter/flutter@3.50.1 packages/flutter/lib/src/material/scaffold.dart:242`); a
//! scaffold under a messenger of its own below an outer scaffold is the root
//! of its own set. [`crate::Scaffold`] reads the private
//! `ScaffoldMessengerHandle::current_entry` in its own `build`.
//!
//! ## `clear_snack_bars`
//!
//! Drops every queued, not yet shown entry's `on_closed` **silently**: no
//! callback fires, as Flutter drops their `Completer`s unfulfilled
//! (`flutter/flutter@3.50.1 packages/flutter/lib/src/material/scaffold.dart:463`). The current
//! entry is then hidden with [`SnackBarClosedReason::Hide`]: `clearSnackBars`
//! calls `hideCurrentSnackBar()`, whose reason defaults to `hide`.
//!
//! ## Completion reason
//!
//! Each queued entry carries a `reason` cell its pop reads (falling back to
//! [`SnackBarClosedReason::Remove`] for an entry popped with none recorded:
//! one whose entrance was refused). It is set two ways:
//!
//! - `QueuedEntry::set_reason_once` (hide, timeout) is **provisional**: it
//!   applies only if nothing is recorded yet. A hide completes when the exit
//!   animation settles, so its reason is a promise about that later
//!   completion.
//! - `QueuedEntry::set_reason` (remove) **overwrites**. A remove completes at
//!   once with its own reason, so a `remove_current_snack_bar()` arriving
//!   during the exit of an earlier `hide_current_snack_bar()` reports
//!   `Remove`.
//!
//! ## Reachable reasons
//!
//! [`SnackBarClosedReason::Timeout`], [`SnackBarClosedReason::Hide`],
//! [`SnackBarClosedReason::Remove`] and [`SnackBarClosedReason::Action`].
//! [`SnackBarClosedReason::Dismiss`] and [`SnackBarClosedReason::Swipe`] name
//! a close icon and swipe-to-dismiss, which this crate does not build.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::{Rc, Weak};

use std::time::Duration;

use flui_sdk::animation::{Animation, AnimationController, AnimationStatus, DrivenController};
use flui_sdk::foundation::panic::FirstPanic;
use flui_sdk::foundation::{ElementId, Subscription};
use flui_sdk::scheduler::{AsyncDriver, Instant, PostFrameHandle, TaskToken, TimerHandle};
use flui_sdk::view::environment::VsyncScope;
use flui_sdk::view::prelude::*;
use flui_sdk::view::{RebuildHandle, impl_inherited_view};

use crate::snack_bar::SnackBar;

/// The shared entrance/exit controller's duration.
const ENTRY_TRANSITION_DURATION: Duration = Duration::from_millis(250);

/// Specifies how a [`SnackBar`] was closed.
///
/// See the module docs' "Reachable reasons" section for which variants this
/// crate produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SnackBarClosedReason {
    /// Closed after the user pressed the [`crate::snack_bar::SnackBarAction`].
    Action,
    /// Closed through an accessibility dismiss action. Not produced yet: no
    /// close icon is built.
    Dismiss,
    /// Closed by a user swipe. Not produced yet: no swipe-to-dismiss is
    /// built.
    Swipe,
    /// Closed by [`ScaffoldMessengerHandle::hide_current_snack_bar`] — also
    /// what [`ScaffoldMessengerHandle::clear_snack_bars`] hides the
    /// surviving current entry with.
    Hide,
    /// Closed by [`ScaffoldMessengerHandle::remove_current_snack_bar`] —
    /// abrupt, no exit animation.
    Remove,
    /// Closed because its display duration passed.
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
/// The completion is a synchronous one-shot callback, run through the
/// messenger's writer so it receives an [`EventContext`].
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
        callback: impl FnOnce(&mut EventContext<'_>, SnackBarClosedReason) -> R + 'static,
    ) {
        let messenger = self.messenger.clone();
        let _prev = self.on_closed.borrow_mut().replace(Box::new(move |reason| {
            let Some(messenger) = messenger.upgrade() else {
                return;
            };
            let writer = messenger.writer.borrow().clone();
            if let Some(writer) = writer {
                writer.write(|ctx| callback(ctx, reason).report());
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
/// slot. See the module docs' "Completion reason" section.
struct QueuedEntry {
    snack_bar: SnackBar,
    hovered_presenters: Cell<usize>,
    reason: Cell<Option<SnackBarClosedReason>>,
    on_closed: ClosedCallbackSlot,
}

/// Exact queued entry addressed by a mounted presenter; neither weak handle
/// keeps a messenger or a retired entry alive.
#[derive(Clone)]
pub(crate) struct SnackBarHoverTarget {
    messenger: Weak<MessengerCore>,
    entry: Weak<QueuedEntry>,
}

impl SnackBarHoverTarget {
    pub(crate) fn is_same(&self, other: &Self) -> bool {
        Weak::ptr_eq(&self.messenger, &other.messenger) && Weak::ptr_eq(&self.entry, &other.entry)
    }

    /// Counts one presenter's hover in or out; the first hover pauses the
    /// entry's display timer and the last one to end resumes it.
    pub(crate) fn set_hovered(&self, hovered: bool) {
        let Some(entry) = self.entry.upgrade() else {
            return;
        };
        let count = entry.hovered_presenters.get();
        let next = if hovered {
            count
                .checked_add(1)
                .expect("BUG: mounted SnackBar hover count exhausted")
        } else {
            count
                .checked_sub(1)
                .expect("BUG: a hover lease releases its own admission")
        };
        entry.hovered_presenters.set(next);
        if (count == 0) == (next == 0) {
            return;
        }
        let Some(messenger) = self.messenger.upgrade() else {
            return;
        };
        if next == 0 {
            messenger.resume_display_timer(&entry);
        } else {
            messenger.pause_display_timer(&entry);
        }
    }
}

impl QueuedEntry {
    /// Records `reason` only if nothing has been recorded yet — the
    /// provisional guard [`hide_current`](MessengerCore::hide_current)/timeout
    /// use. See the module docs' "Completion reason" section.
    fn set_reason_once(&self, reason: SnackBarClosedReason) {
        if self.reason.get().is_none() {
            self.reason.set(Some(reason));
        }
    }

    /// Unconditionally overwrites the recorded reason —
    /// [`remove_current`](MessengerCore::remove_current)'s own use, which
    /// always wins over a not-yet-completed provisional reason. See the
    /// module docs' "Completion reason" section.
    fn set_reason(&self, reason: SnackBarClosedReason) {
        self.reason.set(Some(reason));
    }

    /// Fires `on_closed` with the recorded reason, or
    /// [`SnackBarClosedReason::Remove`] when none was recorded.
    fn complete(&self) {
        let reason = self.reason.get().unwrap_or(SnackBarClosedReason::Remove);
        let taken = self.on_closed.borrow_mut().take();
        if let Some(callback) = taken {
            callback(reason);
        }
    }

    /// Drops `on_closed` WITHOUT invoking it — `clear_snack_bars`' silent
    /// drop of a still-queued entry, as Flutter abandons its `Completer`.
    fn complete_silently(&self) {
        let _prev = self.on_closed.borrow_mut().take();
    }
}

/// Where a [`MessengerCore::reconcile`] call originated — see the module
/// docs' "Deferring `on_closed` out of the frame" section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReconcileOrigin {
    /// A public [`ScaffoldMessengerHandle`] method, called synchronously
    /// from an event-handler call stack — `on_closed` fires immediately.
    Direct,
    /// A controller status listener or the display timer, inside a frame —
    /// `on_closed` is deferred past this frame.
    Tick,
}

/// What the drain runs on this call stack — see the module docs' "The drain"
/// section. Ordered: a step entered inside another keeps the further phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Drain {
    /// Nothing runs: a status listener reconciles the change it reports.
    Idle,
    /// A queue operation or a reconcile runs and reads every status change
    /// its own commands make.
    Reconciling,
    /// [`MessengerCore::pop_and_advance`] runs, its `on_closed` included: a
    /// nested advance returns and leaves the next entrance to it.
    Advancing,
}

/// Restores [`MessengerCore::drain`] to the phase it held before a step, on
/// return and on unwind.
struct DrainStep<'a> {
    drain: &'a Cell<Drain>,
    outer: Drain,
}

impl Drop for DrainStep<'_> {
    fn drop(&mut self) {
        self.drain.set(self.outer);
    }
}

/// The UI runtime's task driver and clock, which the display timer runs on.
struct DisplayClock {
    driver: AsyncDriver,
    timer: TimerHandle,
}

/// The display timer of one entry — see the module docs' "The entrance
/// controller and the display timer" section.
struct DisplayTimer {
    /// The entry it counts for: it never closes another.
    entry: Weak<QueuedEntry>,
    countdown: Countdown,
}

enum Countdown {
    /// A task sleeping until `deadline` (`None`: past what the clock can
    /// represent), which then hides the entry. Held for its drop, which
    /// cancels the task.
    Running {
        deadline: Option<Instant>,
        _task: TaskToken,
    },
    /// Hovered, with `remaining` left when the hover ends.
    Paused { remaining: Duration },
    /// No UI runtime: no clock to count on.
    Unclocked,
}

impl DisplayTimer {
    fn counts_for(&self, entry: &Rc<QueuedEntry>) -> bool {
        std::ptr::eq(self.entry.as_ptr(), Rc::as_ptr(entry))
    }
}

/// The messenger's interior-mutable state, `Rc`-shared between
/// [`ScaffoldMessengerHandle`] clones.
struct MessengerCore {
    entry_controller: AnimationController,
    entry_owner: RefCell<Option<DrivenController>>,
    /// Set by `ScaffoldMessengerHandle::detach`: the entrance owner is not
    /// put back after a rebind.
    detached: Cell<bool>,
    /// This core, for the display timer's task.
    this: Weak<MessengerCore>,
    drain: Cell<Drain>,
    /// Acquired in `create_state` (ADR-0021); `None` when no binding
    /// installed one — see the module docs' "Deferring `on_closed` out of the
    /// frame" section.
    post_frame: Option<PostFrameHandle>,
    /// Acquired in `create_state`; `None` on a tree bound to no UI runtime.
    clock: Option<DisplayClock>,
    display: RefCell<Option<DisplayTimer>>,
    writer: RefCell<Option<WriterSource>>,
    queue: RefCell<VecDeque<Rc<QueuedEntry>>>,
    /// The registered scaffolds, one entry per element. A messenger serves
    /// a handful, so they are a list.
    scaffolds: RefCell<Vec<(ElementId, RebuildHandle)>>,
}

impl MessengerCore {
    /// Wakes every registered scaffold. Each wake and each handle's drop runs
    /// contained; the first panic is resumed after all of them ran.
    fn deliver_scaffold_rebuilds(&self) {
        // Host wake callbacks may synchronously register or dispose scaffolds.
        let handles: Vec<RebuildHandle> = self
            .scaffolds
            .borrow()
            .iter()
            .map(|(_, rebuild)| rebuild.clone())
            .collect();
        let mut failure = FirstPanic::new();
        for rebuild in &handles {
            failure.contain(|| rebuild.schedule(flui_sdk::view::RebuildReason::StateChange));
        }
        // Each handle is destroyed on its own under containment: dropped as
        // one Vec, a second panicking destructor would run during the first's
        // unwind and abort (ADR-0281 §3).
        for handle in handles {
            failure.retire(handle);
        }
        failure.resume();
    }

    /// Enters `step` for as long as the returned guard lives.
    fn enter(&self, step: Drain) -> DrainStep<'_> {
        let outer = self.drain.get();
        self.drain.set(outer.max(step));
        DrainStep {
            drain: &self.drain,
            outer,
        }
    }

    /// Reads the entrance status and the queue and acts until nothing is
    /// left to do. See the module docs' "The drain" section.
    fn reconcile(&self, origin: ReconcileOrigin) {
        let _step = self.enter(Drain::Reconciling);
        self.settle(origin);
    }

    /// Run a public operation and the reconcile that ends it as one
    /// `ReconcileOrigin::Direct` step: a status listener its controller
    /// commands fire leaves the change to that reconcile, so `on_closed`
    /// fires synchronously in the caller's stack.
    fn direct(&self, operation: impl FnOnce(&Self)) {
        let _step = self.enter(Drain::Reconciling);
        operation(self);
        self.settle(ReconcileOrigin::Direct);
    }

    /// The entrance controller's status listener: reconcile a tick-driven
    /// settle now, unless a step of this messenger runs on this stack (its
    /// own command fired the listener, and that step reads the change).
    fn reconcile_from_tick(core: &Weak<Self>) {
        let Some(core) = core.upgrade() else {
            return;
        };
        if core.drain.get() == Drain::Idle {
            core.reconcile(ReconcileOrigin::Tick);
        }
    }

    fn settle(&self, origin: ReconcileOrigin) {
        loop {
            let acted = match self.entry_controller.status() {
                AnimationStatus::ReverseSettled => {
                    !self.queue.borrow().is_empty() && self.pop_and_advance(origin)
                }
                AnimationStatus::ForwardSettled => self.start_display_timer(),
                AnimationStatus::Forward | AnimationStatus::Reverse => false,
            };
            if !acted {
                break;
            }
        }
    }

    /// Pops the exited front entry, fires (or, for [`ReconcileOrigin::Tick`],
    /// defers) its `on_closed`, then begins the next entry's entrance if the
    /// queue is still non-empty. The single place that ever pops the queue.
    /// Returns `false`, doing nothing, inside another advance.
    fn pop_and_advance(&self, origin: ReconcileOrigin) -> bool {
        if self.drain.get() == Drain::Advancing {
            // Called from the `on_closed` the outer advance runs: the outer
            // call finishes the advance.
            return false;
        }
        let advancing = self.enter(Drain::Advancing);

        let mut failure = FirstPanic::new();
        failure.retire(self.display.take());
        let popped = self.queue.borrow_mut().pop_front();
        if let Some(entry) = &popped {
            // The queue's reference is held here, outside the completion, so
            // the completion's unwind cannot retire the last owner of its
            // snack-bar value.
            failure.contain(|| self.complete_entry(Rc::clone(entry), origin));
        }
        let has_next = !self.queue.borrow().is_empty();
        if has_next {
            failure.contain(|| {
                let _ = self.entry_controller.forward();
            });
        }
        failure.contain(|| self.deliver_scaffold_rebuilds());
        // The completed entry owns no remaining queue obligation; after a
        // caught failure its destructor cannot replace the first one.
        failure.retire(popped);
        drop(advancing);
        failure.resume();
        true
    }

    /// Fires `entry`'s `on_closed` per `origin` — immediately for
    /// [`ReconcileOrigin::Direct`], or deferred through [`Self::post_frame`]
    /// for [`ReconcileOrigin::Tick`]. Without a live post-frame lane the
    /// callback is cancelled, never executed inside the frame. See "Deferring
    /// `on_closed` out of the frame" section.
    fn complete_entry(&self, entry: Rc<QueuedEntry>, origin: ReconcileOrigin) {
        if origin == ReconcileOrigin::Direct {
            entry.complete();
            return;
        }
        let Some(post_frame) = self.post_frame.clone() else {
            tracing::warn!("SnackBar on_closed cancelled: no owner-local post-frame lane");
            entry.complete_silently();
            return;
        };
        let cancelled_entry = Rc::clone(&entry);
        let scheduled = post_frame.schedule(move |_timing| entry.complete());
        if let Err(error) = scheduled {
            tracing::warn!(
                %error,
                "SnackBar on_closed cancelled: owner-local post-frame lane is closed"
            );
            cancelled_entry.complete_silently();
        }
    }

    /// Starts the front entry's display timer, paused while a presenter
    /// hovers it. Returns `false`, doing nothing, when there is no front
    /// entry or it already has its timer.
    fn start_display_timer(&self) -> bool {
        let Some(front) = self.queue.borrow().front().cloned() else {
            return false;
        };
        let started = self
            .display
            .borrow()
            .as_ref()
            .is_some_and(|display| display.counts_for(&front));
        if started {
            return false;
        }
        let duration = front.snack_bar.configured_duration();
        let countdown = if front.hovered_presenters.get() == 0 {
            self.count_down(&front, duration)
        } else {
            Countdown::Paused {
                remaining: duration,
            }
        };
        self.install_display_timer(&front, countdown);
        true
    }

    /// Spawns the task that hides `entry` once `remaining` has passed on the
    /// UI runtime's clock.
    fn count_down(&self, entry: &Rc<QueuedEntry>, remaining: Duration) -> Countdown {
        let Some(clock) = &self.clock else {
            return Countdown::Unclocked;
        };
        let deadline = clock.timer.now().checked_add(remaining);
        let sleep = match deadline {
            Some(deadline) => clock.timer.sleep_until(deadline),
            None => clock.timer.sleep(remaining),
        };
        let core = self.this.clone();
        let counted = Rc::downgrade(entry);
        let task = clock.driver.spawn_local(Box::pin(async move {
            sleep.await;
            MessengerCore::display_elapsed(&core, &counted);
        }));
        if task.is_cancelled() {
            tracing::warn!("SnackBar display timer refused: its UI runtime is gone");
        }
        Countdown::Running {
            deadline,
            _task: task,
        }
    }

    /// Replaces the display timer with `entry`'s. The outgoing timer is
    /// dropped, its task cancelled, with the slot released.
    fn install_display_timer(&self, entry: &Rc<QueuedEntry>, countdown: Countdown) {
        let outgoing = self.display.replace(Some(DisplayTimer {
            entry: Rc::downgrade(entry),
            countdown,
        }));
        drop(outgoing);
    }

    fn cancel_display_timer(&self) {
        let outgoing = self.display.take();
        drop(outgoing);
    }

    /// The display timer's task woke: if it is still `entry`'s running timer,
    /// hide `entry` with [`SnackBarClosedReason::Timeout`].
    fn display_elapsed(core: &Weak<Self>, entry: &Weak<QueuedEntry>) {
        let (Some(core), Some(entry)) = (core.upgrade(), entry.upgrade()) else {
            return;
        };
        let fired = {
            let mut display = core.display.borrow_mut();
            let running = display.as_ref().is_some_and(|display| {
                display.counts_for(&entry) && matches!(display.countdown, Countdown::Running { .. })
            });
            if running { display.take() } else { None }
        };
        let Some(fired) = fired else {
            return;
        };
        // The task is being polled: dropping its own token removes its slot,
        // and the pump does not queue it again.
        drop(fired);
        let _step = core.enter(Drain::Reconciling);
        let front = core
            .queue
            .borrow()
            .front()
            .is_some_and(|front| Rc::ptr_eq(front, &entry));
        if front {
            core.hide_current(SnackBarClosedReason::Timeout);
        }
        core.settle(ReconcileOrigin::Tick);
    }

    /// Pauses `entry`'s running display timer, keeping the time it has left.
    fn pause_display_timer(&self, entry: &Rc<QueuedEntry>) {
        // The clock is read before the slot is borrowed.
        let now = self.clock.as_ref().map(|clock| clock.timer.now());
        let paused = {
            let mut display = self.display.borrow_mut();
            let Some(timer) = display.as_mut().filter(|timer| timer.counts_for(entry)) else {
                return;
            };
            let Countdown::Running { deadline, .. } = timer.countdown else {
                return;
            };
            let remaining = match (deadline, now) {
                (Some(deadline), Some(now)) => deadline.saturating_duration_since(now),
                _ => Duration::MAX,
            };
            std::mem::replace(&mut timer.countdown, Countdown::Paused { remaining })
        };
        // The running task is cancelled with the slot released.
        drop(paused);
    }

    /// Resumes `entry`'s paused display timer with the time it had left.
    fn resume_display_timer(&self, entry: &Rc<QueuedEntry>) {
        let remaining = match self.display.borrow().as_ref() {
            Some(
                timer @ DisplayTimer {
                    countdown: Countdown::Paused { remaining },
                    ..
                },
            ) if timer.counts_for(entry) => *remaining,
            _ => return,
        };
        let countdown = self.count_down(entry, remaining);
        self.install_display_timer(entry, countdown);
    }

    /// Runs the exit animation of the current entry, if one is showing.
    /// Always the animated reverse: `accessibleNavigation` is not read.
    fn hide_current(&self, reason: SnackBarClosedReason) {
        if self.entry_controller.status() == AnimationStatus::ReverseSettled {
            // Nothing showing to hide, including the empty-queue
            // case (an empty queue always leaves the controller ReverseSettled
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

    /// Removes the current entry with no exit animation. Overwrites the
    /// reason, unlike a hide — see the module docs' "Completion reason"
    /// section. On an entry already `ReverseSettled` it changes no status:
    /// the reconcile that ends the operation completes it (the module docs'
    /// "Queue-wedge invariant").
    fn remove_current(&self, reason: SnackBarClosedReason) {
        let Some(front) = self.queue.borrow().front().cloned() else {
            return;
        };
        front.set_reason(reason);
        self.cancel_display_timer();
        if self.entry_controller.status() != AnimationStatus::ReverseSettled {
            let _ = self.entry_controller.set_value(0.0);
        }
    }

    /// See the module docs' "`clear_snack_bars`" section for the reason
    /// `hide_current` closes the surviving current entry with.
    fn clear(&self) {
        if self.queue.borrow().is_empty()
            || self.entry_controller.status() == AnimationStatus::ReverseSettled
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

/// Two handles are equal when they name the same messenger.
impl PartialEq for ScaffoldMessengerHandle {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.shared, &other.shared)
    }
}

impl Eq for ScaffoldMessengerHandle {}

impl std::fmt::Debug for ScaffoldMessengerHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScaffoldMessengerHandle")
            .finish_non_exhaustive()
    }
}

impl ScaffoldMessengerHandle {
    /// A handle with an empty queue, born bound to `ctx`'s writer, post-frame
    /// lane and task driver. The entrance controller has no `Vsync` until
    /// [`Self::rebind`]: the ambient `VsyncScope` is read in `init_state` and
    /// `did_change_dependencies`. [`Self::detach`] takes the writer, so a
    /// handle that outlives its messenger writes nothing.
    fn new(ctx: &dyn LifecycleContext) -> Self {
        let entry_controller = AnimationController::builder(ENTRY_TRANSITION_DURATION)
            .debug_label("SnackBar.entry")
            .build_on(None);
        let clock = ctx.async_driver().map(|driver| DisplayClock {
            timer: driver.timer(),
            driver,
        });
        let shared = Rc::new_cyclic(|this| MessengerCore {
            this: this.clone(),
            entry_controller: entry_controller.controller().clone(),
            entry_owner: RefCell::new(Some(entry_controller)),
            detached: Cell::new(false),
            drain: Cell::new(Drain::Idle),
            post_frame: ctx.post_frame_handle(),
            clock,
            display: RefCell::new(None),
            writer: RefCell::new(Some(ctx.writer_source())),
            queue: RefCell::new(VecDeque::new()),
            scaffolds: RefCell::new(Vec::new()),
        });
        Self { shared }
    }

    /// Subscribes the reconcile to the entrance controller's status, for a
    /// change no call made (the entrance or the exit settling).
    fn listen_entry_status(&self) -> Subscription {
        let core = Rc::downgrade(&self.shared);
        self.shared
            .entry_controller
            .listen_status(Rc::new(move |_status| {
                MessengerCore::reconcile_from_tick(&core);
            }))
    }

    /// Binds the entrance controller to the ambient `Vsync`.
    fn rebind(&self, ctx: &dyn LifecycleContext) {
        let vsync = VsyncScope::maybe_of(ctx);
        let Some(mut owner) = self.shared.entry_owner.borrow_mut().take() else {
            return;
        };
        let mut failure = FirstPanic::new();
        failure.contain(|| {
            if let Err(error) = owner.rebind(vsync.as_ref()) {
                tracing::error!(%error, "ScaffoldMessenger lost its frame registry");
            }
        });
        let outgoing = {
            let mut slot = self.shared.entry_owner.borrow_mut();
            if !self.shared.detached.get() && slot.is_none() {
                slot.replace(owner)
            } else {
                Some(owner)
            }
        };
        failure.retire(outgoing);
        failure.resume();
    }

    /// Drops every queued entry silently, cancels the display timer and
    /// retires the entrance controller.
    fn detach(&self) {
        self.shared.detached.set(true);
        self.shared.writer.borrow_mut().take();
        let entries = std::mem::take(&mut *self.shared.queue.borrow_mut());
        for entry in entries {
            entry.complete_silently();
        }
        self.shared.cancel_display_timer();
        let outgoing = self.shared.entry_owner.borrow_mut().take();
        drop(outgoing);
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
        let replaced = {
            let mut scaffolds = self.shared.scaffolds.borrow_mut();
            if let Some((_, held)) = scaffolds.iter_mut().find(|(id, _)| *id == element_id) {
                Some(std::mem::replace(held, rebuild))
            } else {
                scaffolds.push((element_id, rebuild));
                None
            }
        };
        // The replaced handle's destructor runs with the table released.
        drop(replaced);
    }

    /// Unregisters a [`crate::Scaffold`] — called from its `dispose`.
    pub(crate) fn unregister_scaffold(&self, element_id: ElementId) {
        let removed = {
            let mut scaffolds = self.shared.scaffolds.borrow_mut();
            scaffolds
                .iter()
                .position(|(id, _)| *id == element_id)
                .map(|at| scaffolds.swap_remove(at))
        };
        drop(removed);
    }

    /// The entry every registered [`crate::Scaffold`] should currently
    /// display, if any: the config plus a clone of the shared entrance/exit
    /// controller driving its height animation.
    #[must_use]
    pub(crate) fn current_entry(
        &self,
    ) -> Option<(SnackBar, AnimationController, SnackBarHoverTarget)> {
        self.shared.queue.borrow().front().map(|entry| {
            (
                entry.snack_bar.clone(),
                self.shared.entry_controller.clone(),
                SnackBarHoverTarget {
                    messenger: Rc::downgrade(&self.shared),
                    entry: Rc::downgrade(entry),
                },
            )
        })
    }

    /// Shows `snack_bar` across every registered [`crate::Scaffold`]. If one
    /// is already showing, `snack_bar` queues behind it (FIFO) and is shown
    /// once every earlier entry has closed.
    ///
    /// One snack bar per call; the transition duration is fixed at
    /// `ENTRY_TRANSITION_DURATION` (no `snackBarAnimationStyle` override).
    pub fn show_snack_bar(&self, snack_bar: SnackBar) -> SnackBarController {
        let on_closed = Rc::new(RefCell::new(None));
        let entry = Rc::new(QueuedEntry {
            snack_bar,
            hovered_presenters: Cell::new(0),
            reason: Cell::new(None),
            on_closed: Rc::clone(&on_closed),
        });

        self.shared.direct(|core| {
            let should_enter = {
                let mut queue = core.queue.borrow_mut();
                queue.push_back(entry);
                queue.len() == 1
            };
            if should_enter {
                let _ = core.entry_controller.forward();
                core.deliver_scaffold_rebuilds();
            }
        });

        SnackBarController {
            on_closed,
            messenger: Rc::downgrade(&self.shared),
        }
    }

    /// Removes the current snack bar (if any) by running its normal exit
    /// animation, with [`SnackBarClosedReason::Hide`]. A no-op if nothing is
    /// currently showing.
    pub fn hide_current_snack_bar(&self) {
        self.shared
            .direct(|core| core.hide_current(SnackBarClosedReason::Hide));
    }

    /// Reason-carrying counterpart of [`Self::hide_current_snack_bar`], for
    /// callers inside this crate that close with a specific reason (e.g.
    /// [`crate::snack_bar::SnackBarAction`]'s single-fire press).
    pub(crate) fn hide_current_snack_bar_because(&self, reason: SnackBarClosedReason) {
        self.shared.direct(|core| core.hide_current(reason));
    }

    /// Removes the current snack bar (if any) immediately, with no exit
    /// animation, with [`SnackBarClosedReason::Remove`]. If any snack bars
    /// are queued, the next begins its entrance immediately.
    pub fn remove_current_snack_bar(&self) {
        self.shared
            .direct(|core| core.remove_current(SnackBarClosedReason::Remove));
    }

    /// Drops every queued (not-yet-shown) snack bar silently (their
    /// `on_closed` never fires) and hides the current one with its normal
    /// exit animation. See the module docs' "`clear_snack_bars`" section.
    pub fn clear_snack_bars(&self) {
        self.shared.direct(MessengerCore::clear);
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

    fn child(&self) -> &BoxedView {
        &self.child
    }

    fn update_should_notify(&self, _old: &Self) -> bool {
        // The handle is the same stable object across every rebuild — see
        // `Self::maybe_of`'s doc.
        false
    }
}

impl_inherited_view!(ScaffoldMessengerScope);

/// Queues [`SnackBar`]s and shows the current one in each registered
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
    /// The entrance controller's status listener; dropped with the state.
    _entry_status: Subscription,
}

impl std::fmt::Debug for ScaffoldMessengerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScaffoldMessengerState")
            .finish_non_exhaustive()
    }
}

impl StatefulView for ScaffoldMessenger {
    type State = ScaffoldMessengerState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        let handle = ScaffoldMessengerHandle::new(ctx);
        let entry_status = handle.listen_entry_status();
        ScaffoldMessengerState {
            handle,
            _entry_status: entry_status,
        }
    }
}

impl ViewState<ScaffoldMessenger> for ScaffoldMessengerState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.handle.rebind(ctx);
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.handle.rebind(ctx);
    }

    fn build(&self, view: &ScaffoldMessenger, _ctx: &dyn BuildContext) -> impl IntoView {
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
        Rc<flui_animation::__runtime::VsyncDriver>,
    ) {
        let captured = Rc::new(RefCell::new(None));
        let driver = Rc::new(flui_animation::__runtime::VsyncDriver::new());
        let harness =
            flui_testing::widgets::harness::mount(flui_sdk::view::environment::VsyncScope::new(
                driver.vsync().clone(),
                ScaffoldMessenger::new(CaptureMessenger {
                    captured: Rc::clone(&captured),
                }),
            ));
        let handle = captured.borrow().clone().expect("messenger mounted");
        (harness, handle, driver)
    }

    mod failure_cases;

    #[test]
    fn a_panicking_completion_does_not_lock_future_queue_operations() {
        failure_cases::run();
    }
}
