//! Wake-deadline actuation for the AppKit backend (ADR-0353 decision 2).
//!
//! # Why this module exists
//!
//! The application's handler answers the earliest wall-clock instant the
//! event loop should come back at. The Linux loops hand that to their poll
//! timeout and the Android backend checks it once per
//! iteration of its own loop. AppKit has neither: `[NSApplication run]` is an
//! opaque, unbounded run loop, so a deadline nothing actuated was dropped
//! on the floor and the process parked in
//! `_nextEventMatchingEventMask:untilDate:` with the handler never asked.
//! This module is that deadline's landing place.
//!
//! # The two things that make the shape what it is
//!
//! 1. **The hook re-enters `flui-app` and `APP_RUNTIME` is thread-local**, so
//!    it may only be consulted on the owner (main) thread — which rules out a
//!    background deadline thread and picks the main queue as the actuator.
//!    [`owner_queue`] is that queue and `exec_after_on_owner` schedules onto it without
//!    an Objective-C target class, so `[NSApp run]` keeps draining it
//!    unchanged.
//! 2. **A redraw request is not an idle point.** `request_redraw` runs inside
//!    the owner's dispatch, while `flui-app` has a UI runtime checked out, and
//!    the hook's contract is one consult per idle iteration. So
//!    [`WakePump::arm`] never consults: it schedules the first look for the
//!    next owner-queue turn. A frame also arms its fallback deadline *after*
//!    the request that triggered it, so the first look schedules a second one
//!    ([`FOLLOW_UP`]) when it finds nothing — that second look is what picks
//!    the deadline up.
//!
//! The chain starts at a redraw request, and after an owner turn that leaves
//! a deadline (`LoopControl`, [`WakePump::after_owner_turn`]): such a turn can
//! leave one no redraw request starts it for, such as the frame a covered
//! window owes its surface (ADR-0357), whose window asks for no frame while it
//! cannot be seen. A turn that leaves none schedules nothing.
//!
//! Each look arms and delivers the deadline by the rule every native host
//! shares ([`PumpDeadline`] over `WakeDeadline`): a due instant is delivered
//! once, as a frame request to every window, and armed again only after a
//! frame ran, which the windows count into [`WakePump::frame_counter`]. The
//! chain is self-limiting, which is what keeps it from being a busy poll: a
//! look that finds nothing armed stops (the first look of a redraw request
//! looks once more), so a deadline whose frames the handler declines costs one
//! delivery, and an idle app sends no redraw requests, so nothing is ever
//! scheduled for it.

use std::{
    sync::{
        Arc, Weak,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use super::owner_lane::{exec_after_on_owner, owner_queue};

use crate::owner::app_delivery::AppDelivery;
use crate::owner::apple::pump_deadline::{FOLLOW_UP, Look, NextLook, PumpDeadline};
use crate::owner::slot::OwnerCell;
use crate::owner::wake_deadline::FrameCount;

/// The AppKit landing place for the platform's wake deadline.
///
/// Held as `Arc` by [`super::MacOSLoop`] and as `Weak` by each AppKit
/// window, which arms it from `request_redraw`.
pub(crate) struct WakePump {
    /// The loop's delivery, whose handler [`Self::consult`] asks for its wake
    /// deadline, with no lock held.
    app: Arc<AppDelivery>,

    /// Asks this platform's windows for a frame. Set once at construction;
    /// never read under a lock (see [`Self::fire`]).
    wake: Box<dyn Fn() + Send + Sync>,

    /// Supersedes stale ticks. An `arm` or a firing takes the next
    /// generation, and a tick whose generation is no longer current returns
    /// without acting.
    generation: AtomicU64,

    /// The instant the scheduled tick will run at, as microseconds since
    /// [`base`], or `0` when none is scheduled. Read by [`Self::arm`] to
    /// refuse to move a tick LATER — the rule that keeps a redraw burst from
    /// starving the pump (see `arm`).
    ///
    /// A plain atomic rather than a `Mutex<Option<Instant>>`: the value is
    /// read on every redraw request and written on every schedule, and an
    /// atomic keeps both off the lock-discipline surface
    /// (no guard whose drop ordering has to be gotten right) for a value
    /// that carries no
    /// invariant beyond itself.
    pending_at: AtomicU64,

    /// The deadline armed and the last one delivered, the thread's that made
    /// the pump (the main thread, where every tick and owner turn runs).
    deadline: OwnerCell<PumpDeadline>,

    /// The frames this platform's windows' handlers ran, which tell a
    /// deadline a frame left due again from one nothing serviced.
    frames: FrameCount,
}

/// The epoch [`WakePump::pending_at`] counts from.
///
/// `web_time::Instant` has no representation as an integer and no
/// `from_micros`, so the deadline is stored as an offset from a process-wide
/// base fixed at first use. Nothing ever compares these offsets across
/// processes, so the base only has to be stable within one.
fn base() -> web_time::Instant {
    static BASE: std::sync::OnceLock<web_time::Instant> = std::sync::OnceLock::new();
    *BASE.get_or_init(web_time::Instant::now)
}

/// Microseconds from [`base`] to `instant`, for [`WakePump::pending_at`].
///
/// Never `0`, which `pending_at` reserves for "no tick queued": a tick
/// scheduled for the instant the base was fixed (the first `arm`) would
/// otherwise read as none and be superseded by the next arm.
fn micros_since_base(instant: web_time::Instant) -> u64 {
    u64::try_from(instant.saturating_duration_since(base()).as_micros())
        .unwrap_or(u64::MAX)
        .max(1)
}

impl WakePump {
    /// Build the pump. `wake` is invoked on the owner thread when a deadline
    /// has come due; it must be safe to call from there (see
    /// [`super::MacOSLoop`]'s construction site).
    pub(crate) fn new(app: Arc<AppDelivery>, wake: Box<dyn Fn() + Send + Sync>) -> Arc<Self> {
        Arc::new(Self {
            app,
            wake,
            generation: AtomicU64::new(0),
            pending_at: AtomicU64::new(0),
            deadline: OwnerCell::new(),
            frames: FrameCount::default(),
        })
    }

    /// The counter a window's delivery raises for each frame its handler
    /// runs (`WindowDelivery::counting_frames`).
    pub(crate) fn frame_counter(&self) -> Arc<AtomicU64> {
        self.frames.counter()
    }

    /// Ask the pump to make the loop come back. Called from `request_redraw`
    /// (and once as the run binds the application's handler), i.e. whenever a frame is
    /// about to happen.
    pub(crate) fn arm(self: &Arc<Self>) {
        // The hook is not consulted here. The caller is inside the owner's
        // dispatch (a runtime asking its own window for a frame), where the
        // hook finds that runtime checked out; the first look runs on the
        // next owner-queue turn instead, once the dispatch has returned.
        let target = web_time::Instant::now();

        // Only ever move the tick EARLIER. Re-scheduling unconditionally
        // looks harmless, but it is not: while a frame is being asked for
        // over and over (the redraw burst a cold start produces), each
        // request would take a new generation and cancel the tick the
        // previous one scheduled, so no tick would ever run and the pump
        // would be starved by exactly the traffic it exists to survive.
        // Measured: 241 arms, 0 firings, 238 superseded — the deadline armed
        // mid-burst was never consulted at all. Leaving an already-due tick
        // alone is what makes the burst harmless.
        let pending = self.pending_at.load(Ordering::Acquire);
        if pending != 0 && base() + Duration::from_micros(pending) <= target {
            return;
        }

        tracing::trace!(
            target: "flui.pace",
            event = "wake_pump_armed",
            "AppKit wake pump armed"
        );
        self.schedule(target, Look::First);
    }

    /// An owner turn ended, an idle point: look at the wake deadline once and
    /// schedule a tick for it only when one is armed earlier than the tick
    /// queued, so a turn that leaves nothing due, or only an instant already
    /// delivered that no frame has run for since, costs no wake. A turn can
    /// leave a deadline no redraw request starts the pump for: the frame a
    /// covered window owes its surface, whose window asks for no frame while
    /// it cannot be seen (ADR-0357).
    pub(crate) fn after_owner_turn(self: &Arc<Self>) {
        let answer = self.consult();
        let frames = self.frames.get();
        let Some(deadline) = self
            .deadline
            .with(|deadline| deadline.after_owner_turn(answer, frames))
            .flatten()
        else {
            return;
        };
        let pending = self.pending_at.load(Ordering::Acquire);
        if pending != 0 && base() + Duration::from_micros(pending) <= deadline {
            return;
        }
        self.schedule(deadline, Look::Again);
    }

    /// Ask the application's handler for its wake deadline through the call
    /// point, with no lock held, because the handler walks every hosted UI
    /// runtime and takes gesture-arena locks.
    fn consult(&self) -> Option<web_time::Instant> {
        self.app.wake_deadline().or_resume(None)
    }

    /// Schedule one tick for `target`, superseding any tick before it.
    fn schedule(self: &Arc<Self>, target: web_time::Instant, look: Look) {
        self.pending_at
            .store(micros_since_base(target), Ordering::Release);
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        let delay = target.saturating_duration_since(web_time::Instant::now());
        let pump = Arc::clone(self);
        exec_after_on_owner(owner_queue(), delay, move || {
            pump.fire(generation, look);
        });
    }

    /// A scheduled tick: honor the deadline if it is due, otherwise wait for it.
    ///
    /// A tick whose hook or wake panics stops the chain the way a look that
    /// finds nothing armed does, so the next `arm` schedules again instead of
    /// reading the failed tick as still queued; a newer tick the hook
    /// scheduled before it panicked stays queued.
    fn fire(self: &Arc<Self>, generation: u64, look: Look) {
        if self.generation.load(Ordering::Acquire) != generation {
            // A later arm superseded this tick; it owns the deadline now.
            return;
        }
        let unfinished = StopUnlessFinished(Some(Tick {
            pump: self,
            generation,
            pending_at: self.pending_at.load(Ordering::Acquire),
        }));
        self.run_tick(look);
        unfinished.finish();
    }

    /// What a current tick does.
    fn run_tick(self: &Arc<Self>, look: Look) {
        let answer = self.consult();
        let now = web_time::Instant::now();
        let frames = self.frames.get();
        // Nothing armed on a first look is the common case right after a
        // redraw request and is NOT "nothing to do": the frame that request
        // triggers may not have run, so the deadline it arms, or the frame
        // that lets a delivered one be armed again, is not visible yet. The
        // look then schedules a second one; a second that still finds nothing
        // stops. Off the thread that made the pump, or with a cell the
        // exhausted owner slots left empty, nothing is armed.
        let next = self
            .deadline
            .with(|deadline| deadline.look(look, answer, now, frames))
            .unwrap_or(NextLook::Stop);
        match next {
            NextLook::Stop => {
                // Nothing armed and this tick requested no frame, so there
                // is nothing left to come back for. Stopping HERE is what
                // keeps an idle app from ticking: it sends no redraw
                // requests, so this is the only place the chain could
                // restart itself.
                //
                // Clear `pending_at` with the stop. This tick has run, so no
                // tick is queued any more; leaving the (now-expired) instant
                // behind would make every later `arm()` see a pending tick
                // earlier than its own target and return without scheduling —
                // `arm` reads a nonzero `pending_at` as "a tick is still
                // queued", which is no longer true. The next real deadline
                // (no-present fallback, device recovery) would then never
                // wake the loop. The store must happen before the trace so a
                // reader of the field after this arm never sees the stopped
                // tick as live.
                self.pending_at.store(0, Ordering::Release);
                tracing::trace!(
                    target: "flui.pace",
                    event = "wake_pump_idle",
                    "AppKit wake pump found no deadline armed; chain stopped"
                );
            }
            NextLook::Wake { late } => {
                tracing::trace!(
                    target: "flui.pace",
                    event = "wake_pump_fired",
                    late_us = u64::try_from(late.as_micros()).unwrap_or(u64::MAX),
                    "AppKit wake pump firing a frame for a due deadline"
                );
                (self.wake)();
                // The frame just requested runs after this block returns
                // (AppKit coalesces it into a display pass), so it may arm a
                // deadline this consult could not have seen — look again
                // shortly, the same reason `arm` does.
                self.schedule(web_time::Instant::now() + FOLLOW_UP, Look::Again);
            }
            // The armed deadline ahead, or the second look of a redraw
            // request.
            NextLook::At(at) => self.schedule(at, Look::Again),
        }
    }
}

/// The tick a [`StopUnlessFinished`] guards: its generation and the
/// `pending_at` it found.
struct Tick<'a> {
    pump: &'a WakePump,
    generation: u64,
    pending_at: u64,
}

/// Clears [`WakePump::pending_at`] when dropped before [`Self::finish`]: a tick
/// that unwound has scheduled nothing, so no tick is queued, unless the hook
/// scheduled a newer one before it panicked (a new generation and instant),
/// which stays.
struct StopUnlessFinished<'a>(Option<Tick<'a>>);

impl StopUnlessFinished<'_> {
    /// The tick ran to its end and left `pending_at` as its plan set it.
    fn finish(mut self) {
        self.0 = None;
    }
}

impl Drop for StopUnlessFinished<'_> {
    fn drop(&mut self) {
        if let Some(tick) = self.0.take()
            && tick.pump.generation.load(Ordering::Acquire) == tick.generation
        {
            let _ = tick.pump.pending_at.compare_exchange(
                tick.pending_at,
                0,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
    }
}

/// Weak handle a window's redraw requester stores (see `MacOSWindow::install_wake_pump`).
pub(crate) type WeakWakePump = Weak<WakePump>;

#[cfg(test)]
mod tests {
    use super::*;

    use crate::owner::app_delivery::testing::{AppLog, Bound, bind};
    use std::rc::Rc;

    /// A pump whose loop's delivery is bound to a recording handler, which
    /// answers no deadline until a test says otherwise.
    fn silent_pump() -> (Arc<WakePump>, Rc<AppLog>, Bound) {
        let app = Arc::new(AppDelivery::new());
        let pump = WakePump::new(
            Arc::clone(&app),
            Box::new(|| panic!("this pump must not request a frame")),
        );
        let handler = AppLog::new();
        let run = bind(app, handler.clone());
        (pump, handler, run)
    }

    #[test]
    fn an_arm_never_moves_a_pending_tick_later() {
        // The anti-starvation rule. Without it a redraw burst cancels its own
        // ticks: every request takes a generation, so the tick the previous
        // one scheduled never runs — measured as 241 arms, 0 firings, 238
        // superseded, with the deadline armed in the middle of the burst
        // never consulted at all.
        let (pump, _app, _run) = silent_pump();
        // A tick already due: an earlier request of the burst scheduled it
        // for its own turn. This arm's target is now, which is no earlier, so
        // the pending tick must be left alone.
        let due = micros_since_base(web_time::Instant::now());
        pump.pending_at.store(due, Ordering::Release);
        // `pending_at` is whole microseconds from the base and never 0, so
        // the tick stored can lie up to a microsecond ahead of now: an arm
        // inside that microsecond is earlier, and moves it. Let it come due.
        while base().elapsed() <= Duration::from_micros(due) {
            std::hint::spin_loop();
        }
        let before = pump.generation.load(Ordering::Acquire);
        pump.arm();
        assert_eq!(
            pump.generation.load(Ordering::Acquire),
            before,
            "an arm later than the pending tick must not re-schedule it"
        );
    }

    /// A redraw request runs inside the owner's dispatch, where `flui-app`
    /// has the asking runtime checked out and its hook answers `Busy`; the
    /// counter app aborted on start that way. The arm schedules the look
    /// instead of taking it. Red-check: consult in `arm` again.
    #[test]
    fn an_arm_schedules_the_look_instead_of_consulting() {
        use std::sync::atomic::AtomicUsize;

        let (pump, app, _run) = silent_pump();
        let consults = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&consults);
        app.answer_deadline_with(move || -> Option<web_time::Instant> {
            counted.fetch_add(1, Ordering::AcqRel);
            None
        });
        let before = pump.generation.load(Ordering::Acquire);

        pump.arm();

        assert_eq!(
            consults.load(Ordering::Acquire),
            0,
            "the arm consulted the hook inside the caller's dispatch"
        );
        assert_ne!(
            pump.generation.load(Ordering::Acquire),
            before,
            "the arm scheduled no look"
        );
        assert_ne!(
            pump.pending_at.load(Ordering::Acquire),
            0,
            "the scheduled look does not read as queued"
        );
    }

    /// The first look finds no deadline yet — the frame the request triggered
    /// arms it later — and looks once more instead of stopping. Red-check:
    /// stop on an empty first look.
    #[test]
    fn an_empty_first_look_looks_again() {
        let (pump, app, _run) = silent_pump();
        app.answer_deadline_with(|| -> Option<web_time::Instant> { None });
        let generation = pump.generation.load(Ordering::Acquire);

        pump.fire(generation, Look::First);

        assert_ne!(
            pump.pending_at.load(Ordering::Acquire),
            0,
            "an empty first look stopped the chain"
        );
        let generation = pump.generation.load(Ordering::Acquire);
        pump.fire(generation, Look::Again);
        assert_eq!(
            pump.pending_at.load(Ordering::Acquire),
            0,
            "an empty second look kept the chain going"
        );
    }

    /// A tick whose hook panics is contained by the GCD boundary, and must not
    /// leave the expired tick looking queued: `arm` would then refuse every
    /// later deadline. Red-check: drop the `StopUnlessFinished` guard.
    #[test]
    fn a_tick_whose_hook_panics_leaves_no_tick_queued() {
        let (pump, app, _run) = silent_pump();
        app.answer_deadline_with(|| -> Option<web_time::Instant> { panic!("a failing wake hook") });
        // The tick being fired: a nonzero instant, whatever the base.
        pump.pending_at.store(
            micros_since_base(web_time::Instant::now() + Duration::from_millis(5)),
            Ordering::Release,
        );
        let generation = pump.generation.load(Ordering::Acquire);

        let fired = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            pump.fire(generation, Look::Again);
        }));

        assert!(
            fired.is_err(),
            "the hook's panic reaches the tick's boundary"
        );
        assert_eq!(
            pump.pending_at.load(Ordering::Acquire),
            0,
            "the failed tick still reads as queued"
        );
    }

    /// A hook that scheduled a newer tick before it panicked keeps that tick:
    /// only the failed tick stops reading as queued. Red-check: clear
    /// `pending_at` without comparing the generation and instant.
    #[test]
    fn a_tick_whose_hook_rearms_then_panics_keeps_the_newer_tick() {
        use std::sync::atomic::AtomicBool;

        let (pump, app, _run) = silent_pump();
        let weak = Arc::downgrade(&pump);
        let rearmed = AtomicBool::new(false);
        app.answer_deadline_with(move || -> Option<web_time::Instant> {
            if !rearmed.swap(true, Ordering::AcqRel)
                && let Some(pump) = weak.upgrade()
            {
                pump.schedule(
                    web_time::Instant::now() + Duration::from_millis(50),
                    Look::Again,
                );
            }
            panic!("a wake hook that re-armed, then failed")
        });
        pump.pending_at.store(
            micros_since_base(web_time::Instant::now() + Duration::from_millis(5)),
            Ordering::Release,
        );
        let generation = pump.generation.load(Ordering::Acquire);

        let fired = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            pump.fire(generation, Look::Again);
        }));

        assert!(
            fired.is_err(),
            "the hook's panic reaches the tick's boundary"
        );
        assert_ne!(
            pump.pending_at.load(Ordering::Acquire),
            0,
            "the newer tick the hook scheduled no longer reads as queued"
        );
    }
}
